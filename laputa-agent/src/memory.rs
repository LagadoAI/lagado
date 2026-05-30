use crate::types::Step;

const COMPACT_AFTER:  usize = 10; // compact when history exceeds this
const COMPACT_OLDEST: usize = 5;  // how many old steps to summarize
const KEEP_RECENT:    usize = 5;  // full-detail steps to always keep

pub struct Memory {
    summaries: Vec<String>,     // compacted summaries
    recent:    Vec<Step>,       // full-detail recent steps
    summarizer: Box<dyn Fn(&[Step]) -> String + Send + Sync>,
}

impl Memory {
    pub fn new(summarizer: impl Fn(&[Step]) -> String + Send + Sync + 'static) -> Self {
        Self {
            summaries:  Vec::new(),
            recent:     Vec::new(),
            summarizer: Box::new(summarizer),
        }
    }

    pub fn push(&mut self, step: Step) {
        self.recent.push(step);
        if self.recent.len() > COMPACT_AFTER {
            self.compact();
        }
    }

    /// Build the context string injected into every prompt.
    pub fn context_string(&self) -> String {
        let mut parts = Vec::new();

        if !self.summaries.is_empty() {
            parts.push(format!("[History summary]\n{}", self.summaries.join("\n")));
        }

        let recent_strs: Vec<String> = self.recent
            .iter()
            .map(|s| format!("Step {}: {}", s.index, s.output.trim()))
            .collect();

        if !recent_strs.is_empty() {
            parts.push(format!("[Recent steps]\n{}", recent_strs.join("\n")));
        }

        parts.join("\n\n")
    }

    // ── private ──────────────────────────────────────────────────────────────

    fn compact(&mut self) {
        if self.recent.len() <= KEEP_RECENT {
            return;
        }

        // Take the oldest steps beyond the keep-recent window
        let to_summarize_count = self.recent.len() - KEEP_RECENT;
        // But cap at COMPACT_OLDEST per compaction cycle
        let count = to_summarize_count.min(COMPACT_OLDEST);

        let old_steps: Vec<Step> = self.recent.drain(..count).collect();
        let first = old_steps.first().map(|s| s.index).unwrap_or(0);
        let last  = old_steps.last().map(|s| s.index).unwrap_or(0);

        let summary_text = (self.summarizer)(&old_steps);
        self.summaries.push(format!("Steps {first}–{last}: {summary_text}"));
    }
}
