//! react_probe — ReAct with a SEARCH tool. planner_probe showed PLAN-AHEAD hallucinates dead plans
//! ("play beyonce" -> `start firefox`; "order pizza" -> curl a fake URL). This tests the fix: give the
//! small model a web-search tool and a Reason -> Act(SEARCH/STEP/DONE) -> Observe loop, so when it
//! DOESN'T KNOW it looks up real facts/URLs/apps instead of inventing them — and the search results
//! become the grounded plan. No VM; the loop + the live :8080 brain + real web search are the subject.
//! Run: cargo run --bin react_probe

#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, inference::InferenceAdapter};
    use lagado_agent::tools::executor;
    use serde_json::Value;

    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));

    // The same goals planner_probe hallucinated on — so the before/after is apples to apples.
    let goals = [
        "play beyonce",
        "order me a pizza",
        "find me a recipe for lasagna and show it",
        "download a CSV of world population by country, then tell me the 5 largest",
    ];

    for goal in goals {
        println!("\n══════════════ GOAL: {goal}");
        let mut observations = String::from("(nothing observed yet)");
        let mut plan: Vec<String> = Vec::new();
        let mut searched = 0;

        for step in 1..=6 {
            let prompt = format!(
"You are a local desktop agent. Take ONE action at a time; you'll see the result before the next.
Output EXACTLY one line, one of:
  SEARCH: <query>   — search the web when you DON'T KNOW how to do something, or need a REAL url / app
                      name / fact. ALWAYS prefer searching over guessing or inventing.
  STEP: <action>    — a concrete step you are now SURE about, grounded in what you observed
                      (open an app / type text / click something / run a shell command).
  DONE              — the goal is accomplished.

Goal: {goal}

Observed so far:
{observations}

Next action:");
            let out = match adapter.generate(&prompt, 80, 0.1) {
                Ok(t) => t.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").to_string(),
                Err(e) => { println!("  ! model error: {e}"); break; }
            };
            println!("  [{step}] {out}");
            let lower = out.to_lowercase();

            if lower.starts_with("done") {
                println!("        → declares done.");
                break;
            } else if lower.starts_with("search") {
                searched += 1;
                let q = out.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                let mut args = serde_json::Map::new();
                args.insert("query".into(), Value::String(q.clone()));
                args.insert("num_results".into(), Value::Number(3.into()));
                let results = executor::dispatch("web_search", &args).await
                    .unwrap_or_else(|| "(web_search tool returned nothing)".to_string());
                let snip: String = results.chars().take(500).collect();
                for l in snip.lines() { println!("        ┊ {l}"); }
                observations = format!("{observations}\n- I searched \"{q}\" and found:\n{snip}");
            } else {
                // STEP: (or an unlabeled line) — a grounded plan step.
                let s = if lower.starts_with("step") {
                    out.splitn(2, ':').nth(1).unwrap_or("").trim().to_string()
                } else { out.clone() };
                if !s.is_empty() {
                    plan.push(s.clone());
                    observations = format!("{observations}\n- I did: {s} (assume it worked)");
                }
            }
        }

        println!("  ──── grounded plan ({} search step{}) ────", searched, if searched == 1 { "" } else { "s" });
        if plan.is_empty() {
            println!("     (no concrete steps emitted)");
        } else {
            for (i, s) in plan.iter().enumerate() { println!("     {}. {s}", i + 1); }
        }
    }
    println!("\n(compare to planner_probe: did SEARCH replace the hallucinated URLs/apps with real ones?)");
}
