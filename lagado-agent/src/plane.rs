//! Plane-governor — deterministic PICKER + SWITCHER over perception/actuation planes.
//!
//! The model NEVER chooses the plane; this is RAILS. The **CLI is the launch pad**: the most reliable
//! plane, it does most intra-OS work (file/system/launch) + the route-around-the-API **back-door**
//! (config-file / gsettings-dconf / D-Bus / sibling-CLI) + **discovery**, and is the vantage from which
//! the governor determines the next plane.
//!
//! AUTONOMY-FIRST: human handback is the ABSOLUTE LAST RESORT — only when the AI is 100% sure it cannot
//! proceed (every feasible plane tried AND re-discovery + re-pick yield nothing AND the supervisor confirms
//! no world progress). Exhausting the ladder under current findings is NOT a handback trigger; the agent
//! re-discovers and re-picks first.
//!
//! This module is the DECISION CORE (pure, unit-tested). Execution reuses the existing `Perceptor`/
//! `Actuator` planes and the `supervisor` switch engine; full wiring + the plane impls are the integration
//! step (see `docs/plans/LAGADO_PLANE_GOVERNOR_v1.md`). It JOINS logic previously scattered across
//! `agent.rs`, `supervisor.rs`, `perception/*`, and the OSWorld Python adapter.

use std::collections::HashSet;
use crate::supervisor::StepOutcome;

/// The perception/actuation planes. The in-app planes are ordered by VISIBILITY (richest first); the CLI
/// launch pad + its back-door are the system base (most reliable, least IN-APP visibility).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PlaneId {
    /// Launch pad: shell — file/system/launch + discovery. Most reliable; the home the governor works from.
    Cli,
    /// Route-around-the-API, reached FROM the CLI: config-file / dconf-gsettings / D-Bus / sibling-CLI.
    BackDoor,
    /// The app's programmatic surface (UNO/CDP/code-CLI). Richest in-app plane when present.
    Api,
    /// AT-SPI element tree.
    A11y,
    /// CV/OCR proposals.
    Cv,
    /// Raw pixel-delta / coordinate. Last resort.
    Pixel,
}

/// What KIND of work the goal is — drives which plane to PICK first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskKind {
    /// Create/move/delete files, run a program → the CLI launch pad does it directly.
    FileSystem,
    /// Change an app/desktop SETTING → the back-door (route around the app's GUI).
    AppSettings,
    /// The app must DO something to its document (compute/transform) → the app's API, else the GUI.
    InAppSemantic,
    /// Click/type a specific on-screen control → in-app planes by visibility (NOT the CLI).
    GuiInteraction,
    Unknown,
}

/// Unified discovery — what the CLI launch pad learned about the box. Feeds feasibility + pick.
/// (Built from `discover_environment` + a `command -v`/D-Bus probe + the focused-app read.)
#[derive(Clone, Debug, Default)]
pub struct Findings {
    /// `discover_environment` output (grounding + "is there a file surface to work on").
    pub fs_listing: String,
    /// The foreground app, if any (routing-correction + API/a11y feasibility).
    pub focused_app: Option<String>,
    /// Discovered: the focused app exposes a usable programmatic API (UNO/CDP/code-CLI).
    pub app_has_api: bool,
    /// A config back-door is reachable (dconf/gsettings present, or a known config file).
    pub has_config_backend: bool,
    /// A display/GUI surface exists (a11y/CV/pixel are feasible at all).
    pub gui_available: bool,
}

fn has_any(lo: &str, kws: &[&str]) -> bool {
    kws.iter().any(|k| lo.contains(k))
}

/// A goal scoped to the DESKTOP/system (not an in-app change) — so the back-door is the right route even
/// when an app happens to be focused. Keeps the routing-correction from misfiring on real desktop settings.
fn desktop_scoped(lo: &str) -> bool {
    has_any(lo, &["desktop", "system", "wallpaper", "background image", "screen resolution",
                  "global", "taskbar", "panel", "night light", "do not disturb"])
}

/// Raw goal-shape classification (pre routing-correction).
fn raw_classify(lo: &str) -> TaskKind {
    // GUI interaction first: an explicit on-screen control beats a settings keyword.
    if has_any(lo, &["click", "press the", "select the ", "check the box", "checkbox", "toolbar",
                     "drag ", "scroll ", "double-click", "right-click"]) {
        return TaskKind::GuiInteraction;
    }
    if has_any(lo, &["setting", "preference", "enable ", "disable ", "turn on", "turn off",
                     "configure", "dark mode", "default application", "shortcut key"]) {
        return TaskKind::AppSettings;
    }
    // Document/in-app semantic work (compute/transform the app's content).
    if has_any(lo, &["calculate", "compute", "sum ", "total", "formula", "gross profit", "chart",
                     "pivot", "convert the image", "palette", "merge cells", "fill in", "in a new sheet",
                     "rename the sheet", "export as"]) {
        return TaskKind::InAppSemantic;
    }
    if has_any(lo, &["create ", "make ", "delete", "remove ", "move ", "copy ", "rename ", "folder",
                     "directory", "run ", "launch ", "open the ", "install ", "git "]) {
        return TaskKind::FileSystem;
    }
    TaskKind::Unknown
}

/// Is the FOCUSED app a spreadsheet — the surface the API plane (UNO/openpyxl set_cell) addresses? The
/// signal is the APP'S IDENTITY (the App-Intents model: the focused app declares the work surface), NOT a
/// verb in the goal — so "fill all the blank cells" and "compute the gross profit" route the same way. The
/// set is the apps the API plane actually serves; it grows as planes are added. Safe to widen because the
/// api_plane branch falls THROUGH to the GUI plane when no API-addressable document is found.
fn in_spreadsheet_app(findings: &Findings) -> bool {
    match &findings.focused_app {
        Some(app) => {
            let lo = app.to_lowercase();
            has_any(&lo, &["calc", "gnumeric", "spreadsheet", "localc"])
        }
        None => false,
    }
}

/// Classify the goal into a `TaskKind`, with two ROUTING-CORRECTIONS that use GROUND-TRUTH state (the
/// focused app) over goal phrasing:
/// 1. APP-AWARE in-app-semantic: when a SPREADSHEET app is focused, content work on its document is
///    in-app-semantic regardless of how the goal is worded — the app, not a keyword, is the signal. File
///    ops (create/move a file) and DESKTOP-scoped settings are NOT in-app and keep their classification.
/// 2. settings-shaped goal + ANY app focused + not desktop-scoped = an IN-APP change (the back-door can't
///    config-edit its way to it). Ported from the Python adapter's `_is_desktop_config` + focused-app check.
pub fn classify_task(goal: &str, findings: &Findings) -> TaskKind {
    let lo = goal.to_lowercase();
    let base = raw_classify(&lo);
    // (1) A spreadsheet is focused → the work is on its document, unless it's plainly a file op or a
    // desktop-scoped setting. Phrasing-independent (fixes the keyword-fragile miss on "fill all the
    // blank cells"). GuiInteraction (an explicit on-screen control click) stays GUI.
    if in_spreadsheet_app(findings)
        && !matches!(base, TaskKind::FileSystem | TaskKind::GuiInteraction)
        && !desktop_scoped(&lo) {
        return TaskKind::InAppSemantic;
    }
    // (2) settings-shaped, any app focused, not desktop-scoped → in-app.
    if base == TaskKind::AppSettings && findings.focused_app.is_some() && !desktop_scoped(&lo) {
        return TaskKind::InAppSemantic;
    }
    base
}

/// The plane order to TRY for a task kind. In-app planes are by visibility (API → a11y → CV → pixel); the
/// CLI launch pad is the base for system work and the always-feasible fallback, but is deliberately ABSENT
/// from GUI-interaction (least in-app visibility — the in-app planes own that work).
pub fn preferred_order(kind: TaskKind) -> Vec<PlaneId> {
    use PlaneId::*;
    match kind {
        TaskKind::FileSystem => vec![Cli],
        TaskKind::AppSettings => vec![BackDoor, Api, A11y, Cv, Pixel],
        TaskKind::InAppSemantic => vec![Api, BackDoor, A11y, Cv, Pixel],
        TaskKind::GuiInteraction => vec![A11y, Cv, Pixel],
        TaskKind::Unknown => vec![Cli, BackDoor, Api, A11y, Cv, Pixel],
    }
}

/// The full IN-TASK stepback ladder, richest-first, ENDING at the CLI base:
/// **API → back-door → a11y → CV → pixel → CLI**.
/// The CLI is NOT excluded — it's the reliable launch pad, placed LAST for in-app *visibility* work (it's
/// blind to in-app elements) but kept available for what it's GOOD at (file/system/launch/discovery,
/// operate-on-file, the back-door route) as the final reliable resort before giving up to Human. Because the
/// stepback only descends on EXHAUSTION (not one no-effect), it reaches the CLI only after the richer planes
/// are spent — so the CLI is used for its strengths, never as a reflex that abandons the GUI on a stall.
pub const IN_APP_LADDER: [PlaneId; 6] =
    [PlaneId::Api, PlaneId::BackDoor, PlaneId::A11y, PlaneId::Cv, PlaneId::Pixel, PlaneId::Cli];

/// The next feasible IN-TASK plane below `current` on the ladder, or `None` if exhausted (⇒ the supervisor
/// escalates to Human). Called when the within-plane stepback is spent — NOT on a single no-effect. Spans
/// the whole set (API + back-door + a11y/CV/pixel + the CLI base), not just a11y/CV/pixel.
pub fn next_in_app(current: PlaneId, findings: &Findings) -> Option<PlaneId> {
    let i = IN_APP_LADDER.iter().position(|&p| p == current)?;
    IN_APP_LADDER[i + 1..].iter().copied().find(|&p| plane_applicable(p, findings))
}

/// Is a plane usable at all in the current environment (the FEASIBILITY gate the supervisor's blind
/// `escalate()` lacks)? The CLI launch pad is always available (the reliability floor).
pub fn plane_applicable(id: PlaneId, findings: &Findings) -> bool {
    match id {
        PlaneId::Cli => true,
        PlaneId::BackDoor => findings.has_config_backend,
        PlaneId::Api => findings.app_has_api,
        PlaneId::A11y | PlaneId::Cv | PlaneId::Pixel => findings.gui_available,
    }
}

/// PICK the starting plane for a goal: the most task-appropriate FEASIBLE plane. A hypothesis, not a lock —
/// `switch()` corrects it. Always re-evaluates feasibility against current findings, so a re-pick after the
/// world changes can return a cheaper plane (switch-BACK).
pub fn pick(goal: &str, findings: &Findings) -> PlaneId {
    let kind = classify_task(goal, findings);
    preferred_order(kind)
        .into_iter()
        .find(|&id| plane_applicable(id, findings))
        .unwrap_or(PlaneId::Cli)
}

/// SWITCH to the next feasible plane mid-task when the current one isn't working. Excludes the stalled
/// plane and any already tried this episode; RE-EVALUATES feasibility against current findings (not a blind
/// `tier_idx += 1`). `None` ⇒ all feasible planes exhausted ⇒ honestly infeasible (fail, don't loop).
pub fn switch(goal: &str, stalled: PlaneId, findings: &Findings, tried: &HashSet<PlaneId>) -> Option<PlaneId> {
    let kind = classify_task(goal, findings);
    preferred_order(kind)
        .into_iter()
        .find(|&id| id != stalled && !tried.contains(&id) && plane_applicable(id, findings))
}

/// Does THIS single step outcome mean "switch the PLANE now" (vs let the within-plane stepback keep
/// working)? ONLY `PerceptionBlind` — the structural "same model on the same read can't help, retrying is
/// pointless" signal. A single `NoChange`/`Failed` must NOT switch: the supervisor accumulates those up to
/// its stall/retry thresholds (and the loop's settle/re-perceive/re-plan runs) before it emits `Escalate`;
/// the governor re-aims on THAT escalation, not on one "nothing happened". One no-effect ≠ plane switch.
pub fn switch_on_outcome(outcome: StepOutcome) -> bool {
    matches!(outcome, StepOutcome::PerceptionBlind)
}

/// Stateful loop-facing governor: a THIN decision over the stepback machinery the loop ALREADY has. The
/// loop keeps its own locus (current sub-goal) and its own "nothing happened at the locus → step back"
/// fallback; the governor only answers **which plane the same stepback re-aims at next**. `start()` picks
/// the entry plane; `next()` is called when the current plane's stepback exhausts (no effect at the locus /
/// supervisor escalate) and returns the next FEASIBLE plane, or `None` ⇒ all feasible planes exhausted ⇒
/// the loop's existing honest handback. It does NOT re-derive targets and does NOT rebuild the stepback.
pub struct PlaneGovernor {
    current: PlaneId,
    tried: HashSet<PlaneId>,
}

impl PlaneGovernor {
    /// Pick the entry plane for the goal (a hypothesis; `next()` corrects it).
    pub fn start(goal: &str, findings: &Findings) -> Self {
        let current = pick(goal, findings);
        let tried = [current].into_iter().collect();
        Self { current, tried }
    }

    pub fn current(&self) -> PlaneId {
        self.current
    }

    /// Re-aim the stepback at the next feasible plane (records the exhausted one). Feasibility is
    /// re-evaluated against current findings, so a re-pick can land on a cheaper plane if the world changed.
    ///
    /// `None` ⇒ all feasible planes tried UNDER CURRENT FINDINGS. This is NOT a human-handback trigger by
    /// itself — the caller must RE-DISCOVER and `repick()` first (autonomy-first). Human is the ABSOLUTE
    /// LAST RESORT (see `repick`).
    ///
    /// CALL TIMING (load-bearing): only after the WITHIN-PLANE stepback has EXHAUSTED — i.e. the
    /// supervisor's `Escalate` (accumulated stalls past its threshold) or `PerceptionBlind`. NEVER on a
    /// single "nothing happened": one no-effect is normal (slow paint / settle / a retry-able miss), and the
    /// loop's own settle → re-perceive → re-plan-a-different-op absorbs it first. Jumping planes on one
    /// no-effect would thrash.
    pub fn next(&mut self, goal: &str, findings: &Findings) -> Option<PlaneId> {
        let n = switch(goal, self.current, findings, &self.tried)?;
        self.tried.insert(n);
        self.current = n;
        Some(n)
    }

    /// Re-pick from scratch after RE-DISCOVERY (fresh findings): clear the tried set and pick anew. The
    /// autonomy-first retry — when the ladder is exhausted under stale findings, the world may have changed
    /// (a dialog closed, an app launched, the API came up), so a fresh pass can find a now-feasible (even
    /// cheaper) plane. HUMAN HANDBACK IS THE ABSOLUTE LAST RESORT: only after re-discovery + re-pick ALSO
    /// yield no usable plane AND the supervisor confirms no world progress — i.e. 100% sure the AI cannot
    /// proceed. Exhaust autonomy before ever asking the human.
    pub fn repick(&mut self, goal: &str, findings: &Findings) -> PlaneId {
        let current = pick(goal, findings);
        self.tried = [current].into_iter().collect();
        self.current = current;
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui() -> Findings { Findings { gui_available: true, ..Default::default() } }

    #[test]
    fn classify_covers_the_kinds() {
        let f = Findings::default();
        assert_eq!(classify_task("create a folder /tmp/x", &f), TaskKind::FileSystem);
        assert_eq!(classify_task("enable dark mode", &f), TaskKind::AppSettings);
        assert_eq!(classify_task("calculate the total sales in a new row", &f), TaskKind::InAppSemantic);
        assert_eq!(classify_task("click the Save button", &f), TaskKind::GuiInteraction);
    }

    #[test]
    fn routing_correction_settings_to_in_app_when_app_focused() {
        // a settings-shaped goal, a real app focused, NOT desktop-scoped → in-app (back-door can't reach it)
        let f = Findings { focused_app: Some("gimp".into()), ..Default::default() };
        assert_eq!(classify_task("configure the default unit", &f), TaskKind::InAppSemantic);
        // but a DESKTOP-scoped settings goal stays AppSettings even with an app focused
        assert_eq!(classify_task("configure the desktop wallpaper", &f), TaskKind::AppSettings);
        // and with no app focused it stays AppSettings
        assert_eq!(classify_task("configure the default unit", &Findings::default()), TaskKind::AppSettings);
    }

    #[test]
    fn app_aware_routes_content_work_to_in_app_regardless_of_phrasing() {
        // a spreadsheet is focused → content work is in-app-semantic even when NO verb keyword matches
        // (the keyword-fragile miss that sent "fill all the blank cells" to Unknown → wrong plane).
        let calc = Findings { focused_app: Some("Untitled 1 — LibreOffice Calc".into()), ..Default::default() };
        assert_eq!(classify_task("fill all the blank cells with 0", &calc), TaskKind::InAppSemantic);
        assert_eq!(classify_task("highlight the duplicate rows", &calc), TaskKind::InAppSemantic);
        // but a FILE op stays a file op even with calc focused (create a sibling file, not edit content)
        assert_eq!(classify_task("make a backup copy of the file", &calc), TaskKind::FileSystem);
        // and an explicit on-screen control click stays GUI (the API plane can't express it)
        assert_eq!(classify_task("click the Save button", &calc), TaskKind::GuiInteraction);
        // a NON-spreadsheet app focused does NOT get the upgrade (gimp keeps GUI/Unknown → GUI loop)
        let gimp = Findings { focused_app: Some("GNU Image Manipulation Program".into()), ..Default::default() };
        assert_eq!(classify_task("fill all the blank cells with 0", &gimp), TaskKind::Unknown);
    }

    #[test]
    fn pick_is_task_appropriate_and_feasible() {
        // file work → the launch pad
        assert_eq!(pick("create a folder /tmp/x", &Findings::default()), PlaneId::Cli);
        // settings with a config backend → the back-door (route around the app)
        let cfg = Findings { has_config_backend: true, ..Default::default() };
        assert_eq!(pick("enable dark mode", &cfg), PlaneId::BackDoor);
        // settings with NO backend but a GUI → falls to a11y (still never the CLI for in-app)
        assert_eq!(pick("enable dark mode", &gui()), PlaneId::A11y);
        // in-app semantic with an API → the API plane
        let api = Findings { app_has_api: true, gui_available: true, ..Default::default() };
        assert_eq!(pick("calculate the total sales", &api), PlaneId::Api);
        // in-app semantic, no API, GUI present → a11y
        assert_eq!(pick("calculate the total sales", &gui()), PlaneId::A11y);
        // GUI interaction → a11y (by visibility)
        assert_eq!(pick("click the Save button", &gui()), PlaneId::A11y);
        // GUI interaction with NO gui surface → the launch pad is the always-feasible fallback
        assert_eq!(pick("click the Save button", &Findings::default()), PlaneId::Cli);
    }

    #[test]
    fn switch_walks_the_feasible_order_then_exhausts() {
        let f = gui();
        let mut tried = HashSet::new();
        tried.insert(PlaneId::A11y);
        // a11y stalled → CV
        assert_eq!(switch("click the Save button", PlaneId::A11y, &f, &tried), Some(PlaneId::Cv));
        tried.insert(PlaneId::Cv);
        // CV stalled → pixel
        assert_eq!(switch("click the Save button", PlaneId::Cv, &f, &tried), Some(PlaneId::Pixel));
        tried.insert(PlaneId::Pixel);
        // all in-app planes exhausted → honestly infeasible (no CLI for a GUI-interaction task)
        assert_eq!(switch("click the Save button", PlaneId::Pixel, &f, &tried), None);
    }

    #[test]
    fn switch_re_evaluates_feasibility() {
        // in-app semantic, API stalled, no API/GUI feasible now → infeasible (no blind descent into dead planes)
        let none = Findings::default();
        let tried: HashSet<PlaneId> = [PlaneId::Api].into_iter().collect();
        assert_eq!(switch("calculate the total sales", PlaneId::Api, &none, &tried), None);
        // but if a GUI is available, the same switch lands on a11y
        assert_eq!(switch("calculate the total sales", PlaneId::Api, &gui(), &tried), Some(PlaneId::A11y));
    }

    #[test]
    fn perception_blind_triggers_switch() {
        assert!(switch_on_outcome(StepOutcome::PerceptionBlind));
        assert!(!switch_on_outcome(StepOutcome::NoChange));
        assert!(!switch_on_outcome(StepOutcome::Progressed));
    }

    #[test]
    fn stateful_governor_re_aims_then_exhausts() {
        // GUI-interaction goal, full GUI surface: start on a11y, step back through CV → pixel → exhausted.
        let f = gui();
        let mut gov = PlaneGovernor::start("click the Save button", &f);
        assert_eq!(gov.current(), PlaneId::A11y);
        assert_eq!(gov.next("click the Save button", &f), Some(PlaneId::Cv));
        assert_eq!(gov.next("click the Save button", &f), Some(PlaneId::Pixel));
        assert_eq!(gov.next("click the Save button", &f), None); // all feasible planes tried → handback
    }

    #[test]
    fn in_app_ladder_spans_api_backdoor_a11y_cv_pixel_then_cli() {
        let f = gui();
        // step DOWN the visibility ladder, then to the CLI base (NOT excluded), then truly exhausted
        assert_eq!(next_in_app(PlaneId::A11y, &f), Some(PlaneId::Cv));
        assert_eq!(next_in_app(PlaneId::Cv, &f), Some(PlaneId::Pixel));
        assert_eq!(next_in_app(PlaneId::Pixel, &f), Some(PlaneId::Cli)); // CLI base = last reliable resort
        assert_eq!(next_in_app(PlaneId::Cli, &f), None);                 // only NOW is the ladder spent
        // the ladder INCLUDES API (top) + back-door, not just a11y/cv/pixel:
        let api_cfg = Findings { app_has_api: true, has_config_backend: true, gui_available: true, ..Default::default() };
        assert_eq!(next_in_app(PlaneId::Api, &api_cfg), Some(PlaneId::BackDoor));
        let api_gui = Findings { app_has_api: true, gui_available: true, ..Default::default() };
        assert_eq!(next_in_app(PlaneId::Api, &api_gui), Some(PlaneId::A11y));
        assert_eq!(next_in_app(PlaneId::BackDoor, &f), Some(PlaneId::A11y));
        // no GUI surface → the in-app planes are infeasible, but the CLI base is still there (not excluded)
        assert_eq!(next_in_app(PlaneId::A11y, &Findings::default()), Some(PlaneId::Cli));
    }

    #[test]
    fn repick_after_rediscovery_finds_newly_feasible_plane() {
        // ladder exhausted with no GUI → next() is None (NOT a human handback)
        let blind = Findings::default();
        let mut gov = PlaneGovernor::start("click the Save button", &blind);
        // (start picks Cli as the always-feasible fallback when no GUI)
        // world changes: a GUI surface appears → re-discover + repick lands on a11y, autonomy continues
        let now_gui = gui();
        assert_eq!(gov.repick("click the Save button", &now_gui), PlaneId::A11y);
    }

    #[test]
    fn stateful_governor_file_goal_has_no_switch() {
        // A file goal lives on the CLI launch pad; there's nothing to re-aim to → straight to handback.
        let f = Findings::default();
        let mut gov = PlaneGovernor::start("create a folder /tmp/x", &f);
        assert_eq!(gov.current(), PlaneId::Cli);
        assert_eq!(gov.next("create a folder /tmp/x", &f), None);
    }
}

// ── SENSE-MARKET DISPATCH (2026-07-08 doctrine: ladder → market) ─────────────────
//
// The ladder above switches planes on FAILURE. The dispatch table below picks the
// ACTOR per sub-goal from MEASURED fitness facts BEFORE failure — deterministic
// rules, never a learned router (cortex/subcortex doctrine). v1 is wired
// MEASURE-FIRST: the agent loop logs the table's verdict beside the world-model
// fact at every perceive; control flips to the table only after the logged
// verdicts prove it against the existing routing.

/// Measured inputs to the dispatch decision. Every field is a FACT with a source,
/// never a guess: world model (staleness/coverage), selection (label_match),
/// sequencer (command_surface), plane registry (scriptable_app), eyes (target_moving).
#[derive(Debug, Clone, Copy)]
pub struct DispatchFacts {
    /// goal expressible as commands with a checkable postcondition (sequencer class)
    pub command_surface: bool,
    /// an in-app API plane owns this app's semantics (e.g. UNO for spreadsheets)
    pub scriptable_app: bool,
    /// world model: last a11y read damaged since (valid-until-damaged)
    pub a11y_stale: bool,
    /// world model: a11y elements / CV boxes on the same frame
    pub coverage: f32,
    /// selection: some candidate label shares a content token with the goal
    pub label_match: bool,
    /// eyes: ongoing translation/change in the target region (future; false today)
    pub target_moving: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// the brain drives commands directly (Qwen's proven ground: 11/11 stress)
    CliDirect,
    /// in-app semantic ops through the app's API (native session / UNO)
    ApiPlane,
    /// the a11y selection loop (labels match, surface is rich enough)
    A11ySelection,
    /// the visual path: eyes locate, hands drive (a11y thin/stale or target moving)
    EyesHands,
    /// no actor fits yet — refresh perception first
    Reperceive,
}

/// The table. Rules in priority order; returns the actor AND the rule name so every
/// dispatch is a chronos-auditable fact ("which rule fired, on what measurements").
pub fn dispatch_actor(f: &DispatchFacts) -> (Actor, &'static str) {
    if f.scriptable_app {
        return (Actor::ApiPlane, "api-plane: scriptable app owns in-app semantics");
    }
    if f.command_surface {
        return (Actor::CliDirect, "cli-first: command surface + checkable postcondition");
    }
    if f.target_moving {
        return (Actor::EyesHands, "visual: target moving (measured hands regime)");
    }
    if f.label_match && !f.a11y_stale {
        return (Actor::A11ySelection, "a11y: fresh read + label match");
    }
    if f.label_match {
        return (Actor::A11ySelection, "a11y: label match (stale read — re-read first)");
    }
    if f.coverage < 0.5 {
        return (Actor::EyesHands, "visual: a11y thin vs CV (coverage < 0.5)");
    }
    (Actor::Reperceive, "no actor fits — refresh perception")
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    fn base() -> DispatchFacts {
        DispatchFacts {
            command_surface: false,
            scriptable_app: false,
            a11y_stale: false,
            coverage: 1.0,
            label_match: false,
            target_moving: false,
        }
    }

    #[test]
    fn priority_order_is_api_cli_moving_a11y_visual() {
        let mut f = base();
        f.scriptable_app = true;
        f.command_surface = true;
        assert_eq!(dispatch_actor(&f).0, Actor::ApiPlane, "api outranks cli");
        f.scriptable_app = false;
        assert_eq!(dispatch_actor(&f).0, Actor::CliDirect, "cli outranks gui paths");
        f.command_surface = false;
        f.target_moving = true;
        f.label_match = true;
        assert_eq!(dispatch_actor(&f).0, Actor::EyesHands, "moving target outranks a11y");
    }

    #[test]
    fn a11y_when_labels_match_visual_when_thin() {
        let mut f = base();
        f.label_match = true;
        assert_eq!(dispatch_actor(&f).0, Actor::A11ySelection);
        f.label_match = false;
        f.coverage = 0.2;
        assert_eq!(dispatch_actor(&f).0, Actor::EyesHands, "CV sees what a11y doesn't");
        f.coverage = 1.5;
        assert_eq!(dispatch_actor(&f).0, Actor::Reperceive, "rich surface, no match → re-look");
    }

    #[test]
    fn every_rule_names_itself() {
        // the audit contract: no dispatch without a stated rule
        let (_, r) = dispatch_actor(&base());
        assert!(!r.is_empty());
    }
}
