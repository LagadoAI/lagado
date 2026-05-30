use std::pin::Pin;
use std::future::Future;
use crate::{
    operator::StepEnforcer,
    parser::{parse_tool_call, rescue_parse},
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
            match parse_tool_call(&last_raw) {
                Ok(tool) => return Ok(tool),
                Err(PipelineError::ParseFailed(ref raw)) if attempt < MAX_RETRIES => {
                    let nudge = build_nudge_prompt(raw, "JSON parse error");
                    let nudge_with_step = enforcer.annotate(&nudge);
                    last_raw = (self.model_fn)(nudge_with_step).await?;
                }
                Err(_) => break,
            }
        }

        rescue_parse(&last_raw)
            .ok_or(PipelineError::MaxRetriesExceeded)
    }
}

fn build_nudge_prompt(raw_output: &str, parse_error: &str) -> String {
    format!(
        "Your previous output failed to parse as JSON.\n\
         Parse error: {parse_error}\n\n\
         Your output was:\n```\n{raw_output}\n```\n\n\
         Please output ONLY the corrected JSON tool call, \
         with no explanation, markdown, or extra text."
    )
}
