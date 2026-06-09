mod action_graph;
mod types;
mod bracket_parser;
mod forge;
mod gate;
mod governor;
mod hydra;
mod operator;
mod memory;
mod chronos;
mod envelope;
mod inference;
mod config;
mod perception;
mod agent;
mod bootstrap;

fn main() {
    // CLI dev entry point — starts llama-server only.
    // The production binary is the Tauri app in lagado-ui/src-tauri/.
    tracing_subscriber::fmt::init();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let _child = bootstrap::ensure_llama_server().await;
        tracing::info!("llama-server ready. This CLI runner holds it alive — Ctrl-C to exit.");
        tokio::signal::ctrl_c().await.ok();
    });
}
