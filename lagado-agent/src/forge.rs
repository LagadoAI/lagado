use std::pin::Pin;
use std::future::Future;
use crate::{
    operator::StepEnforcer,
    bracket_parser::parse_bracket_tool_call,
    types::{PipelineError, ToolCall},
};

const MAX_RETRIES: usize = 2;

pub struct Forge {
    pub model_fn: Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(String, f32), PipelineError>> + Send>> + Send + Sync>,
}

impl Forge {
    /// Call the model with retry on parse failure.
    /// Returns `(ToolCall, confidence)` where confidence is 0.0–1.0.
    /// Confidence is 1.0 when the adapter doesn't support logprobs — callers
    /// must treat 1.0 as "no information", not as "certain".
    pub async fn call_with_retry(
        &self,
        prompt: &str,
        enforcer: &StepEnforcer,
    ) -> Result<(ToolCall, f32), PipelineError> {
        let full_prompt = enforcer.annotate(prompt);
        let (mut last_raw, mut confidence) = (self.model_fn)(full_prompt.clone()).await?;

        for attempt in 0..=MAX_RETRIES {
            match parse_bracket_tool_call(&last_raw) {
                Ok(tool) => return Ok((tool, confidence)),
                Err(PipelineError::ParseFailed(ref raw)) if attempt < MAX_RETRIES => {
                    let nudge = build_nudge_prompt(raw, "bracket parse error");
                    let nudge_with_step = enforcer.annotate(&nudge);
                    let (new_raw, new_conf) = (self.model_fn)(nudge_with_step).await?;
                    last_raw = new_raw;
                    confidence = new_conf; // take confidence from final attempt
                }
                Err(_) => break,
            }
        }

        parse_bracket_tool_call(&last_raw)
            .map(|tool| (tool, confidence))
            .map_err(|_| PipelineError::MaxRetriesExceeded)
    }
}

fn build_nudge_prompt(raw_output: &str, parse_error: &str) -> String {
    format!(
        "Your previous output failed to parse as a bracket tool call.\n\
         Parse error: {parse_error}\n\n\
         Your output was:\n```\n{raw_output}\n```\n\n\
         Respond with ONLY one bracket tool call. Example: click(selector=\"ref_3\")"
    )
}
