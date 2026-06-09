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
