use std::process::{Child, Command, Stdio};
use std::sync::{Arc, RwLock};

use crate::perception::PerceptionCache;

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
        let seed = format!("{data_dir}/vm-images/seed-fedora.iso");
        Self {
            disk_image: format!("{data_dir}/vm-images/lagado-guest-fedora.qcow2"),
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
    pub ssh_cache: Arc<std::sync::Mutex<PerceptionCache>>,
    pub host: Arc<dyn crate::perception::Actuator + Send + Sync>,
}

/// Routes perceptor calls through SSH when a VM is active, host impl otherwise.
pub struct DynamicPerceptor {
    pub vm_port: VmSshPort,
    pub ssh_cache: Arc<std::sync::Mutex<PerceptionCache>>,
    pub host: Arc<dyn crate::perception::Perceptor + Send + Sync>,
}

impl crate::perception::Actuator for DynamicActuator {
    fn click(&self, selector: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::with_cache("127.0.0.1", port, "laputa", self.ssh_cache.clone()).click(selector)
        } else {
            self.host.click(selector)
        }
    }
    fn type_text(&self, selector: &str, text: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::with_cache("127.0.0.1", port, "laputa", self.ssh_cache.clone()).type_text(selector, text)
        } else {
            self.host.type_text(selector, text)
        }
    }
    fn key(&self, key: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::with_cache("127.0.0.1", port, "laputa", self.ssh_cache.clone()).key(key)
        } else {
            self.host.key(key)
        }
    }
    fn run_command(&self, cmd: &str) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshActuator::with_cache("127.0.0.1", port, "laputa", self.ssh_cache.clone()).run_command(cmd)
        } else {
            self.host.run_command(cmd)
        }
    }
}

impl crate::perception::Perceptor for DynamicPerceptor {
    fn read_screen(&self) -> String {
        if let Some(port) = *self.vm_port.read().unwrap_or_else(|e| e.into_inner()) {
            SshPerceptor::with_cache("127.0.0.1", port, "laputa", self.ssh_cache.clone()).read_screen()
        } else {
            self.host.read_screen()
        }
    }

    fn capture_frame(&self) {
        // VM path → QMP screendump (SshPerceptor); host path → whatever the host perceptor does.
        if self.vm_port.read().unwrap_or_else(|e| e.into_inner()).is_some() {
            SshPerceptor::with_cache("127.0.0.1", 0, "laputa", self.ssh_cache.clone()).capture_frame();
        } else {
            self.host.capture_frame();
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

/// Kill any orphaned QEMU process that holds the ssh port forward we intend to use.
///
/// Detection: `pgrep -f "hostfwd=tcp::{ssh_port}-:22"` — matches only on the
/// exact hostfwd argument emitted by our boot() call, so unrelated processes
/// are never touched.  SIGTERM first, poll ≤2 s, SIGKILL if still alive.
#[cfg(unix)]
fn kill_stale_vm(ssh_port: u16) {
    let pattern = format!("hostfwd=tcp::{ssh_port}-:22");
    let output = match Command::new("pgrep").args(["-f", &pattern]).output() {
        Ok(o) => o,
        Err(_) => return,
    };
    if !output.status.success() {
        return; // no matching processes
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let pid: u32 = match line.trim().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        tracing::warn!("kill_stale_vm: orphaned qemu pid={pid} holds port {ssh_port}, sending SIGTERM");
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
        // Poll up to 2 s for graceful exit
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let alive = Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !alive || std::time::Instant::now() >= deadline {
                break;
            }
        }
        // Force-kill if still alive after grace period
        let still_alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if still_alive {
            tracing::warn!("kill_stale_vm: pid={pid} still alive after SIGTERM, sending SIGKILL");
            let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).status();
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

impl VmBackend for QemuDesktopBackend {
    fn boot(&self, cfg: &VmConfig) -> Result<VmHandle, String> {
        // Kill any stale lagado VM holding the ssh port forward or QMP socket
        // before removing the socket file — otherwise the spawned QEMU can never
        // receive SSH connections (port collision) and QMP is also blocked.
        #[cfg(unix)]
        kill_stale_vm(cfg.ssh_port);

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
        // QEMU's native seccomp sandbox — Linux-only; safe no-op on other platforms
        cmd.args(crate::security::sandbox::qemu_sandbox_args());
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| format!("qemu-system-x86_64 spawn failed: {e}"))?;

        // cgroup v2 memory + pid limits — best-effort, logged on failure
        {
            let mem_cap = (cfg.mem_mib as u64 + cfg.mem_mib as u64 / 2) * 1024 * 1024;
            if let Err(e) = crate::security::sandbox::apply_limits(child.id(), "qemu", mem_cap, 512) {
                tracing::warn!("sandbox: qemu: {e}");
            }
        }

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
