//! sysinfo.rs — real machine facts for the UI (onboarding, settings).
//!
//! Invariant #9: the system specs a user sees are PROBED, never hardcoded. Linux probes
//! via /proc, /etc/os-release, and `df`; each falls back to "unknown"/0 rather than a
//! made-up value. (Cross-platform probes land with the mac/Win targets.)

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub cpu_model: String,
    pub physical_cores: usize,
    pub logical_threads: usize,
    pub ram_total_gb: f32,
    pub gpu_name: Option<String>,
    pub vram_total_mb: Option<u64>,
    pub vram_free_mb: Option<u64>,
    pub storage_free_gb: f32,
    pub storage_total_gb: f32,
    pub os: String,
}

pub fn probe() -> SystemInfo {
    let gpu = crate::governor::detect_gpu();
    let (storage_free_gb, storage_total_gb) = storage(&crate::config::data_dir()).unwrap_or((0.0, 0.0));
    SystemInfo {
        cpu_model: cpu_model().unwrap_or_else(|| "Unknown CPU".to_string()),
        physical_cores: physical_cores(),
        logical_threads: logical_threads(),
        ram_total_gb: ram_total_gb(),
        gpu_name: gpu_name(),
        vram_total_mb: gpu.as_ref().map(|g| g.vram_total_mb),
        vram_free_mb: gpu.as_ref().map(|g| g.vram_free_mb),
        storage_free_gb,
        storage_total_gb,
        os: os_name().unwrap_or_else(|| "Unknown OS".to_string()),
    }
}

fn cpu_model() -> Option<String> {
    let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    info.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
}

fn logical_threads() -> usize {
    std::fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0)
}

/// Unique (physical id, core id) pairs in /proc/cpuinfo → physical cores. Falls back to
/// threads/2 (hyperthreading guess) then threads.
fn physical_cores() -> usize {
    use std::collections::HashSet;
    if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
        let mut set: HashSet<(String, String)> = HashSet::new();
        let (mut phys, mut core) = (String::new(), String::new());
        for line in s.lines() {
            if let Some((k, v)) = line.split_once(':') {
                match k.trim() {
                    "physical id" => phys = v.trim().to_string(),
                    "core id" => core = v.trim().to_string(),
                    _ => {}
                }
            }
            if line.trim().is_empty() && !core.is_empty() {
                set.insert((phys.clone(), core.clone()));
                core.clear();
            }
        }
        if !set.is_empty() {
            return set.len();
        }
    }
    let t = logical_threads();
    if t > 1 { t / 2 } else { t }
}

fn ram_total_gb() -> f32 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|kb| kb.parse::<f64>().ok())
        })
        .map(|kb| (kb / 1024.0 / 1024.0) as f32)
        .unwrap_or(0.0)
}

fn gpu_name() -> Option<String> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// `(free_gib, total_gib)` for the filesystem holding `path`, via `df` (zero-dep).
fn storage(path: &Path) -> Option<(f32, f32)> {
    let out = Command::new("df")
        .args(["-B1", "--output=avail,size"])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().nth(1)?; // skip header row
    let mut it = line.split_whitespace();
    let avail: f64 = it.next()?.parse().ok()?;
    let total: f64 = it.next()?.parse().ok()?;
    let g = 1024f64.powi(3);
    Some(((avail / g) as f32, (total / g) as f32))
}

fn os_name() -> Option<String> {
    let s = std::fs::read_to_string("/etc/os-release").ok()?;
    s.lines()
        .find(|l| l.starts_with("PRETTY_NAME="))
        .map(|l| l.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string())
}
