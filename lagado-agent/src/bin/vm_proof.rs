use std::time::Instant;
use lagado_agent::vm::{QemuDesktopBackend, VmBackend, VmConfig};

fn main() {
    let t0 = Instant::now();
    println!("[vm_proof] booting QEMU desktop VM...");

    let backend = QemuDesktopBackend::default();
    let cfg = VmConfig::default();

    let mut handle = match backend.boot(&cfg) {
        Ok(h) => h,
        Err(e) => { eprintln!("[vm_proof] boot failed: {e}"); std::process::exit(1); }
    };

    println!("[vm_proof] VM started ({:?}), waiting for guest exit...", t0.elapsed());

    match handle.child.wait() {
        Ok(status) => {
            println!("[vm_proof] guest exited: {status}, total: {:?}", t0.elapsed());
            if !status.success() { std::process::exit(1); }
        }
        Err(e) => { eprintln!("[vm_proof] wait failed: {e}"); std::process::exit(1); }
    }
}
