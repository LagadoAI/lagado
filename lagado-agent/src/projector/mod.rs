//! projector/mod.rs — Cross-platform OS input dispatch.
//!
//! Separates the "send input to OS" concern from perception (reading the screen).
//! Linux: xdotool / AT-SPI2 activation
//! macOS: stub (Phase 2: Accessibility API)
//! Windows: stub (Phase 2: UIAutomation / SendInput)

pub mod executor;
pub mod validator;

#[cfg(target_os = "linux")]
pub mod platform_linux;

pub use executor::{Executor, InputAction, ActionResult};
pub use validator::Validator;
