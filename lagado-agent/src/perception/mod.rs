//! perception/mod.rs — Screen reading (Perceptor) and input actuation (Actuator).
//!
//! Platform-specific implementations (e.g. Windows UIAutomation) drop in behind
//! these traits. `MockPerceptor`/`MockActuator` let the agent core build and run
//! on any OS and in CI with no desktop dependency.

/// Reads the focused window's interactive elements as a text dump.
pub trait Perceptor: Send + Sync {
    fn read_screen(&self) -> String;
}

/// Performs user-input actions on the desktop.
pub trait Actuator: Send + Sync {
    fn click(&self, selector: &str) -> String;
    fn type_text(&self, selector: &str, text: &str) -> String;
    fn key(&self, key: &str) -> String;
}

/// Canned perceptor for tests / headless CI / pre-platform-impl development.
pub struct MockPerceptor;

impl Perceptor for MockPerceptor {
    fn read_screen(&self) -> String {
        "[focused: Mock Window]\n\
         [window: x=0 y=0 w=1280 h=720]\n  \
         ref_1  button   \"OK\"      state=enabled\n  \
         ref_2  entry    \"Search\"  state=editable"
            .to_string()
    }
}

/// Echoing actuator for tests / headless CI / pre-platform-impl development.
pub struct MockActuator;

impl Actuator for MockActuator {
    fn click(&self, selector: &str) -> String {
        format!("Clicked {selector}")
    }
    fn type_text(&self, selector: &str, text: &str) -> String {
        format!("Typed {} chars into {}", text.chars().count(), selector)
    }
    fn key(&self, key: &str) -> String {
        format!("Pressed {key}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_actuator_describes_actions() {
        let a = MockActuator;
        assert_eq!(a.click("ref_3"), "Clicked ref_3");
        assert_eq!(a.type_text("ref_5", "hi"), "Typed 2 chars into ref_5");
        assert_eq!(a.key("Return"), "Pressed Return");
    }

    #[test]
    fn mock_perceptor_returns_focused_dump() {
        let p = MockPerceptor;
        assert!(p.read_screen().contains("focused"));
    }
}
