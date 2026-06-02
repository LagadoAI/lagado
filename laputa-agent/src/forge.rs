use std::pin::Pin;
use std::future::Future;
use crate::{
    operator::StepEnforcer,
    bracket_parser::parse_bracket_tool_call,
    types::{PipelineError, ToolCall},
};

const MAX_RETRIES: usize = 2;

pub struct Forge {
    pub model_fn: Box<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<String, PipelineError>> + Send>> + Send + Sync>,
}

impl Forge {
    pub async fn call_with_retry(
        &self,
        prompt: &str,
        enforcer: &StepEnforcer,
    ) -> Result<ToolCall, PipelineError> {
        let full_prompt = enforcer.annotate(prompt);
        let mut last_raw = (self.model_fn)(full_prompt.clone()).await?;

        for attempt in 0..=MAX_RETRIES {
            match parse_bracket_tool_call(&last_raw) {
                Ok(tool) => return Ok(tool),
                Err(PipelineError::ParseFailed(ref raw)) if attempt < MAX_RETRIES => {
                    let nudge = build_nudge_prompt(raw, "bracket parse error");
                    let nudge_with_step = enforcer.annotate(&nudge);
                    last_raw = (self.model_fn)(nudge_with_step).await?;
                }
                Err(_) => break,
            }
        }

        parse_bracket_tool_call(&last_raw)
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
