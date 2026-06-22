//! Plane-governor — deterministic PICKER + SWITCHER over perception/actuation planes.
//!
//! The model NEVER chooses the plane; this is RAILS. The **CLI is the launch pad**: the most reliable
//! plane, it does most intra-OS work (file/system/launch) + the route-around-the-API **back-door**
//! (config-file / gsettings-dconf / D-Bus / sibling-CLI) + **discovery**, and is the vantage from which
//! the governor determines the next plane.
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

/// Classify the goal into a `TaskKind`, with the ROUTING-CORRECTION (ported from the Python adapter's
/// `_is_desktop_config` + focused-app check): a settings-shaped goal, while a real app is focused, that is
/// NOT desktop-scoped, is really an IN-APP change — the back-door can't config-edit its way to it.
pub fn classify_task(goal: &str, findings: &Findings) -> TaskKind {
    let lo = goal.to_lowercase();
    let base = raw_classify(&lo);
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

/// Does this step outcome mean "switch the PLANE" (vs retry on the same plane)? `PerceptionBlind` is the
/// canonical "same model on the same read can't help — switch the plane, don't retry" signal. The
/// supervisor aggregates stall/loop into its own `Escalate(Sense)` directive, which the caller also routes
/// to `switch()`.
pub fn switch_on_outcome(outcome: StepOutcome) -> bool {
    matches!(outcome, StepOutcome::PerceptionBlind)
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
}
