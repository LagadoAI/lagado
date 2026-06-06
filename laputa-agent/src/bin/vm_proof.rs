use std::ffi::CString;
use std::fs::OpenOptions;
use std::os::raw::c_char;
use std::os::unix::io::IntoRawFd;
use std::ptr;
use std::time::Instant;

use krun_sys::{
    krun_add_virtio_console_default, krun_create_ctx, krun_disable_implicit_console,
    krun_set_exec, krun_set_log_level, krun_set_root, krun_set_vm_config, krun_start_enter,
};

// Minimal virtiofs rootfs: static probe binary + dirs for proc/sys/dev.
//
// KNOWN ISSUE (non-production): still host-filesystem paths — not a
// sandboxed container image.  Production must use a dedicated image.
const ROOTFS: &str = "/tmp/vm-root";

// Cmdline under test — patched into libkrun binary via LD_LIBRARY_PATH.
// Base: "reboot=k panic=-1 panic_print=0 nomodule console=hvc0 rootfstype=virtiofs rw quiet no-kvmapf"
// Active patch: replaced "quiet no-kvmapf" with "clocksource=tsc" (same 15 bytes).
const ACTIVE_PATCH: &str = "clocksource=tsc"; // arg (a)

fn main() {
    let t0 = Instant::now();
    println!("[vm_proof] starting microVM boot...");
    println!("[vm_proof] rootfs = {ROOTFS} (minimal tree — NOT isolated)");
    println!("[vm_proof] patched cmdline tail: {ACTIVE_PATCH}");

    unsafe {
        krun_set_log_level(1);

        let ctx = krun_create_ctx();
        if ctx < 0 {
            eprintln!("[vm_proof] krun_create_ctx failed: {ctx}");
            std::process::exit(1);
        }
        let ctx = ctx as u32;

        let r = krun_set_vm_config(ctx, 1, 2048);
        if r < 0 {
            eprintln!("[vm_proof] krun_set_vm_config failed: {r}");
            std::process::exit(1);
        }

        let root = CString::new(ROOTFS).unwrap();
        let r = krun_set_root(ctx, root.as_ptr());
        if r < 0 {
            eprintln!("[vm_proof] krun_set_root failed: {r}");
            std::process::exit(1);
        }

        // Cmdline is patched directly into the libkrun binary (LD_LIBRARY_PATH override).
        // krun_set_kernel with external kernel not viable for the embedded libkrunfw bundle.

        let r = krun_disable_implicit_console(ctx);
        if r < 0 {
            eprintln!("[vm_proof] krun_disable_implicit_console failed: {r}");
            std::process::exit(1);
        }

        let null_in = OpenOptions::new().read(true).open("/dev/null")
            .expect("open /dev/null").into_raw_fd();
        let r = krun_add_virtio_console_default(ctx, null_in, 1, 2);
        if r < 0 {
            eprintln!("[vm_proof] krun_add_virtio_console_default failed: {r}");
            std::process::exit(1);
        }

        let exec = CString::new("/bin/probe").unwrap();
        let a0 = CString::new("probe").unwrap();
        let argv: [*const c_char; 2] = [a0.as_ptr(), ptr::null()];

        let r = krun_set_exec(ctx, exec.as_ptr(), argv.as_ptr(), ptr::null());
        if r < 0 {
            eprintln!("[vm_proof] krun_set_exec failed: {r}");
            std::process::exit(1);
        }

        println!("[vm_proof] setup done in {:?}, entering VM...", t0.elapsed());

        let r = krun_start_enter(ctx);

        if r >= 0 {
            println!("[vm_proof] guest exited (code={r}), total: {:?}", t0.elapsed());
        } else {
            eprintln!("[vm_proof] krun_start_enter returned error: {r}");
            std::process::exit(1);
        }
    }
}
