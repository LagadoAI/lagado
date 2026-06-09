use crate::perception::Actuator;

pub struct SshActuator {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl Default for SshActuator {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 2222,
            user: "laputa".to_string(),
        }
    }
}

impl SshActuator {
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self { host: host.to_string(), port, user: user.to_string() }
    }

    fn ssh_run(&self, cmd: &str) -> String {
        match std::process::Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &self.port.to_string(),
                &format!("{}@{}", self.user, self.host),
                cmd,
            ])
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(e) => format!("ssh error: {e}"),
        }
    }
}

impl Actuator for SshActuator {
    fn click(&self, selector: &str) -> String {
        self.ssh_run(&format!("DISPLAY=:0 xdotool mousemove {selector} click 1"))
    }

    fn type_text(&self, _selector: &str, text: &str) -> String {
        self.ssh_run(&format!("DISPLAY=:0 xdotool type --clearmodifiers -- {text:?}"))
    }

    fn key(&self, key: &str) -> String {
        self.ssh_run(&format!("DISPLAY=:0 xdotool key {key}"))
    }
}
