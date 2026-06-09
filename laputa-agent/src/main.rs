mod action_graph;
mod types;
mod bracket_parser;
mod forge;
mod gate;
mod governor;
mod operator;
mod memory;
mod chronos;
mod envelope;
mod inference;
mod config;
mod perception;
mod agent;
mod server;
mod bootstrap;

use std::sync::Arc;
use tokio::sync::Mutex;
use inference::llama_cpp::LlamaCppAdapter;
use inference::InferenceAdapter;
use perception::{Perceptor, Actuator, MockPerceptor, MockActuator};
use agent::AgentState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Keep the llama-server child alive for the program's duration.
    let _server_child = bootstrap::ensure_llama_server().await;

    let model_path = config::model_path();
    let adapter: Arc<dyn InferenceAdapter> = match LlamaCppAdapter::new(&model_path.to_string_lossy(), config::CONTEXT_SIZE) {
        Ok(a) => Arc::new(a),
        Err(e) => {
            tracing::error!("failed to construct inference adapter: {e}");
            std::process::exit(1);
        }
    };

    let perceptor: Arc<dyn Perceptor> = Arc::new(MockPerceptor);
    let actuator: Arc<dyn Actuator> = Arc::new(MockActuator);

    let state = Arc::new(Mutex::new(AgentState {
        goal: String::new(),
        running: false,
        approval_tx: None,
        pending_id: None,
    }));

    tokio::spawn(server::run_ws_server(state, adapter, perceptor, actuator));

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
