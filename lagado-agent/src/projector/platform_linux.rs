//! platform_linux.rs — Linux input dispatch via xdotool.

use super::executor::{ActionResult, Executor, InputAction};

pub struct LinuxExecutor;

impl LinuxExecutor {
    pub fn new() -> Self { Self }
}

impl Executor for LinuxExecutor {
    fn execute(&self, action: InputAction) -> ActionResult {
        match action {
            InputAction::MouseClick { x, y, button } => {
                let status = std::process::Command::new("xdotool")
                    .args([
                        "mousemove", "--sync",
                        &x.to_string(), &y.to_string(),
                        "click", &button.to_string(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => ActionResult::ok(format!("clicked ({x},{y}) btn={button}")),
                    Ok(_) => ActionResult::err("xdotool click failed — native Wayland unsupported"),
                    Err(e) => ActionResult::err(format!("xdotool spawn failed: {e}")),
                }
            }
            InputAction::TypeText { text } => {
                let status = std::process::Command::new("xdotool")
                    .args(["type", "--clearmodifiers", "--", &text])
                    .status();
                match status {
                    Ok(s) if s.success() => ActionResult::ok(format!("typed {} chars", text.chars().count())),
                    Ok(_) => ActionResult::err("xdotool type failed"),
                    Err(e) => ActionResult::err(format!("xdotool spawn failed: {e}")),
                }
            }
            InputAction::KeyPress { key } => {
                let status = std::process::Command::new("xdotool")
                    .args(["key", "--clearmodifiers", &key])
                    .status();
                match status {
                    Ok(s) if s.success() => ActionResult::ok(format!("pressed {key}")),
                    Ok(_) => ActionResult::err("xdotool key failed"),
                    Err(e) => ActionResult::err(format!("xdotool spawn failed: {e}")),
                }
            }
            InputAction::MouseMove { x, y } => {
                let status = std::process::Command::new("xdotool")
                    .args(["mousemove", "--sync", &x.to_string(), &y.to_string()])
                    .status();
                match status {
                    Ok(s) if s.success() => ActionResult::ok(format!("moved to ({x},{y})")),
                    Ok(_) => ActionResult::err("xdotool mousemove failed"),
                    Err(e) => ActionResult::err(format!("xdotool spawn failed: {e}")),
                }
            }
        }
    }
}
