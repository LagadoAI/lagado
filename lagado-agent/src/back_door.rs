//! Back-door plane — route AROUND an app's API/GUI by operating from the CLI: write a setting without the
//! app's UI, or transform a file with a sibling CLI tool, then reconcile (M1 reload-into-focus) so the live
//! app reflects the change. First-class TYPED verbs (closes the skew-audit #7 gap: the back-door used to be
//! only prompt text, never a capability). CLASS-GENERAL by the proven pattern: the model SELECTS a verb and
//! fills typed slots; the harness builds ONE deterministic command that runs through the SAME `gate` as every
//! other actuation. The verb is the class; platform/tool lives in a typed slot, not a hardcoded branch.

/// Where a setting lives. The CLASS is "write a config value"; the VARIANT is the platform/mechanism (so a
/// Windows registry / macOS defaults variant slots in later without new control flow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigBackend {
    /// dconf path write — schema-agnostic, the most general Linux desktop config route. `key` is a full
    /// `/org/.../foo` path.
    Dconf,
    /// gsettings — needs the schema id alongside the key.
    Gsettings { schema: String },
    /// A plain `key=value` line in a config FILE (the universal fallback for apps with an ini-style rc).
    IniFile { path: String },
}

/// A route-around operation the model can author. Two verbs cover the proven back-door surface; more slot in
/// as typed variants, never as per-app branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackDoorOp {
    /// Set an app/desktop setting without its GUI (the running app may need an M1 reload to pick it up).
    SetConfig { backend: ConfigBackend, key: String, value: String },
    /// Transform a file with a sibling CLI tool (route around the app entirely), `input` → `output`.
    RunSibling { tool: String, input: String, output: String, args: Vec<String> },
}

use std::collections::HashMap;

/// Map a parsed typed-verb call (verb + kwargs, from the shared Pythonic `parse_capability_call`) into a
/// `BackDoorOp`. The model SELECTS the verb and fills slots (grammar-constrained); this validates + types
/// it. `None` = malformed/unknown (the loop re-emits, never runs garbage). Class-general: new backends are
/// new match arms over a typed slot, not per-app branches.
pub fn from_call(verb: &str, kw: &HashMap<String, String>) -> Option<BackDoorOp> {
    match verb {
        "set_config" => {
            let key = kw.get("key")?.clone();
            let value = kw.get("value").cloned().unwrap_or_default();
            let backend = match kw.get("backend").map(String::as_str) {
                Some("dconf") => ConfigBackend::Dconf,
                Some("gsettings") => ConfigBackend::Gsettings { schema: kw.get("schema")?.clone() },
                Some("file") => ConfigBackend::IniFile { path: kw.get("path")?.clone() },
                _ => return None,
            };
            Some(BackDoorOp::SetConfig { backend, key, value })
        }
        "run_sibling" => Some(BackDoorOp::RunSibling {
            tool: kw.get("tool")?.clone(),
            input: kw.get("input")?.clone(),
            output: kw.get("output")?.clone(),
            args: kw.get("args").map(|a| a.split_whitespace().map(String::from).collect()).unwrap_or_default(),
        }),
        _ => None,
    }
}

/// Shell-quote a single argument (POSIX single-quote, with `'\''` escaping). Keeps model-supplied values
/// from breaking out of the command — the `gate` is the authority on whether it RUNS, this just makes the
/// built string faithful to the typed slots.
fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build ONE deterministic shell command for a back-door op (or `None` if a required slot is empty). The
/// result still passes through `gate::evaluate_action` like any command — this only renders the typed op.
pub fn to_command(op: &BackDoorOp) -> Option<String> {
    match op {
        BackDoorOp::SetConfig { backend, key, value } => {
            if key.trim().is_empty() {
                return None;
            }
            match backend {
                ConfigBackend::Dconf => {
                    if !key.starts_with('/') {
                        return None; // dconf keys are absolute paths
                    }
                    Some(format!("dconf write {} {}", shq(key), shq(value)))
                }
                ConfigBackend::Gsettings { schema } => {
                    if schema.trim().is_empty() {
                        return None;
                    }
                    Some(format!("gsettings set {} {} {}", shq(schema), shq(key), shq(value)))
                }
                ConfigBackend::IniFile { path } => {
                    if path.trim().is_empty() {
                        return None;
                    }
                    // Idempotent: replace the key's line in place if present, else append it. `key=value`.
                    let kv = format!("{key}={value}");
                    Some(format!(
                        "grep -q {k} {p} && sed -i {expr} {p} || printf '%s\\n' {kv} >> {p}",
                        k = shq(&format!("^{key}=")),
                        p = shq(path),
                        expr = shq(&format!("s|^{key}=.*|{kv}|")),
                        kv = shq(&kv),
                    ))
                }
            }
        }
        BackDoorOp::RunSibling { tool, input, output, args } => {
            if tool.trim().is_empty() || input.trim().is_empty() || output.trim().is_empty() {
                return None;
            }
            let mut parts = vec![shq(tool), shq(input)];
            parts.extend(args.iter().map(|a| shq(a)));
            parts.push(shq(output));
            Some(parts.join(" "))
        }
    }
}

/// Build the READ-BACK falsifier for an applied op: `Some((command, expected))` where running
/// `command` and comparing its output against `expected` (via `readback_matches`) decides
/// verified/unverified. Sound in one direction only — a mismatch proves the op did NOT take;
/// a match is necessary, not sufficient (the gate on claiming "done" stays honest).
pub fn verify_command(op: &BackDoorOp) -> Option<(String, String)> {
    match op {
        BackDoorOp::SetConfig { backend, key, value } => match backend {
            ConfigBackend::Dconf => Some((format!("dconf read {}", shq(key)), value.clone())),
            ConfigBackend::Gsettings { schema } => Some((
                format!("gsettings get {} {}", shq(schema), shq(key)),
                value.clone(),
            )),
            ConfigBackend::IniFile { path } => Some((
                format!("grep -q {} {} && echo LAGADO_OK",
                        shq(&format!("^{key}={value}")), shq(path)),
                "LAGADO_OK".to_string(),
            )),
        },
        BackDoorOp::RunSibling { output, .. } => Some((
            format!("test -e {} && echo LAGADO_OK", shq(output)),
            "LAGADO_OK".to_string(),
        )),
    }
}

/// Does a read-back output confirm the expected value? Normalizes the representational noise
/// between write and read dialects (quote style, whitespace, case, gsettings type prefixes like
/// `uint32 5`) WITHOUT weakening the value comparison itself. Pure.
pub fn readback_matches(readback: &str, expected: &str) -> bool {
    fn norm(s: &str) -> String {
        s.trim()
            .trim_start_matches("uint32 ").trim_start_matches("int32 ").trim_start_matches("double ")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '\'' && *c != '"')
            .collect::<String>()
            .to_lowercase()
    }
    // ignore harness markers ([exit N] / [stderr] lines) — compare content lines only
    let content: String = readback.lines()
        .filter(|l| !l.trim_start().starts_with('['))
        .collect::<Vec<_>>().join("\n");
    !norm(expected).is_empty() && norm(&content) == norm(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dconf_write_is_schema_agnostic() {
        let op = BackDoorOp::SetConfig {
            backend: ConfigBackend::Dconf,
            key: "/org/gnome/desktop/interface/color-scheme".into(),
            value: "'prefer-dark'".into(),
        };
        assert_eq!(
            to_command(&op).unwrap(),
            "dconf write '/org/gnome/desktop/interface/color-scheme' ''\\''prefer-dark'\\'''"
        );
        // a non-absolute dconf key is invalid → None (no malformed command emitted)
        let bad = BackDoorOp::SetConfig { backend: ConfigBackend::Dconf, key: "color-scheme".into(), value: "x".into() };
        assert_eq!(to_command(&bad), None);
    }

    #[test]
    fn gsettings_needs_schema() {
        let op = BackDoorOp::SetConfig {
            backend: ConfigBackend::Gsettings { schema: "org.gnome.desktop.interface".into() },
            key: "color-scheme".into(),
            value: "prefer-dark".into(),
        };
        assert_eq!(to_command(&op).unwrap(),
                   "gsettings set 'org.gnome.desktop.interface' 'color-scheme' 'prefer-dark'");
        let no_schema = BackDoorOp::SetConfig {
            backend: ConfigBackend::Gsettings { schema: "".into() }, key: "k".into(), value: "v".into() };
        assert_eq!(to_command(&no_schema), None);
    }

    #[test]
    fn inifile_edit_is_idempotent_replace_or_append() {
        let op = BackDoorOp::SetConfig {
            backend: ConfigBackend::IniFile { path: "/home/user/.config/app.conf".into() },
            key: "theme".into(), value: "dark".into(),
        };
        let cmd = to_command(&op).unwrap();
        assert!(cmd.contains("grep -q") && cmd.contains("sed -i") && cmd.contains(">>"));
        assert!(cmd.contains("theme=dark"));
    }

    #[test]
    fn run_sibling_routes_around_the_app() {
        // e.g. set an image palette WITHOUT GIMP, via ImageMagick
        let op = BackDoorOp::RunSibling {
            tool: "convert".into(),
            input: "/home/user/Desktop/computer.png".into(),
            output: "/home/user/Desktop/computer.png".into(),
            args: vec!["-colors".into(), "256".into()],
        };
        assert_eq!(to_command(&op).unwrap(),
            "'convert' '/home/user/Desktop/computer.png' '-colors' '256' '/home/user/Desktop/computer.png'");
        // a missing required slot → None, never a half-built command
        let bad = BackDoorOp::RunSibling { tool: "".into(), input: "a".into(), output: "b".into(), args: vec![] };
        assert_eq!(to_command(&bad), None);
    }

    #[test]
    fn from_call_types_the_verbs() {
        let mut kw = HashMap::new();
        kw.insert("backend".into(), "dconf".into());
        kw.insert("key".into(), "/org/gnome/desktop/interface/color-scheme".into());
        kw.insert("value".into(), "'prefer-dark'".into());
        assert_eq!(from_call("set_config", &kw), Some(BackDoorOp::SetConfig {
            backend: ConfigBackend::Dconf,
            key: "/org/gnome/desktop/interface/color-scheme".into(), value: "'prefer-dark'".into() }));
        // gsettings backend without a schema slot → None (malformed, re-emit)
        let mut g = HashMap::new();
        g.insert("backend".into(), "gsettings".into()); g.insert("key".into(), "k".into());
        assert_eq!(from_call("set_config", &g), None);
        let mut s = HashMap::new();
        s.insert("tool".into(), "convert".into()); s.insert("input".into(), "a.png".into());
        s.insert("output".into(), "b.png".into()); s.insert("args".into(), "-colors 256".into());
        assert_eq!(from_call("run_sibling", &s), Some(BackDoorOp::RunSibling {
            tool: "convert".into(), input: "a.png".into(), output: "b.png".into(),
            args: vec!["-colors".into(), "256".into()] }));
        assert_eq!(from_call("bogus_verb", &HashMap::new()), None);
    }

    #[test]
    fn quoting_contains_injection() {
        let op = BackDoorOp::RunSibling {
            tool: "convert".into(), input: "a; rm -rf ~".into(), output: "b".into(), args: vec![],
        };
        // the dangerous chars are quoted inside a single arg — the gate still decides whether it RUNS
        assert_eq!(to_command(&op).unwrap(), "'convert' 'a; rm -rf ~' 'b'");
    }

    #[test]
    fn verify_command_builds_the_read_back_per_backend() {
        let g = BackDoorOp::SetConfig {
            backend: ConfigBackend::Gsettings { schema: "org.gnome.desktop.interface".into() },
            key: "color-scheme".into(), value: "'prefer-dark'".into(),
        };
        let (cmd, expect) = verify_command(&g).unwrap();
        assert_eq!(cmd, "gsettings get 'org.gnome.desktop.interface' 'color-scheme'");
        assert_eq!(expect, "'prefer-dark'");

        let d = BackDoorOp::SetConfig {
            backend: ConfigBackend::Dconf, key: "/org/x/y".into(), value: "true".into() };
        assert_eq!(verify_command(&d).unwrap().0, "dconf read '/org/x/y'");

        let s = BackDoorOp::RunSibling {
            tool: "convert".into(), input: "a".into(), output: "/tmp/out.png".into(), args: vec![] };
        let (cmd, expect) = verify_command(&s).unwrap();
        assert!(cmd.contains("test -e '/tmp/out.png'"));
        assert_eq!(expect, "LAGADO_OK");
    }

    #[test]
    fn readback_matching_normalizes_dialect_not_value() {
        // quote style + type prefix are dialect; the value itself must match exactly
        assert!(readback_matches("'prefer-dark'\n[exit 0]", "prefer-dark"));
        assert!(readback_matches("uint32 5", "5"));
        assert!(readback_matches("true", "true"));
        assert!(!readback_matches("'prefer-light'", "prefer-dark"));
        assert!(!readback_matches("", "prefer-dark"));
        assert!(!readback_matches("[exit 1]", "LAGADO_OK"));
        // an empty expectation can never auto-verify
        assert!(!readback_matches("anything", "  "));
    }
}
