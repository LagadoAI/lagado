//! platform_linux.rs — Linux input dispatch via xdotool.

use super::executor::{ActionResult, Executor, InputAction};

pub struct LinuxExecutor;

impl LinuxExecutor {
    pub fn new() -> Self { Self }
}

/// Build the xdotool argument list for an action. PURE (no process spawn) so the full
/// raw motor surface is unit-testable in CI. Wheel: buttons 4=up 5=down 6=left 7=right;
/// +dy = down, +dx = right. Drags interleave `sleep` + a midpoint move — toolkits drop
/// a press+teleport+release chain with no motion events between.
pub(crate) fn xdotool_args(action: &InputAction) -> Result<Vec<String>, String> {
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match action {
        InputAction::MouseClick { x, y, button, count } => {
            let c = (*count).max(1);
            Ok(s(&["mousemove", "--sync", &x.to_string(), &y.to_string(),
                   "click", "--repeat", &c.to_string(), "--delay", "120", &button.to_string()]))
        }
        InputAction::TypeText { text } => Ok(s(&["type", "--clearmodifiers", "--", text])),
        InputAction::KeyPress { key } => Ok(s(&["key", "--clearmodifiers", key])),
        InputAction::MouseMove { x, y } =>
            Ok(s(&["mousemove", "--sync", &x.to_string(), &y.to_string()])),
        InputAction::MouseScroll { dx, dy } => {
            if *dx == 0 && *dy == 0 {
                return Err("scroll with zero delta".to_string());
            }
            let mut args: Vec<String> = Vec::new();
            if *dy != 0 {
                let btn = if *dy > 0 { "5" } else { "4" };
                args.extend(s(&["click", "--repeat", &dy.abs().to_string(), "--delay", "60", btn]));
            }
            if *dx != 0 {
                let btn = if *dx > 0 { "7" } else { "6" };
                args.extend(s(&["click", "--repeat", &dx.abs().to_string(), "--delay", "60", btn]));
            }
            Ok(args)
        }
        InputAction::MouseDrag { x1, y1, x2, y2, button } => {
            let (mx, my) = ((x1 + x2) / 2, (y1 + y2) / 2);
            let b = button.to_string();
            Ok(s(&["mousemove", "--sync", &x1.to_string(), &y1.to_string(),
                   "mousedown", &b, "sleep", "0.2",
                   "mousemove", "--sync", &mx.to_string(), &my.to_string(), "sleep", "0.1",
                   "mousemove", "--sync", &x2.to_string(), &y2.to_string(), "sleep", "0.2",
                   "mouseup", &b]))
        }
    }
}

fn detail_for(action: &InputAction) -> String {
    match action {
        InputAction::MouseClick { x, y, button, count } =>
            format!("clicked ({x},{y}) btn={button} x{}", (*count).max(1)),
        InputAction::TypeText { text } => format!("typed {} chars", text.chars().count()),
        InputAction::KeyPress { key } => format!("pressed {key}"),
        InputAction::MouseMove { x, y } => format!("moved to ({x},{y})"),
        InputAction::MouseScroll { dx, dy } => format!("scrolled dx={dx} dy={dy}"),
        InputAction::MouseDrag { x1, y1, x2, y2, button } =>
            format!("dragged ({x1},{y1})→({x2},{y2}) btn={button}"),
    }
}

impl Executor for LinuxExecutor {
    fn execute(&self, action: InputAction) -> ActionResult {
        let args = match xdotool_args(&action) {
            Ok(a) => a,
            Err(e) => return ActionResult::err(e),
        };
        let status = std::process::Command::new("xdotool").args(&args).status();
        match status {
            Ok(st) if st.success() => ActionResult::ok(detail_for(&action)),
            Ok(_) => ActionResult::err(format!("xdotool failed: {}", detail_for(&action))),
            Err(e) => ActionResult::err(format!("xdotool spawn failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_carries_repeat_and_button() {
        let a = xdotool_args(&InputAction::MouseClick { x: 10, y: 20, button: 3, count: 2 }).unwrap();
        assert_eq!(a.join(" "), "mousemove --sync 10 20 click --repeat 2 --delay 120 3");
    }

    #[test]
    fn scroll_signs_map_to_wheel_buttons() {
        let a = xdotool_args(&InputAction::MouseScroll { dx: -2, dy: 3 }).unwrap();
        assert_eq!(a.join(" "), "click --repeat 3 --delay 60 5 click --repeat 2 --delay 60 6");
        assert!(xdotool_args(&InputAction::MouseScroll { dx: 0, dy: 0 }).is_err());
    }

    #[test]
    fn drag_orders_press_motion_release() {
        let a = xdotool_args(&InputAction::MouseDrag { x1: 0, y1: 0, x2: 8, y2: 4, button: 1 })
            .unwrap().join(" ");
        let down = a.find("mousedown 1").unwrap();
        let mid = a.find("mousemove --sync 4 2").unwrap();
        let up = a.find("mouseup 1").unwrap();
        assert!(down < mid && mid < up, "order wrong: {a}");
    }
}
