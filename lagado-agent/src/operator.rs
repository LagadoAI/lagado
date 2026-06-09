const MAX_STEPS: usize = 50;
const URGENCY_THRESHOLD: usize = 10;

pub struct StepEnforcer {
    current_step: usize,
}

impl StepEnforcer {
    pub fn new() -> Self { Self { current_step: 0 } }

    pub fn advance(&mut self) -> Result<usize, crate::types::PipelineError> {
        self.current_step += 1;
        if self.current_step > MAX_STEPS {
            Err(crate::types::PipelineError::MaxStepsExceeded)
        } else {
            Ok(self.current_step)
        }
    }

    /// Wrap a prompt with step annotation and optional urgency nudge.
    pub fn annotate(&self, prompt: &str) -> String {
        let header = format!(
            "(Step {} of max {MAX_STEPS})\n",
            self.current_step
        );

        let urgency = if self.current_step >= URGENCY_THRESHOLD {
            "\nYou must make progress toward the goal now. Output ONE action.\n"
        } else {
            ""
        };

        format!("{header}{urgency}{prompt}")
    }

    pub fn step(&self) -> usize { self.current_step }
}

/// A tool the agent can invoke, with description for retrieval scoring.
#[derive(Debug, Clone)]
pub struct ToolDescriptor {
    pub name:        String,
    pub description: String,
    pub risk_level:  RiskLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiskLevel {
    Read,        // safe, auto-allow
    Write,       // requires tap-confirm
    Destructive, // requires typed-confirm
}

/// The canonical tool set available to the agent.
/// Phase 2: merge with MCP-loaded tools at runtime.
pub fn core_tools() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "click".into(),
            description: "Click a UI element by ref_id. Use for buttons, links, checkboxes, menu items.".into(),
            risk_level: RiskLevel::Write,
        },
        ToolDescriptor {
            name: "type".into(),
            description: "Type text into an editable field by ref_id. Use for text inputs, search boxes, forms.".into(),
            risk_level: RiskLevel::Write,
        },
        ToolDescriptor {
            name: "key".into(),
            description: "Press a keyboard key (Return, Escape, Tab, Ctrl+c, etc).".into(),
            risk_level: RiskLevel::Write,
        },
        ToolDescriptor {
            name: "wait".into(),
            description: "Wait a number of milliseconds before the next action. Use when waiting for page load or animation.".into(),
            risk_level: RiskLevel::Read,
        },
        ToolDescriptor {
            name: "done".into(),
            description: "Signal the goal is complete with a short reason.".into(),
            risk_level: RiskLevel::Read,
        },
    ]
}
