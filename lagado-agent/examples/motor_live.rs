//! motor_live.rs — drive the completed motor surface against a live OSWorld guest
//! and capture frame windows around each action for the eyes to characterize.
//!
//! Usage: cargo run --example motor_live -- <host> <port> <outdir>

use lagado_agent::perception::{Actuator, MouseButton, Perceptor, PointerAction};
use lagado_agent::vm::osworld::osworld_pair;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port: u16 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5000);
    let outdir = args.get(3).cloned().unwrap_or_else(|| "/tmp/lagado_motor_demo".into());

    let (perceptor, actuator) = osworld_pair(&host, port);

    let snap = |phase: &str, n: usize| {
        perceptor.capture_frame();
        let dst = format!("{outdir}/{phase}");
        let _ = std::fs::create_dir_all(&dst);
        let _ = std::fs::copy(lagado_agent::config::FRAME_PATH, format!("{dst}/f{n:02}.png"));
    };
    let settle = |ms: u64| std::thread::sleep(Duration::from_millis(ms));

    // ── stage: a terminal full of scrollable content ──
    println!("▸ opening terminal with 400 lines of content…");
    let out = actuator.run_command(
        "DISPLAY=:0 gnome-terminal --geometry=120x40+200+100 -- bash -c 'seq 1 400; exec sleep 600' >/dev/null 2>&1; sleep 3; echo STAGED");
    println!("  {}", out.lines().last().unwrap_or(""));
    settle(2000);

    // pointer to the terminal's middle so the wheel lands on it
    println!("▸ hover (MoveTo terminal center)…");
    println!("  {}", actuator.pointer(&PointerAction::MoveTo { x: 760, y: 500 }));
    settle(500);

    // ── SCROLL UP (reveal earlier lines) with frames around it ──
    println!("▸ scroll up 5 wheel clicks…");
    for n in 0..2 { snap("scroll", n); settle(250); }
    println!("  {}", actuator.pointer(&PointerAction::Scroll { dx: 0, dy: -5 }));
    for n in 2..6 { settle(250); snap("scroll", n); }

    // ── RIGHT-CLICK on the desktop (context menu = popup) ──
    println!("▸ right-click desktop…");
    for n in 0..2 { snap("rightclick", n); settle(250); }
    println!("  {}", actuator.pointer(&PointerAction::ClickAt { x: 1700, y: 900, button: MouseButton::Right, count: 1 }));
    for n in 2..6 { settle(300); snap("rightclick", n); }
    // close the menu again (Escape)
    let _ = actuator.key("Escape");
    settle(400);

    // ── DRAG the xterm window by its title bar ──
    println!("▸ drag window 250px right / 120px down…");
    for n in 0..2 { snap("drag", n); settle(250); }
    println!("  {}", actuator.pointer(&PointerAction::Drag { x1: 760, y1: 110, x2: 1010, y2: 230, button: MouseButton::Left }));
    for n in 2..6 { settle(300); snap("drag", n); }

    // ── DOUBLE-CLICK a word inside the terminal (select word — small local change) ──
    println!("▸ double-click a word in the terminal…");
    for n in 0..2 { snap("dblclick", n); settle(250); }
    println!("  {}", actuator.pointer(&PointerAction::ClickAt { x: 1030, y: 620, button: MouseButton::Left, count: 2 }));
    for n in 2..6 { settle(300); snap("dblclick", n); }

    println!("done → frames in {outdir}/<phase>/");
}
