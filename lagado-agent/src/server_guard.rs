use std::sync::Arc;
use tokio::sync::Mutex;
use crate::bootstrap::{KillOnDrop, check_health_sync, ensure_llama_server, ensure_classifier_server};
use crate::config;

const POLL_SECS: u64 = 10;
const FAILURE_THRESHOLD: u32 = 3;
const HEALTH_TIMEOUT_SECS: u64 = 3;

pub enum ServerEvent {
    Crashed { server: &'static str },
    Restarted { server: &'static str },
    RestartFailed { server: &'static str },
}

// ── Pure state machine — no I/O, fully unit-testable ─────────────────────────

struct ServerHealth {
    /// True if we own the binary/model and can restart the server.
    /// Set once at construction from filesystem state; never mutates.
    manages: bool,
    /// Startup grace: don't count failures before the first successful check.
    ever_healthy: bool,
    failures: u32,
}

enum HealthAction {
    Nothing,
    Restart,
}

impl ServerHealth {
    fn new(manages: bool) -> Self {
        Self { manages, ever_healthy: false, failures: 0 }
    }

    /// Advance the state machine with the result of one health poll.
    /// Returns `Restart` exactly when FAILURE_THRESHOLD consecutive failures
    /// follow a prior healthy observation and we manage the server.
    /// After returning `Restart`, resets `failures` to 0 — so a failed restart
    /// (where `ever_healthy` stays true) will naturally retry after another
    /// FAILURE_THRESHOLD failures, indefinitely.
    fn tick(&mut self, healthy: bool) -> HealthAction {
        if healthy {
            self.ever_healthy = true;
            self.failures = 0;
            return HealthAction::Nothing;
        }
        // Grace period: ignore failures until we've confirmed the server came up.
        // Also: nothing to do if we can't restart (no binary/model).
        if !self.manages || !self.ever_healthy {
            return HealthAction::Nothing;
        }
        self.failures += 1;
        if self.failures >= FAILURE_THRESHOLD {
            self.failures = 0;
            HealthAction::Restart
        } else {
            HealthAction::Nothing
        }
    }
}

// ── ServerGuard ───────────────────────────────────────────────────────────────

pub struct ServerGuard {
    llama_child: Arc<Mutex<Option<KillOnDrop>>>,
    classifier_child: Arc<Mutex<Option<KillOnDrop>>>,
    on_event: Box<dyn Fn(ServerEvent) + Send + Sync + 'static>,
}

impl ServerGuard {
    pub fn new(
        llama_child: Arc<Mutex<Option<KillOnDrop>>>,
        classifier_child: Arc<Mutex<Option<KillOnDrop>>>,
        on_event: impl Fn(ServerEvent) + Send + Sync + 'static,
    ) -> Self {
        Self { llama_child, classifier_child, on_event: Box::new(on_event) }
    }

    pub async fn run(self) {
        // `manages` is determined from filesystem at startup: if we have the binary
        // and model we can restart, regardless of whether we originally spawned the
        // server (handles the "reuse external" path in ensure_llama_server too).
        let manages_llama = config::llama_server_bin().exists() && config::model_path().exists();
        let manages_classifier = config::llama_server_bin().exists()
            && config::classifier_model_path().exists();

        let mut llama_health = ServerHealth::new(manages_llama);
        let mut classifier_health = ServerHealth::new(manages_classifier);

        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(POLL_SECS)).await;

            // ── Main llama-server (8080) ───────────────────────────────────
            let llama_url = config::llama_base_url();
            let llama_up = tokio::task::spawn_blocking(move || {
                check_health_sync(&llama_url, HEALTH_TIMEOUT_SECS)
            })
            .await
            .unwrap_or(false);

            match llama_health.tick(llama_up) {
                HealthAction::Nothing => {
                    if !llama_up && llama_health.failures > 0 {
                        tracing::warn!(
                            "llama-server health check failed ({}/{})",
                            llama_health.failures, FAILURE_THRESHOLD
                        );
                    }
                }
                HealthAction::Restart => {
                    (self.on_event)(ServerEvent::Crashed { server: "llama" });
                    tracing::warn!("llama-server declared crashed — restarting");
                    {
                        let mut guard = self.llama_child.lock().await;
                        let _ = guard.take(); // KillOnDrop drop kills old process
                    } // guard released before any await
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    match ensure_llama_server().await {
                        Some(child) => {
                            *self.llama_child.lock().await = Some(KillOnDrop(child));
                            (self.on_event)(ServerEvent::Restarted { server: "llama" });
                            tracing::info!("llama-server restarted successfully");
                        }
                        None => {
                            // `ever_healthy` stays true, `failures` = 0 — next
                            // FAILURE_THRESHOLD consecutive fails will retry automatically.
                            (self.on_event)(ServerEvent::RestartFailed { server: "llama" });
                            tracing::error!(
                                "llama-server restart failed — will retry after {} more failures",
                                FAILURE_THRESHOLD
                            );
                        }
                    }
                }
            }

            // ── Classifier server (8081) ───────────────────────────────────
            if !manages_classifier {
                continue;
            }

            let classifier_url = config::classifier_base_url();
            let classifier_up = tokio::task::spawn_blocking(move || {
                check_health_sync(&classifier_url, HEALTH_TIMEOUT_SECS)
            })
            .await
            .unwrap_or(false);

            match classifier_health.tick(classifier_up) {
                HealthAction::Nothing => {
                    if !classifier_up && classifier_health.failures > 0 {
                        tracing::warn!(
                            "classifier health check failed ({}/{})",
                            classifier_health.failures, FAILURE_THRESHOLD
                        );
                    }
                }
                HealthAction::Restart => {
                    (self.on_event)(ServerEvent::Crashed { server: "classifier" });
                    tracing::warn!("classifier declared crashed — restarting");
                    {
                        let mut guard = self.classifier_child.lock().await;
                        let _ = guard.take();
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    match ensure_classifier_server().await {
                        Some(child) => {
                            *self.classifier_child.lock().await = Some(KillOnDrop(child));
                            (self.on_event)(ServerEvent::Restarted { server: "classifier" });
                            tracing::info!("Classifier server restarted successfully");
                        }
                        None => {
                            (self.on_event)(ServerEvent::RestartFailed { server: "classifier" });
                            tracing::error!(
                                "Classifier restart failed — will retry after {} more failures",
                                FAILURE_THRESHOLD
                            );
                        }
                    }
                }
            }
        }
    }
}

// ── Unit tests for the pure state machine ────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_manages_never_restarts() {
        let mut h = ServerHealth::new(false);
        for _ in 0..10 {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
    }

    #[test]
    fn grace_period_before_ever_healthy() {
        // Failures before first success are silently ignored.
        let mut h = ServerHealth::new(true);
        for _ in 0..10 {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(!h.ever_healthy);
        assert_eq!(h.failures, 0);
    }

    #[test]
    fn threshold_triggers_restart() {
        let mut h = ServerHealth::new(true);
        h.tick(true); // establish ever_healthy
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(matches!(h.tick(false), HealthAction::Restart));
    }

    #[test]
    fn healthy_check_resets_failure_count() {
        let mut h = ServerHealth::new(true);
        h.tick(true);
        h.tick(false);
        h.tick(false);
        h.tick(true); // recovery resets counter
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(matches!(h.tick(false), HealthAction::Restart));
    }

    #[test]
    fn retries_indefinitely_after_failed_restart() {
        // After a failed restart the child slot is None but ever_healthy=true,
        // failures=0. The guard does NOT update ServerHealth on restart outcome —
        // the state machine just sees the server is still down and retries after
        // another FAILURE_THRESHOLD consecutive failures.
        let mut h = ServerHealth::new(true);
        h.tick(true);

        // First crash
        for _ in 0..FAILURE_THRESHOLD { h.tick(false); }
        // failures was reset to 0 by the Restart return — simulates failed restart

        // Second attempt
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(matches!(h.tick(false), HealthAction::Restart));

        // Third attempt (confirms indefinite retry, not just one retry)
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(matches!(h.tick(false), HealthAction::Restart));
    }

    #[test]
    fn successful_restart_then_new_crash_cycle() {
        // After a successful restart we should eventually detect the next crash.
        let mut h = ServerHealth::new(true);
        h.tick(true); // initial healthy

        // First crash
        for _ in 0..FAILURE_THRESHOLD { h.tick(false); }

        // Successful restart — caller stores new child, guard sees server healthy again
        h.tick(true); // new server up → ever_healthy stays true, failures = 0

        // Second crash later
        for _ in 0..(FAILURE_THRESHOLD - 1) {
            assert!(matches!(h.tick(false), HealthAction::Nothing));
        }
        assert!(matches!(h.tick(false), HealthAction::Restart));
    }
}
