//! Post-action screen verification.
//!
//! Computes a SHA-256 hash of perceive.py output to detect whether a tool call
//! actually changed the GUI state. A "no-change" outcome triggers a single
//! in-loop retry; a second no-change marks the action as failed so that the
//! action graph and (later) the recovery module record the right outcome.

use sha2::{Digest, Sha256};
use std::time::Duration;

/// How long to wait after a tool call before re-reading the screen.
pub const SETTLE_MS: u64 = 500;

/// How many times an action may be retried inside one logical step
/// when the screen hasn't changed. (Spec: retry once, then mark failure.)
pub const MAX_NO_EFFECT_RETRIES: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Screen hash differs from pre-action — action took effect.
    Changed,
    /// Screen hash identical — action was a no-op.
    NoChange,
}

/// Stable 64-char hex digest of perceive.py output.
/// Acts as both the action-graph state key and the pre/post-verification hash.
pub fn screen_hash(screen: &str) -> String {
    let digest = Sha256::digest(screen.as_bytes());
    format!("{:x}", digest)
}

/// Short prefix for log readability (full hash still used as the key).
pub fn short(hash: &str) -> &str {
    &hash[..hash.len().min(8)]
}

pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(SETTLE_MS)).await;
}

pub fn classify(pre: &str, post: &str) -> VerifyOutcome {
    if pre == post {
        VerifyOutcome::NoChange
    } else {
        VerifyOutcome::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable() {
        let h1 = screen_hash("hello world");
        let h2 = screen_hash("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn hash_differs_for_different_input() {
        assert_ne!(screen_hash("a"), screen_hash("b"));
    }

    #[test]
    fn classify_detects_no_change() {
        assert_eq!(classify("x", "x"), VerifyOutcome::NoChange);
        assert_eq!(classify("x", "y"), VerifyOutcome::Changed);
    }

    #[test]
    fn short_handles_short_input() {
        // Shouldn't panic on a short hash
        let s = short("abc");
        assert_eq!(s, "abc");
    }
}
