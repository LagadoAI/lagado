use crate::perception::Perceptor;

pub struct SshPerceptor {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl Default for SshPerceptor {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 2222,
            user: "laputa".to_string(),
        }
    }
}

impl SshPerceptor {
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self { host: host.to_string(), port, user: user.to_string() }
    }
}

impl Perceptor for SshPerceptor {
    fn read_screen(&self) -> String {
        match std::process::Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &self.port.to_string(),
                &format!("{}@{}", self.user, self.host),
                "DISPLAY=:0 python3 ~/perceive.py 2>/dev/null || echo '[perception unavailable]'",
            ])
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(e) => format!("[ssh error: {e}]"),
        }
    }
}
