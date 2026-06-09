//! pty.rs — PTY session management.
//!
//! Phase 1: Unix PTY via nix crate. Windows: stub.
//! Each session = one shell process with its own PTY.

use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;

pub struct PtySession {
    pub id: String,
    pub title: String,
    pub cols: u16,
    pub rows: u16,
    output_buf: Arc<Mutex<Vec<u8>>>,
    alive: Arc<std::sync::atomic::AtomicBool>,
}

pub struct PtyManager {
    sessions: HashMap<String, PtySession>,
}

impl PtySession {
    pub fn new(id: &str, title: &str, cols: u16, rows: u16) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            cols,
            rows,
            output_buf: Arc::new(Mutex::new(Vec::new())),
            alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    /// Spawn a shell under this PTY. Returns Err on unsupported platforms.
    pub async fn spawn(&self, shell: &str) -> Result<(), String> {
        #[cfg(unix)]
        return self.spawn_unix(shell).await;

        #[cfg(not(unix))]
        return Err("PTY not supported on this platform (Phase 2)".to_string());
    }

    #[cfg(unix)]
    async fn spawn_unix(&self, shell: &str) -> Result<(), String> {
        use std::process::Command;
        let output_buf = self.output_buf.clone();
        let alive = self.alive.clone();
        let shell = shell.to_string();

        tokio::task::spawn_blocking(move || {
            // Phase 1: simple subprocess, not a true PTY
            // Phase 2: use nix openpty() for full PTY support
            let mut child = Command::new(&shell)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("shell spawn failed: {e}"))?;

            if let Some(mut stdout) = child.stdout.take() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let rt = tokio::runtime::Handle::try_current();
                            if let Ok(handle) = rt {
                                let ob = output_buf.clone();
                                let data = buf[..n].to_vec();
                                handle.spawn(async move {
                                    ob.lock().await.extend_from_slice(&data);
                                });
                            }
                        }
                    }
                }
            }
            alive.store(false, std::sync::atomic::Ordering::SeqCst);
            Ok::<(), String>(())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Read available output (non-blocking, returns what's buffered).
    pub async fn read_output(&self) -> Vec<u8> {
        let mut buf = self.output_buf.lock().await;
        std::mem::take(&mut *buf)
    }

    /// Write input to the PTY (Phase 1: no-op, Phase 2: write to PTY fd).
    pub async fn write_input(&self, data: &[u8]) -> Result<(), String> {
        tracing::debug!("PTY input ({} bytes) — full write in Phase 2", data.len());
        Ok(())
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Create and register a new PTY session.
    pub fn create_session(&mut self, title: &str, cols: u16, rows: u16) -> &PtySession {
        let id = uuid::Uuid::new_v4().to_string();
        let session = PtySession::new(&id, title, cols, rows);
        self.sessions.entry(id.clone()).or_insert(session);
        self.sessions.get(&id).unwrap()
    }

    pub fn get(&self, id: &str) -> Option<&PtySession> {
        self.sessions.get(id)
    }

    pub fn remove(&mut self, id: &str) {
        self.sessions.remove(id);
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Remove sessions whose shell process has exited.
    pub fn reap_dead(&mut self) {
        self.sessions.retain(|_, s| s.is_alive());
    }
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}
