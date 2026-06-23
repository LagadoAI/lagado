//! session_drive — drive ONE calc task through the NATIVE SESSION with a HAND op-log (no model).
//!
//! P2c validation step #1: proves the Rust `NativeSession` driver works end-to-end against the
//! REAL OSWorld guest (deploy → daemon → apply → reconcile → gold), isolating it from model
//! variability. The Python runner boots the env + scores; this binary applies a hand-written
//! op-log via the session and reconciles — exactly what the agent's InApp loop does, minus the
//! LLM authoring. It exercises the SAME `NativeSession` code wired into agent.rs.
//!
//! Usage: session_drive <http://guest_ip:port> <guest_file_path> <oplog_json_path>

#[cfg(not(unix))]
fn main() {
    eprintln!("[session_drive] Unix required");
}

#[cfg(unix)]
fn main() {
    use lagado_agent::native_session::NativeSession;
    use lagado_agent::perception::Actuator;
    use lagado_agent::vm::OsworldActuator;
    use std::sync::Arc;

    let args: Vec<String> = std::env::args().collect();
    let base_url = args.get(1).cloned().unwrap_or_default();
    let file = args.get(2).cloned().unwrap_or_default();
    let oplog_path = args.get(3).cloned().unwrap_or_default();
    if base_url.is_empty() || file.is_empty() || oplog_path.is_empty() {
        eprintln!("usage: session_drive <http://guest_ip:port> <guest_file> <oplog_json>");
        std::process::exit(2);
    }

    // parse host:port out of the guest URL (mirror osworld_run)
    let stripped = base_url.strip_prefix("http://").unwrap_or(&base_url);
    let (host, port) = match stripped.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.trim_end_matches('/').parse::<u16>().unwrap_or(5000)),
        None => (stripped.to_string(), 5000),
    };

    let ops: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(&oplog_path).expect("read oplog file"),
    )
    .expect("parse oplog json (expect a JSON array of ops)");

    let actuator: Arc<dyn Actuator> = Arc::new(OsworldActuator::new(&host, port));

    let session = match NativeSession::deploy_and_open(actuator, &file) {
        Ok(s) => {
            println!("[session] deployed + opened {file}");
            s
        }
        Err(e) => {
            eprintln!("[session] deploy/open FAILED: {e}");
            std::process::exit(1);
        }
    };

    let mut applied = 0usize;
    for (i, op) in ops.iter().enumerate() {
        let kind = op.get("op").and_then(|v| v.as_str()).unwrap_or("?");
        match session.apply(op) {
            Ok(()) => {
                applied += 1;
                println!("[apply {i}] ok: {kind}");
            }
            Err(e) => eprintln!("[apply {i}] ERR ({kind}): {e}"),
        }
    }

    match session.reconcile() {
        Ok(()) => println!("[reconcile] ok ({applied}/{} ops applied)", ops.len()),
        Err(e) => {
            eprintln!("[reconcile] ERR: {e}");
            session.close();
            std::process::exit(1);
        }
    }
    session.close();
    println!("[done] native session reconciled {applied} ops");
}
