use std::process::{Child, Command, Stdio};

pub struct VmConfig {
    pub vcpus: u32,
    pub mem_mib: u32,
    pub kernel: String,
    pub initrd: Option<String>,
    pub cmdline: String,
}

pub struct VmHandle {
    pub child: Child,
}

/// Abstraction over microVM backends.
///
/// libkrun is a future backend behind this trait (parked in obsolete/, not deleted).
/// Current active backend: QemuMicrovmBackend.
pub trait VmBackend: Send + Sync {
    fn boot(&self, cfg: &VmConfig) -> Result<VmHandle, String>;
    fn shutdown(&self, h: VmHandle) -> Result<(), String>;
}

pub struct QemuMicrovmBackend {
    pub qemu_path: String,
}

impl Default for QemuMicrovmBackend {
    fn default() -> Self {
        Self { qemu_path: "qemu-system-x86_64".to_string() }
    }
}

impl VmBackend for QemuMicrovmBackend {
    fn boot(&self, cfg: &VmConfig) -> Result<VmHandle, String> {
        let mut cmd = Command::new(&self.qemu_path);
        cmd.args([
            "-M", "microvm,acpi=on",
            "-enable-kvm",
            "-cpu", "host",
            "-m", &format!("{}M", cfg.mem_mib),
            "-kernel", &cfg.kernel,
            "-append", &cfg.cmdline,
            "-serial", "stdio",
            "-nodefaults",
            "-no-user-config",
            "-nographic",
        ]);
        if let Some(ref initrd) = cfg.initrd {
            cmd.args(["-initrd", initrd]);
        }
        // Silence SeaBIOS/QEMU stderr noise; guest serial goes to inherited stdout.
        cmd.stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| format!("qemu spawn failed: {e}"))?;
        Ok(VmHandle { child })
    }

    fn shutdown(&self, mut h: VmHandle) -> Result<(), String> {
        // Normal path: guest init calls LINUX_REBOOT_CMD_POWER_OFF → QEMU exits cleanly.
        // Fallback: kill if still running.
        match h.child.try_wait() {
            Ok(Some(_)) => Ok(()),
            _ => {
                h.child.kill().map_err(|e| format!("kill failed: {e}"))?;
                h.child.wait().map_err(|e| format!("wait failed: {e}"))?;
                Ok(())
            }
        }
    }
}
