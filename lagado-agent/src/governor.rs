use std::process::Command;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
}

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
}

/// Everything `ensure_llama_server` needs to build the right command line.
///
/// `gpu` is Some even when `n_gpu_layers == 0` — it means a GPU was found but
/// the model is too large to fully offload. Future code can use `gpu` to decide
/// on a MoE split strategy without re-probing hardware.
///
/// `moe_experts_on_cpu`: when true, pass `--cpu-moe` to llama-server. This keeps
/// all Mixture-of-Experts weights on the CPU while attention/embedding layers stay
/// on the GPU — the correct strategy for MoE models that exceed VRAM. Currently
/// always false (requires GGUF-aware detection in Phase 3.x); the field is wired
/// through to bootstrap.rs so that detection is the only Phase 3.x change needed.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub ctx: usize,
    pub n_gpu_layers: u32,
    pub flash_attn: bool,
    pub threads: usize,
    pub n_parallel: usize,
    pub gpu: Option<GpuInfo>,
    pub moe_experts_on_cpu: bool,
}

impl ServerConfig {
    /// Fraction of model that could fit in free VRAM. 0.0 on CPU-only systems.
    /// Phase 3.x can use this alongside GGUF metadata to choose a split policy.
    pub fn vram_fit_fraction(&self, model_bytes: u64) -> f32 {
        match &self.gpu {
            Some(info) if model_bytes > 0 => {
                let vram_bytes = info.vram_free_mb * 1024 * 1024;
                (vram_bytes as f32 / model_bytes as f32).min(1.0)
            }
            _ => 0.0,
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Detect hardware and produce a `ServerConfig` for the given model.
///
/// `model_bytes` is the model file size — a reliable proxy for its RAM/VRAM
/// footprint (GGUF ≈ quantized weight bytes). Pass 0 if unknown; the governor
/// will assume full GPU offload when a GPU is present.
pub fn detect_and_plan(default_ctx: usize, model_bytes: u64) -> ServerConfig {
    let threads = num_cores();
    let gpu = detect_nvidia_gpu().or_else(detect_amd_gpu);

    match gpu {
        Some(ref info) => {
            let (n_gpu_layers, moe_experts_on_cpu) =
                compute_offload(info.vram_free_mb, model_bytes);

            if n_gpu_layers > 0 {
                tracing::info!(
                    "GPU: {:?} vram_free={}MiB model={}MiB → full offload",
                    info.vendor,
                    info.vram_free_mb,
                    model_bytes / (1024 * 1024),
                );
                ServerConfig {
                    ctx: default_ctx,
                    n_gpu_layers,
                    flash_attn: true,
                    threads,
                    n_parallel: 4,
                    gpu: Some(info.clone()),
                    moe_experts_on_cpu,
                }
            } else {
                tracing::info!(
                    "GPU: {:?} vram_free={}MiB < model {}MiB — falling back to CPU",
                    info.vendor,
                    info.vram_free_mb,
                    model_bytes / (1024 * 1024),
                );
                cpu_config(default_ctx, threads, Some(info.clone()))
            }
        }
        None => {
            tracing::info!("No dedicated GPU detected — CPU inference");
            cpu_config(default_ctx, threads, None)
        }
    }
}

// ── Decision logic (pure — unit-testable) ─────────────────────────────────────

/// Returns `(n_gpu_layers, moe_experts_on_cpu)` given available VRAM and model size.
///
/// Conservative binary policy: require 10% VRAM headroom above the full model
/// size before committing to GPU offload. This avoids the partial-layer OOM that
/// would occur if we estimated layers without reading GGUF `block_count` metadata.
///
/// Partial offload (`0 < n_gpu_layers < 99`) is reserved for Phase 3.x when GGUF
/// parsing provides an authoritative layer count. For now: full GPU or CPU only.
///
/// `moe_experts_on_cpu` is always false here; Phase 3.x's GGUF detector flips it
/// when it recognises a MoE architecture (`expert_count > 1` in metadata).
fn compute_offload(vram_free_mb: u64, model_bytes: u64) -> (u32, bool) {
    if model_bytes == 0 {
        // Unknown model size — assume it fits; llama.cpp will error if not
        return (99, false);
    }
    let vram_free_bytes = vram_free_mb * 1024 * 1024;
    let fits = (vram_free_bytes as f64) >= (model_bytes as f64) * 1.1;
    if fits { (99, false) } else { (0, false) }
}

fn cpu_config(default_ctx: usize, threads: usize, gpu: Option<GpuInfo>) -> ServerConfig {
    let ctx = if available_ram_gb() < 12 {
        8192
    } else {
        default_ctx.min(16384)
    };
    ServerConfig {
        ctx,
        n_gpu_layers: 0,
        flash_attn: false,
        threads,
        n_parallel: 2,
        gpu,
        moe_experts_on_cpu: false,
    }
}

// ── GPU detection ─────────────────────────────────────────────────────────────

/// Detect the NVIDIA GPU with the most free VRAM via `nvidia-smi`.
/// On multi-GPU systems, picks the card with the most free VRAM.
fn detect_nvidia_gpu() -> Option<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.total,memory.free",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',');
            let total: u64 = parts.next()?.trim().parse().ok()?;
            let free: u64 = parts.next()?.trim().parse().ok()?;
            Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                vram_total_mb: total,
                vram_free_mb: free,
            })
        })
        .max_by_key(|g| g.vram_free_mb)
}

/// Detect an AMD discrete GPU via Linux DRM sysfs.
/// Skips integrated GPUs (vendor 0x8086 = Intel, no mem_info_vram_* files).
fn detect_amd_gpu() -> Option<GpuInfo> {
    for i in 0..8u8 {
        let base = format!("/sys/class/drm/card{i}/device");
        let vendor = match std::fs::read_to_string(format!("{base}/vendor")) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if vendor.trim() != "0x1002" {
            continue; // not AMD (0x1002)
        }
        let total_str = match std::fs::read_to_string(format!("{base}/mem_info_vram_total")) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let total_bytes: u64 = match total_str.trim().parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if total_bytes == 0 {
            continue;
        }
        let used_bytes: u64 = std::fs::read_to_string(format!("{base}/mem_info_vram_used"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        return Some(GpuInfo {
            vendor: GpuVendor::Amd,
            vram_total_mb: total_bytes / (1024 * 1024),
            vram_free_mb: total_bytes.saturating_sub(used_bytes) / (1024 * 1024),
        });
    }
    None
}

// ── System probes ─────────────────────────────────────────────────────────────

fn num_cores() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn available_ram_gb() -> u64 {
    let contents = match std::fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => return 4,
    };
    for line in contents.lines() {
        if line.starts_with("MemAvailable:") {
            let kb: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            return kb / 1_048_576;
        }
    }
    4
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_gpu_when_model_fits_with_margin() {
        // 10,000 MB free; 8 GiB model ≈ 8,192 MB. 10,000 > 8,192 × 1.1 ≈ 9,011 → fits
        let (ngl, moe) = compute_offload(10_000, 8 * 1024 * 1024 * 1024);
        assert_eq!(ngl, 99);
        assert!(!moe);
    }

    #[test]
    fn cpu_when_model_exceeds_vram() {
        // 4,000 MB free, 8 GiB model → 4,000 < 8,192 × 1.1 → CPU
        let (ngl, _) = compute_offload(4_000, 8 * 1024 * 1024 * 1024);
        assert_eq!(ngl, 0);
    }

    #[test]
    fn cpu_when_just_under_margin() {
        // Exactly at the model size but below the 1.1× margin: force CPU
        // 8,192 MB free, 8 GiB model: free < 8,192 × 1.1 = 9,011 → CPU
        let (ngl, _) = compute_offload(8_192, 8 * 1024 * 1024 * 1024);
        assert_eq!(ngl, 0);
    }

    #[test]
    fn zero_model_bytes_defers_to_full_gpu() {
        // Unknown model → assume full GPU (llama.cpp will error if it truly doesn't fit)
        let (ngl, _) = compute_offload(8_000, 0);
        assert_eq!(ngl, 99);
    }

    #[test]
    fn moe_flag_reserved_for_gguf_phase() {
        // moe_experts_on_cpu is always false from governor; Phase 3.x GGUF sets it
        let (_, moe) = compute_offload(32_000, 5 * 1024 * 1024 * 1024);
        assert!(!moe);
    }

    #[test]
    fn vram_fit_fraction_gpu_present() {
        let cfg = ServerConfig {
            ctx: 32768,
            n_gpu_layers: 99,
            flash_attn: true,
            threads: 8,
            n_parallel: 4,
            gpu: Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                vram_total_mb: 11264,
                vram_free_mb: 8192,
            }),
            moe_experts_on_cpu: false,
        };
        let frac = cfg.vram_fit_fraction(8 * 1024 * 1024 * 1024);
        // 8192 MB free / 8192 MB model ≈ 1.0
        assert!((frac - 1.0).abs() < 0.01);
    }

    #[test]
    fn vram_fit_fraction_no_gpu() {
        let cfg = ServerConfig {
            ctx: 8192, n_gpu_layers: 0, flash_attn: false,
            threads: 4, n_parallel: 2, gpu: None, moe_experts_on_cpu: false,
        };
        assert_eq!(cfg.vram_fit_fraction(5 * 1024 * 1024 * 1024), 0.0);
    }
}
