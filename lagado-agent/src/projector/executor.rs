//! executor.rs — Dispatches input actions to the OS via platform impl.

/// A discrete input action the agent can send to the OS.
/// `count` on MouseClick = clicks (1 single, 2 double). Scroll is in wheel clicks,
/// +dy = down / +dx = right (same convention as `perception::PointerAction`).
#[derive(Debug, Clone)]
pub enum InputAction {
    MouseClick  { x: i32, y: i32, button: u8, count: u8 },
    TypeText    { text: String },
    KeyPress    { key: String },
    MouseMove   { x: i32, y: i32 },
    MouseScroll { dx: i32, dy: i32 },
    MouseDrag   { x1: i32, y1: i32, x2: i32, y2: i32, button: u8 },
}

/// Result of executing an action.
#[derive(Debug, Clone)]
pub struct ActionResult {
    pub success: bool,
    pub detail:  String,
}

impl ActionResult {
    pub fn ok(detail: impl Into<String>) -> Self {
        Self { success: true, detail: detail.into() }
    }
    pub fn err(detail: impl Into<String>) -> Self {
        Self { success: false, detail: detail.into() }
    }
}

/// Platform-agnostic executor trait.
pub trait Executor: Send + Sync {
    fn execute(&self, action: InputAction) -> ActionResult;
}

/// No-op executor for tests / CI / platforms not yet implemented.
pub struct NullExecutor;
impl Executor for NullExecutor {
    fn execute(&self, action: InputAction) -> ActionResult {
        tracing::debug!("NullExecutor: {:?}", action);
        ActionResult::ok(format!("null: {:?}", action))
    }
}

/// Build the right executor for the current platform.
pub fn platform_executor() -> Box<dyn Executor> {
    #[cfg(target_os = "linux")]
    return Box::new(super::platform_linux::LinuxExecutor::new());

    #[cfg(not(target_os = "linux"))]
    Box::new(NullExecutor)
}
