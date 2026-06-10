mod action_graph;
mod agent;
mod auth;
mod bootstrap;
mod bracket_parser;
mod chronos;
mod config;
mod distill;
mod envelope;
mod forge;
mod gate;
mod governor;
mod grammar;
mod hydra;
mod inference;
mod kv_slots;
mod liquid;
mod mcp;
mod memory;
mod memory_tiers;
mod operator;
mod perception;
mod projector;
mod recovery;
mod retrieval;
mod security;
mod self_model;
mod skill_library;
mod sleep_gate;
mod terminal;
mod tools;
mod types;
mod vision;

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
