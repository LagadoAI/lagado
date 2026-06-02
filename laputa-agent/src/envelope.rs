use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── Wire envelope ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub kind: String,
    pub payload: Value,
}

// ── Typed payload structs ─────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionPayload {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,           // "tap" | "typed"
    pub tool: String,            // "click" | "type" | "key"
    pub action: String,          // bracket call description
    pub reason: String,
    pub origin_surface: String,
    pub origin_agent: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApprovalPayload {
    pub id: String,
    pub approved: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoalPayload {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandPayload {
    pub cmd: String,             // "pause" | "resume" | "stop"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ActionLogPayload {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusPayload {
    pub state: String,           // "goal_received" | "goal_done" | "goal_aborted" | "blocked" | "denied"
    pub detail: String,
}

// ── Helpers ───────────────────────────────────────────────────────

pub fn make(kind: &str, payload: impl Serialize) -> String {
    let env = serde_json::json!({
        "v": 1u8,
        "kind": kind,
        "payload": serde_json::to_value(payload).unwrap_or(Value::Null),
    });
    serde_json::to_string(&env).unwrap_or_default()
}

pub fn parse(raw: &str) -> Option<Envelope> {
    serde_json::from_str(raw).ok()
}

// ── Unit test ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_round_trip() {
        let json = make("permission", PermissionPayload {
            id: "test-uuid-1234".to_string(),
            type_: "tap".to_string(),
            tool: "click".to_string(),
            action: r#"click(selector="ref_3")"#.to_string(),
            reason: "Write action requires confirmation".to_string(),
            origin_surface: "immersive".to_string(),
            origin_agent: "main".to_string(),
        });

        let env = parse(&json).expect("must parse");
        assert_eq!(env.v, 1);
        assert_eq!(env.kind, "permission");
        assert_eq!(env.payload["type"], "tap");
        assert_eq!(env.payload["tool"], "click");
        assert_eq!(env.payload["id"], "test-uuid-1234");

        // "type" field must not appear as "type_" in JSON
        assert!(!json.contains("type_"));
        assert!(json.contains(r#""type":"tap""#));

        println!("permission JSON: {json}");
    }

    #[test]
    fn approval_parse() {
        let raw = r#"{"v":1,"kind":"approval","payload":{"id":"abc","approved":true}}"#;
        let env = parse(raw).unwrap();
        let p: ApprovalPayload = serde_json::from_value(env.payload).unwrap();
        assert_eq!(p.id, "abc");
        assert!(p.approved);
    }
}
