//! Resource containment for inference server and VM subprocesses.
//!
//! Two mechanisms:
//!   1. cgroup v2 memory + pid limits  — prevents runaway model from OOMing the host
//!   2. QEMU native `-sandbox` seccomp — applied via QEMU's own maintained filter
//!
//! All enforcement is Linux-only. The public API compiles on every platform and
//! degrades gracefully to no-ops on non-Linux or when cgroup setup fails.
//! All `#[cfg(target_os = "linux")]` lives only in this file.

/// Apply cgroup v2 memory and PID limits to a subprocess.
///
/// `name` is a short label ("llama", "classifier", "qemu") used in the leaf
/// cgroup path. `memory_max_bytes = 0` means unlimited. Errors are logged by
/// the caller; this function never panics.
pub fn apply_limits(pid: u32, name: &str, memory_max_bytes: u64, pids_max: u32) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::apply_limits(pid, name, memory_max_bytes, pids_max);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (pid, name, memory_max_bytes, pids_max);
        Ok(())
    }
}

/// Remove empty `lagado-*` cgroup leaves left over from a previous run.
/// Called once at app startup; best-effort, ignores errors.
pub fn cleanup_stale() {
    #[cfg(target_os = "linux")]
    linux::cleanup_stale();
}

/// Extra QEMU command-line arguments that enable QEMU's built-in seccomp sandbox.
/// Returns an empty slice on non-Linux (QEMU's `-sandbox` requires Linux seccomp).
pub fn qemu_sandbox_args() -> &'static [&'static str] {
    #[cfg(target_os = "linux")]
    {
        &[
            "-sandbox",
            "on,obsolete=deny,elevateprivileges=deny,spawn=deny,resourcecontrol=deny",
        ]
    }
    #[cfg(not(target_os = "linux"))]
    { &[] }
}

// ── Linux implementation ──────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Locate the writable cgroup directory where we can create leaf cgroups for
    /// subprocesses. Parses `/proc/self/cgroup` to find our current cgroup, then
    /// returns its parent — i.e. the `app.slice/` or `user@UID.service/` level
    /// where the user has delegation rights.
    fn cgroup_parent() -> Option<PathBuf> {
        cgroup_parent_from(
            &fs::read_to_string("/proc/self/cgroup").ok()?,
        )
    }

    /// Pure inner function — takes the cgroup file contents as a string so it is
    /// unit-testable without touching the filesystem.
    fn cgroup_parent_from(cgroup_data: &str) -> Option<PathBuf> {
        let rel = cgroup_data
            .lines()
            .find(|l| l.starts_with("0::"))?
            .strip_prefix("0::")?
            .trim_end_matches('\n')
            .trim_start_matches('/');
        // Parent of our scope — where delegation permits subdirectory creation
        PathBuf::from("/sys/fs/cgroup")
            .join(rel)
            .parent()
            .map(Path::to_path_buf)
    }

    pub fn apply_limits(
        pid: u32,
        name: &str,
        memory_max_bytes: u64,
        pids_max: u32,
    ) -> Result<(), String> {
        let parent = cgroup_parent().ok_or("could not locate delegated cgroup parent")?;
        let leaf = parent.join(format!("lagado-{name}-{pid}"));

        fs::create_dir_all(&leaf)
            .map_err(|e| format!("cgroup mkdir {}: {e}", leaf.display()))?;

        // memory.max: 0 would mean nothing; use "max" string to signal unlimited
        let mem_str = if memory_max_bytes == 0 {
            "max".to_string()
        } else {
            memory_max_bytes.to_string()
        };
        fs::write(leaf.join("memory.max"), &mem_str)
            .map_err(|e| format!("memory.max: {e}"))?;

        fs::write(leaf.join("pids.max"), pids_max.to_string())
            .map_err(|e| format!("pids.max: {e}"))?;

        fs::write(leaf.join("cgroup.procs"), pid.to_string())
            .map_err(|e| format!("cgroup.procs: {e}"))?;

        tracing::info!(
            "sandbox: pid {pid} ({name}) → {} [mem≤{}MiB pids≤{pids_max}]",
            leaf.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            if memory_max_bytes == 0 { "∞".to_string() } else {
                (memory_max_bytes / (1024 * 1024)).to_string()
            },
        );
        Ok(())
    }

    pub fn cleanup_stale() {
        let parent = match cgroup_parent() {
            Some(p) => p,
            None => return,
        };
        let entries = match fs::read_dir(&parent) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_lagado = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("lagado-"))
                .unwrap_or(false);
            if !is_lagado {
                continue;
            }
            if let Ok(procs) = fs::read_to_string(path.join("cgroup.procs")) {
                if procs.trim().is_empty() {
                    if let Err(e) = fs::remove_dir(&path) {
                        tracing::debug!(
                            "cleanup_stale: rmdir {:?}: {e}",
                            path.file_name().unwrap_or_default()
                        );
                    } else {
                        tracing::info!(
                            "sandbox: removed stale cgroup {:?}",
                            path.file_name().unwrap_or_default()
                        );
                    }
                }
            }
        }
    }

    // ── Tests for the pure path-parsing function ──────────────────────────────

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_standard_scope_path() {
            let data = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-lagado-1234.scope\n";
            let parent = cgroup_parent_from(data).unwrap();
            assert_eq!(
                parent,
                PathBuf::from(
                    "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice"
                )
            );
        }

        #[test]
        fn parses_service_level_path() {
            let data = "0::/user.slice/user-1000.slice/user@1000.service\n";
            let parent = cgroup_parent_from(data).unwrap();
            assert_eq!(
                parent,
                PathBuf::from("/sys/fs/cgroup/user.slice/user-1000.slice")
            );
        }

        #[test]
        fn ignores_cgroup_v1_lines() {
            let data = "12:cpu,cpuacct:/user.slice\n1:memory:/\n0::/user.slice/user-1000.slice/user@1000.service/session.slice\n";
            let parent = cgroup_parent_from(data).unwrap();
            assert_eq!(
                parent,
                PathBuf::from(
                    "/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service"
                )
            );
        }

        #[test]
        fn returns_none_when_no_v2_line() {
            let data = "1:memory:/user.slice\n12:cpu,cpuacct:/user.slice\n";
            assert!(cgroup_parent_from(data).is_none());
        }

        #[test]
        fn leaf_name_format() {
            // Confirm the naming pattern used by apply_limits
            let leaf_name = format!("lagado-{}-{}", "llama", 42u32);
            assert!(leaf_name.starts_with("lagado-"));
            assert!(leaf_name.ends_with("-42"));
        }
    }
}
