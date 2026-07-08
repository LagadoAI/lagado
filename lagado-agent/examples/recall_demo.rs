//! Drive the `recall` tool from the CLI against the real timeline.
//! Usage: cargo run --example recall_demo [-- <day> [query]]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let day = args.get(1).cloned().unwrap_or_default();
    let query = args.get(2).cloned().unwrap_or_default();
    println!("{}", lagado_agent::chronos::recall(&day, "", "", &query, 25));
}
