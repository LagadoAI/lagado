use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;
use std::sync::OnceLock;

use krun_sys::{
    krun_create_ctx, krun_set_console_output, krun_set_exec, krun_set_log_level,
    krun_set_root, krun_set_vm_config, krun_start_enter,
};

// libkrun's logger initialises once per process — subsequent calls are a no-op
// at best and undefined at worst.
static LOG_INIT: OnceLock<()> = OnceLock::new();

fn init_log() {
    LOG_INIT.get_or_init(|| unsafe {
        krun_set_log_level(1); // error level only
    });
}

/// Boot a microVM, run /bin/echo "lagado-vm-ok", then exit.
///
/// KNOWN ISSUE: rootfs = host "/".  Not isolated — guest can read the full
/// host filesystem via virtiofs passthrough.  For production, use a sandboxed
/// image instead.
///
/// krun_start_enter does not return on success when the process exits with the
/// guest.  On error it returns before entering the VM.
pub fn boot_test(console_out: &str) {
    init_log();

    unsafe {
        let ctx = krun_create_ctx();
        if ctx < 0 {
            eprintln!("[vm] krun_create_ctx failed: {ctx}");
            return;
        }
        let ctx = ctx as u32;

        let r = krun_set_vm_config(ctx, 1, 512);
        if r < 0 {
            eprintln!("[vm] krun_set_vm_config failed: {r}");
            return;
        }

        let root = CString::new("/").unwrap();
        let r = krun_set_root(ctx, root.as_ptr());
        if r < 0 {
            eprintln!("[vm] krun_set_root failed: {r}");
            return;
        }

        // Wire the implicit console to a file so libkrun's init can set up
        // the app's stdio without blocking (a pipe context has no TTY).
        let out_path = CString::new(console_out).unwrap();
        let r = krun_set_console_output(ctx, out_path.as_ptr());
        if r < 0 {
            eprintln!("[vm] krun_set_console_output failed: {r}");
            return;
        }

        let exec = CString::new("/bin/echo").unwrap();
        let a0 = CString::new("echo").unwrap();
        let a1 = CString::new("lagado-vm-ok").unwrap();
        let argv: [*const c_char; 3] = [a0.as_ptr(), a1.as_ptr(), ptr::null()];

        let r = krun_set_exec(ctx, exec.as_ptr(), argv.as_ptr(), ptr::null());
        if r < 0 {
            eprintln!("[vm] krun_set_exec failed: {r}");
            return;
        }

        // TSI networking is the default; no explicit network setup required.
        let r = krun_start_enter(ctx);
        eprintln!("[vm] krun_start_enter returned error: {r}");
    }
}
