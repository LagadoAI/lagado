//! supervisor.rs — the strategic OUTER control plane (doctrine C4).
//!
//! RAILS, NOT STRATEGY. The small model decides "what's next" single-turn-fresh from the
//! corrected Board each step; the supervisor decides nothing about the task itself. It
//! only watches for stalls / loops / failures and runs the bounded-retry ESCALATION
//! LADDER. Without the ladder, reset-from-corrected-board loops forever (diagnosis is
//! itself fallible). Detection is deterministic ON PURPOSE — you do not want a flaky
//! model deciding when to escalate.
//!
//! THE LADDER IS GOVERNOR-OWNED, NOT HARDCODED. The supervisor does not know whether an
//! 8B exists, whether cloud is allowed, or what order to try things — that depends on the
//! hardware probe + the local/hybrid/cloud mode + user settings, all of which live in the
//! governor. The governor builds an ordered `Vec<EscalationTier>`; the supervisor just
//! walks it. A weak local-only box might get `[local, human]`; a hybrid box
//! `[local-1.2b, local-8b, cloud, human]`. The loop resolves each tier's label/kind to an
//! actual adapter (or a human handoff).
//!
//! Pure control-plane state machine: the loop reports a `StepOutcome` + the current state
//! hash and obeys the returned `Directive`. No models, no I/O, no VM — fully unit-tested.
//! Complements `recovery.rs` (inner tactical recovery); this is the outer rung authority.

use std::collections::VecDeque;

/// What the just-executed step produced, as the loop sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Screen/state advanced — real progress.
    Progressed,
    /// The action ran but state didn't move (stuck in place).
    NoChange,
    /// Step errored / unparseable output / rejected action.
    Failed,
    /// Goal reached (Done/Task tool call).
    Done,
}

/// What a ladder rung does. The supervisor stays model-agnostic; the loop interprets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierKind {
    /// An inference tier — some model, somewhere. The loop maps `label` → adapter.
    Model,
    /// Hand control to the human (typically the terminal fallback before Abort).
    Human,
}

/// One rung of the escalation ladder, supplied by the GOVERNOR (hardware + mode + user
/// settings). The supervisor owns the escalation LOGIC; the governor owns WHICH tiers
/// exist and in what order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationTier {
    /// Opaque to the supervisor; the loop resolves it (e.g. "local-1.2b", "heavy-8b",
    /// "cloud:claude", "hitl").
    pub label: String,
    pub kind: TierKind,
}

impl EscalationTier {
    pub fn model(label: impl Into<String>) -> Self {
        Self { label: label.into(), kind: TierKind::Model }
    }
    pub fn human() -> Self {
        Self { label: "hitl".to_string(), kind: TierKind::Human }
    }
}

/// What the supervisor tells the loop to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    /// Proceed to the next step at the current tier.
    Continue,
    /// Single-turn reset: re-present a corrected slice from the Board, same tier.
    ResetFromBoard,
    /// Switch to this governor-defined tier (the loop reads `kind` to act).
    Escalate(EscalationTier),
    /// Goal achieved.
    Done,
    /// Ladder exhausted / give up, with a reason.
    Abort(String),
}

const DEFAULT_MAX_LOCAL_RETRIES: u32 = 2;
const DEFAULT_MAX_STALL: u32 = 3;
const DEFAULT_LOOP_THRESHOLD: usize = 2; // prior revisits before "oscillating"
const DEFAULT_WINDOW: usize = 8;

/// Bounded-retry escalation over a governor-supplied ladder, + stall/loop detection.
pub struct Supervisor {
    ladder: Vec<EscalationTier>,
    tier_idx: usize,
    local_retries: u32,
    max_local_retries: u32,
    stall_count: u32,
    max_stall: u32,
    recent: VecDeque<u64>,
    window: usize,
    loop_threshold: usize,
}

impl Supervisor {
    /// `ladder` is built by the governor from hardware + mode + user settings. It should
    /// be non-empty (tier 0 is where the loop starts) and typically ends with a Human tier.
    pub fn new(ladder: Vec<EscalationTier>) -> Self {
        Self::with_limits(
            ladder,
            DEFAULT_MAX_LOCAL_RETRIES,
            DEFAULT_MAX_STALL,
            DEFAULT_LOOP_THRESHOLD,
            DEFAULT_WINDOW,
        )
    }

    pub fn with_limits(
        ladder: Vec<EscalationTier>,
        max_local_retries: u32,
        max_stall: u32,
        loop_threshold: usize,
        window: usize,
    ) -> Self {
        Supervisor {
            ladder,
            tier_idx: 0,
            local_retries: 0,
            max_local_retries,
            stall_count: 0,
            max_stall,
            recent: VecDeque::with_capacity(window),
            window,
            loop_threshold,
        }
    }

    /// The tier the loop should currently be running on (None if the ladder is empty).
    pub fn current_tier(&self) -> Option<&EscalationTier> {
        self.ladder.get(self.tier_idx)
    }

    /// Report a step's outcome + the resulting state hash; get the next directive.
    pub fn observe(&mut self, outcome: StepOutcome, state_hash: u64) -> Directive {
        if outcome == StepOutcome::Done {
            return Directive::Done;
        }

        // Oscillation: have we been at this exact state too many times in the window?
        // No local retry breaks a cycle, so escalate regardless of the outcome kind.
        let revisits = self.recent.iter().filter(|&&h| h == state_hash).count();
        self.record(state_hash);
        if revisits >= self.loop_threshold {
            return self.escalate();
        }

        match outcome {
            StepOutcome::Progressed => {
                self.local_retries = 0;
                self.stall_count = 0;
                Directive::Continue
            }
            StepOutcome::NoChange => {
                self.stall_count += 1;
                if self.stall_count >= self.max_stall {
                    self.escalate()
                } else {
                    Directive::ResetFromBoard
                }
            }
            StepOutcome::Failed => {
                self.local_retries += 1;
                if self.local_retries >= self.max_local_retries {
                    self.escalate()
                } else {
                    Directive::ResetFromBoard
                }
            }
            StepOutcome::Done => unreachable!("handled above"),
        }
    }

    fn record(&mut self, h: u64) {
        if self.recent.len() == self.window {
            self.recent.pop_front();
        }
        self.recent.push_back(h);
    }

    /// Advance to the next governor-defined tier, resetting local counters and the loop
    /// window (a fresh tier gets a clean slate). Past the end of the ladder → Abort.
    fn escalate(&mut self) -> Directive {
        self.local_retries = 0;
        self.stall_count = 0;
        self.recent.clear();
        self.tier_idx += 1;
        match self.ladder.get(self.tier_idx) {
            Some(tier) => Directive::Escalate(tier.clone()),
            None => Directive::Abort("escalation ladder exhausted".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative hybrid-box ladder (what the governor might build).
    fn hybrid_ladder() -> Vec<EscalationTier> {
        vec![
            EscalationTier::model("local-1.2b"),
            EscalationTier::model("heavy-8b"),
            EscalationTier::model("cloud"),
            EscalationTier::human(),
        ]
    }

    #[test]
    fn done_returns_done() {
        let mut s = Supervisor::new(hybrid_ladder());
        assert_eq!(s.observe(StepOutcome::Done, 1), Directive::Done);
    }

    #[test]
    fn starts_on_tier_zero() {
        let s = Supervisor::new(hybrid_ladder());
        assert_eq!(s.current_tier().unwrap().label, "local-1.2b");
    }

    #[test]
    fn progress_continues_and_resets_counters() {
        let mut s = Supervisor::with_limits(hybrid_ladder(), 2, 3, 2, 8);
        assert_eq!(s.observe(StepOutcome::Failed, 1), Directive::ResetFromBoard);
        assert_eq!(s.observe(StepOutcome::Progressed, 2), Directive::Continue);
        assert_eq!(s.observe(StepOutcome::Failed, 3), Directive::ResetFromBoard); // fresh, not escalated
    }

    #[test]
    fn repeated_failures_reset_then_escalate_to_governor_tier() {
        let mut s = Supervisor::with_limits(hybrid_ladder(), 2, 9, 9, 16);
        assert_eq!(s.observe(StepOutcome::Failed, 1), Directive::ResetFromBoard);
        // escalation goes to whatever the GOVERNOR put at index 1 — not a hardcoded "Heavy"
        assert_eq!(
            s.observe(StepOutcome::Failed, 2),
            Directive::Escalate(EscalationTier::model("heavy-8b"))
        );
        assert_eq!(s.current_tier().unwrap().label, "heavy-8b");
    }

    #[test]
    fn stall_escalates_at_threshold() {
        let mut s = Supervisor::with_limits(hybrid_ladder(), 9, 3, 9, 16);
        assert_eq!(s.observe(StepOutcome::NoChange, 1), Directive::ResetFromBoard);
        assert_eq!(s.observe(StepOutcome::NoChange, 2), Directive::ResetFromBoard);
        assert_eq!(
            s.observe(StepOutcome::NoChange, 3),
            Directive::Escalate(EscalationTier::model("heavy-8b"))
        );
    }

    #[test]
    fn oscillation_escalates() {
        let mut s = Supervisor::with_limits(hybrid_ladder(), 9, 9, 2, 8);
        assert_eq!(s.observe(StepOutcome::Progressed, 0xA), Directive::Continue);
        assert_eq!(s.observe(StepOutcome::Progressed, 0xB), Directive::Continue);
        assert_eq!(s.observe(StepOutcome::Progressed, 0xA), Directive::Continue);
        assert_eq!(s.observe(StepOutcome::Progressed, 0xB), Directive::Continue);
        match s.observe(StepOutcome::Progressed, 0xA) {
            Directive::Escalate(t) => assert_eq!(t.label, "heavy-8b"),
            other => panic!("expected escalate, got {other:?}"),
        }
    }

    #[test]
    fn walks_the_full_governor_ladder_then_aborts() {
        let mut s = Supervisor::with_limits(hybrid_ladder(), 1, 9, 9, 16);
        assert_eq!(s.observe(StepOutcome::Failed, 1), Directive::Escalate(EscalationTier::model("heavy-8b")));
        assert_eq!(s.observe(StepOutcome::Failed, 2), Directive::Escalate(EscalationTier::model("cloud")));
        assert_eq!(s.observe(StepOutcome::Failed, 3), Directive::Escalate(EscalationTier::human()));
        match s.observe(StepOutcome::Failed, 4) {
            Directive::Abort(_) => {}
            other => panic!("expected Abort, got {other:?}"),
        }
    }

    #[test]
    fn weak_box_ladder_goes_straight_to_human() {
        // Governor on a weak local-only box: no 8B, no cloud — just [local, human].
        let ladder = vec![EscalationTier::model("local-1.2b"), EscalationTier::human()];
        let mut s = Supervisor::with_limits(ladder, 1, 9, 9, 16);
        assert_eq!(s.observe(StepOutcome::Failed, 1), Directive::Escalate(EscalationTier::human()));
        match s.observe(StepOutcome::Failed, 2) {
            Directive::Abort(_) => {}
            other => panic!("expected Abort, got {other:?}"),
        }
    }
}
