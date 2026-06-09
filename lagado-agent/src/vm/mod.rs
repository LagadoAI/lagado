use std::process::{Child, Command, Stdio};
use std::sync::{Arc, RwLock};

mod qmp;
pub mod ssh_actuator;
pub mod ssh_perceptor;

pub use qmp::QmpClient;
pub use ssh_actuator::SshActuator;
pub use ssh_perceptor::SshPerceptor;

pub struct VmConfig {
    pub disk_image: String,
    pub seed_iso: Option<String>,
    pub mem_mib: u32,
    pub vcpus: u32,
    pub ssh_port: u16,
    pub qmp_socket: String,
}

impl Default for VmConfig {
    fn default() -> Self {
        let data_dir = std::env::var("LAGADO_DATA_DIR").unwrap_or_else(|_| {
            format!("{}/.laputa-secure", std::env::var("HOME").unwrap_or_default())
        });
        let seed = format!("{data_dir}/vm-images/seed.iso");
        Self {
            disk_image: format!("{data_dir}/vm-images/Arch-Linux-x86_64-cloudimg.qcow2"),
            seed_iso: if std::path::Path::new(&seed).exists() { Some(seed) } else { None },
            mem_mib: 4096,
            vcpus: 4,
            ssh_port: 2222,
            qmp_socket: "/tmp/lagado-qmp.sock".to_string(),
        }
    }
}

pub struct VmHandle {
    pub child: Child,
    pub qmp_socket: String,
    pub ssh_port: u16,
}

impl Drop for VmHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Shared SSH port for the active VM. Some(port) when running, None otherwise.
/// Written by vm_boot/vm_stop, read synchronously by Dynamic* wrappers.
pub type VmSshPort = Arc<RwLock<Option<u16>>>;

/// Routes actuator calls through SSH when a VM is active, host impl otherwise.
pub struct DynamicActuator {
    pub vm_port: VmSshPort,
    pub host: Arc<dyn crate::perception::Actuator + Send + Sync>,
}

/// Routes perceptor calls through SSH when a VM is active, host impl otherwise.
pub struct DynamicPerceptor {
    pub vm_port: VmSshPort,
    pub host: Arc<dyn crate::perception::Perceptor + Send + Sync>,
}

impl crate::perception::Actuator for DynamicActuator {
    fn click(&self, selector: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::new("127.0.0.1", port, "laputa").click(selector)
        } else {
            self.host.click(selector)
        }
    }
    fn type_text(&self, selector: &str, text: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::new("127.0.0.1", port, "laputa").type_text(selector, text)
        } else {
            self.host.type_text(selector, text)
        }
    }
    fn key(&self, key: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::new("127.0.0.1", port, "laputa").key(key)
        } else {
            self.host.key(key)
        }
    }
}

impl crate::perception::Perceptor for DynamicPerceptor {
    fn read_screen(&self) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshPerceptor::new("127.0.0.1", port, "laputa").read_screen()
        } else {
            self.host.read_screen()
        }
    }
}

pub trait VmBackend: Send + Sync {
    fn boot(&self, cfg: &VmConfig) -> Result<VmHandle, String>;
    fn shutdown(&self, h: VmHandle) -> Result<(), String>;
}

pub struct QemuDesktopBackend {
    pub qemu_path: String,
}

impl Default for QemuDesktopBackend {
    fn default() -> Self {
        Self { qemu_path: "qemu-system-x86_64".to_string() }
    }
}

impl VmBackend for QemuDesktopBackend {
    fn boot(&self, cfg: &VmConfig) -> Result<VmHandle, String> {
        let _ = std::fs::remove_file(&cfg.qmp_socket);
        let mut cmd = Command::new(&self.qemu_path);
        cmd.args([
            "-enable-kvm",
            "-cpu", "host",
            "-m", &format!("{}M", cfg.mem_mib),
            "-smp", &cfg.vcpus.to_string(),
            "-drive", &format!("file={},format=qcow2,if=virtio", cfg.disk_image),
            "-device", "virtio-vga,xres=1280,yres=800",
            "-display", "none",
            "-qmp", &format!("unix:{},server,nowait", cfg.qmp_socket),
            "-netdev", &format!("user,id=net0,hostfwd=tcp::{}-:22", cfg.ssh_port),
            "-device", "virtio-net-pci,netdev=net0",
            "-boot", "order=c,menu=off",
            "-serial", "null",
        ]);
        if let Some(ref iso) = cfg.seed_iso {
            cmd.args(["-cdrom", iso]);
        }
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| format!("qemu-system-x86_64 spawn failed: {e}"))?;
        Ok(VmHandle {
            child,
            qmp_socket: cfg.qmp_socket.clone(),
            ssh_port: cfg.ssh_port,
        })
    }

    fn shutdown(&self, mut h: VmHandle) -> Result<(), String> {
        if let Ok(mut qmp) = QmpClient::connect(&h.qmp_socket) {
            let _ = qmp.send_command("system_powerdown", None);
            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if let Ok(Some(_)) = h.child.try_wait() {
                    return Ok(());
                }
            }
        }
        h.child.kill().map_err(|e| format!("kill failed: {e}"))?;
        h.child.wait().map_err(|e| format!("wait failed: {e}"))?;
        Ok(())
    }
}
