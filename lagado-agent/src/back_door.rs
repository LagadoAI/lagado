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
    fn quoting_contains_injection() {
        let op = BackDoorOp::RunSibling {
            tool: "convert".into(), input: "a; rm -rf ~".into(), output: "b".into(), args: vec![],
        };
        // the dangerous chars are quoted inside a single arg — the gate still decides whether it RUNS
        assert_eq!(to_command(&op).unwrap(), "'convert' 'a; rm -rf ~' 'b'");
    }
}
