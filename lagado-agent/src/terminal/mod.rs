//! terminal/mod.rs — PTY manager for terminal pages.
//!
//! Spawns shell processes under a PTY, streams output to the UI,
//! accepts input from the UI. Cross-platform interface.

pub mod pty;
pub use pty::{PtySession, PtyManager};
