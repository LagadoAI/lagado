//! routing_probe — what does the intent router ACTUALLY do with natural task goals? The gateway to
//! autonomous action: a goal misrouted to CHAT never reaches the planner. No VM. Run: cargo run --bin routing_probe

#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, hydra::Hydra, inference::InferenceAdapter};

    let adapter: Arc<dyn InferenceAdapter + Send + Sync> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));
    let hydra = Hydra::from_governor(adapter);

    // (goal, what it SHOULD be)
    let goals = [
        ("create two empty files: /tmp/a and /tmp/b", "INTERACTIVE"),
        ("make a directory called /tmp/project", "INTERACTIVE"),
        ("delete the file /tmp/old.log", "INTERACTIVE"),
        ("show how much disk space is free", "INTERACTIVE"),
        ("what process is using port 8080", "INTERACTIVE"),
        ("install ripgrep", "INTERACTIVE"),
        ("rename report.txt to final.txt", "INTERACTIVE"),
        ("open the web browser", "INTERACTIVE (fast-path)"),
        ("launch the terminal emulator", "INTERACTIVE (fast-path)"),
        ("hello there, how are you", "CHAT"),
        ("what is the capital of France", "CHAT"),
        ("write a poem about the sea", "CHAT/REASONING"),
        ("explain how TCP works", "REASONING"),
    ];
    println!("{:<46} {:<22} {}", "GOAL", "EXPECTED", "GOT");
    println!("{}", "─".repeat(90));
    for (g, expect) in goals {
        let intent = hydra.classify_intent(g).await;
        let got = format!("{intent:?}");
        let flag = if expect.contains(&got.to_uppercase()) || (expect.contains("INTERACTIVE") && got == "Interactive") { "" } else { "  ⟵ MISROUTE?" };
        println!("{:<46} {:<22} {}{}", g, expect, got, flag);
    }
}
