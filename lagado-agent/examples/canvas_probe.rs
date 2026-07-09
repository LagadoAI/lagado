//! canvas_probe.rs — exercise THE capture path live: read the shared-memory canvas
//! through the real Rust reader and run the real CV proposer on it. No PNG anywhere.
fn main() {
    let t0 = std::time::Instant::now();
    match lagado_agent::perception::canvas::canvas_seq() {
        Some((w, h, seq)) => println!("canvas header: {w}x{h} seq={seq}"),
        None => { println!("no canvas — is canvas_feed running?"); return; }
    }
    let (rgb, w, h) = lagado_agent::perception::canvas::read_rgb().expect("pixels");
    let t_read = t0.elapsed();
    let boxes = lagado_agent::perception::cv_proposer::propose_frame(&rgb, w, h);
    println!("read+swizzle {w}x{h} in {:?}; CV proposed {} boxes in {:?} total",
             t_read, boxes.len(), t0.elapsed());
    std::thread::sleep(std::time::Duration::from_secs(2));
    if let Some((_, _, seq2)) = lagado_agent::perception::canvas::canvas_seq() {
        println!("seq after 2s: {seq2} (liveness probe — no pixel read needed)");
    }
}
