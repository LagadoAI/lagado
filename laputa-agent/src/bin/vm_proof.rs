use std::time::Instant;
use lagado_agent::vm::{QemuMicrovmBackend, VmBackend, VmConfig};

fn main() {
    let t0 = Instant::now();
    println!("[vm_proof] booting QEMU microvm...");

    let backend = QemuMicrovmBackend::default();
    let cfg = VmConfig {
        vcpus: 1,
        mem_mib: 128,
        kernel: "/usr/lib/modules/6.18.31-1-cachyos-lts/vmlinuz".to_string(),
        initrd: Some("/tmp/lagado-initrd.cpio.gz".to_string()),
        cmdline: "console=ttyS0 panic=-1 quiet".to_string(),
    };

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
