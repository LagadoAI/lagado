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

// ── VRAM prediction (Option A — measured + calibrated, NO arch formula) ─────────
//
// Invariant #9 applied to the governor itself: we do NOT compute KV-cache from a
// transformer formula (LFM2 is a hybrid conv+attention arch; head_count_kv is absent —
// a formula would be an assumption). Instead we DISCOVER the model size (file_bytes,
// block_count) and MEASURE actual VRAM at a couple of contexts, then fit. KV cache is
// linear in sequence length (a real property), so `vram = base + slope·ctx` holds; the
// per-token slope is measured, not assumed. Weights re-scale by offloaded-layer fraction.

/// One observed (config → measured VRAM) point for a model on THIS machine. The
/// calibration that turns the governor from guessing into measuring.
#[derive(Debug, Clone, Copy)]
pub struct CalPoint {
    pub ctx: u32,
    pub n_gpu_layers: u32,
    pub measured_vram_mb: f32,
}

/// VRAM headroom policy (DEFER item — overridable). Leave 15% free for the display
/// compositor and transient spikes; not a model/hardware assumption, a safety policy.
pub const VRAM_HEADROOM_FRACTION: f32 = 0.85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feasibility {
    Green, // fits with headroom
    Amber, // fits but tight (eats the headroom)
    Red,   // predicted to exceed free VRAM → will OOM or spill
}

/// Least-squares linear fit → (intercept, slope). None if <2 points or no x-spread.
fn linear_fit(points: &[(f32, f32)]) -> Option<(f32, f32)> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f32;
    let sx: f32 = points.iter().map(|p| p.0).sum();
    let sy: f32 = points.iter().map(|p| p.1).sum();
    let sxx: f32 = points.iter().map(|p| p.0 * p.0).sum();
    let sxy: f32 = points.iter().map(|p| p.0 * p.1).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < f32::EPSILON {
        return None; // all the same ctx → can't separate slope from intercept
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    Some((intercept, slope))
}

/// Predict GPU VRAM (MiB) for a `(ctx, n_gpu_layers)` config from discovered model size +
/// measured calibration. Returns None on cold start (no usable calibration) — the caller
/// then falls back to safe ceilings rather than guessing. Honest by construction.
pub fn predict_vram_mb(
    file_bytes: u64,
    block_count: u32,
    n_gpu_layers: u32,
    ctx: u32,
    cal: &[CalPoint],
) -> Option<f32> {
    if block_count == 0 {
        return None;
    }
    // Fit from FULL-offload points so weights are fully on the GPU in the fit; we then
    // re-scale weights for partial offload.
    let full: Vec<(f32, f32)> = cal
        .iter()
        .filter(|p| p.n_gpu_layers >= block_count)
        .map(|p| (p.ctx as f32, p.measured_vram_mb))
        .collect();
    let (intercept, slope) = linear_fit(&full)?;
    let weights_full_mb = file_bytes as f32 / (1024.0 * 1024.0);
    let fixed_overhead = intercept - weights_full_mb; // non-weight, non-ctx VRAM
    let layer_frac = n_gpu_layers.min(block_count) as f32 / block_count as f32;
    Some(weights_full_mb * layer_frac + fixed_overhead + slope * ctx as f32)
}

/// Classify a predicted footprint against free VRAM.
pub fn feasibility(predicted_mb: f32, free_mb: f32) -> Feasibility {
    if free_mb <= 0.0 {
        return Feasibility::Red;
    }
    let ratio = predicted_mb / free_mb;
    if ratio <= VRAM_HEADROOM_FRACTION {
        Feasibility::Green
    } else if ratio <= 1.0 {
        Feasibility::Amber
    } else {
        Feasibility::Red
    }
}

// ── Engine planner (ModelInfo × hardware × user × calibration → plan) ───────────

use crate::gguf::ModelInfo;

/// Conservative context to probe on a COLD start (no calibration yet). Not an assumption
/// about the model (we read its real max = `context_length`); it's a safe starting point
/// we can't exceed without prediction. The first launch measures, then calibrated plans
/// grow it. DEFER policy, overridable.
const COLD_START_CTX: u32 = 4096;

/// User overrides from settings — each `None` means "let the governor decide".
#[derive(Debug, Clone, Default)]
pub struct EnginePrefs {
    pub ctx: Option<u32>,
    pub n_gpu_layers: Option<u32>,
    pub cpu_moe: Option<bool>,
}

/// A recommended (or override-validated) engine config, with the reasoning for the UI.
#[derive(Debug, Clone)]
pub struct EnginePlan {
    pub ctx: u32,
    pub n_gpu_layers: u32, // ≤ real block_count; 0 = CPU
    pub cpu_moe: bool,
    pub predicted_vram_mb: Option<f32>, // None when uncalibrated
    pub feasibility: Option<Feasibility>,
    pub rationale: String,
}

/// Produce an engine plan from DISCOVERED model facts × probed hardware × user prefs ×
/// measured calibration. Every number is read, measured, or user-chosen — never a
/// model/hardware assumption (invariant #9). Cold start (no calibration) is conservative
/// and honest; it becomes predictive once the runtime records calibration points.
pub fn plan_engine(
    model: &ModelInfo,
    gpu: Option<&GpuInfo>,
    prefs: &EnginePrefs,
    cal: &[CalPoint],
) -> EnginePlan {
    let model_max_ctx = model.context_length.unwrap_or(COLD_START_CTX as u64) as u32;
    let block_count = model.block_count.unwrap_or(0) as u32;

    // CPU-only.
    let Some(gpu) = gpu else {
        let ctx = prefs.ctx.unwrap_or(COLD_START_CTX).min(model_max_ctx);
        return EnginePlan {
            ctx,
            n_gpu_layers: 0,
            cpu_moe: false,
            predicted_vram_mb: None,
            feasibility: None,
            rationale: format!("No GPU detected → CPU inference. ctx {ctx} (model max {model_max_ctx})."),
        };
    };
    let free = gpu.vram_free_mb as f32;
    let weights_mb = model.file_bytes as f32 / (1024.0 * 1024.0);

    // Layers: honor an override, else all REAL layers (not -ngl 99). 0 block_count
    // (metadata missing) → fall back to the "all" sentinel so llama.cpp offloads all.
    let ngl = prefs
        .n_gpu_layers
        .unwrap_or(if block_count > 0 { block_count } else { 999 })
        .min(if block_count > 0 { block_count } else { 999 });
    let cpu_moe = prefs.cpu_moe.unwrap_or(false);

    // Context: honor an override (capped at the real model max), else recommend.
    let calibrated = predict_vram_mb(model.file_bytes, block_count.max(1), ngl, model_max_ctx, cal).is_some();

    let ctx = match prefs.ctx {
        Some(c) => c.min(model_max_ctx),
        None if calibrated => {
            // Largest ctx whose predicted footprint stays within the VRAM headroom.
            largest_fitting_ctx(model, gpu, ngl, cal).unwrap_or(COLD_START_CTX).min(model_max_ctx)
        }
        None => COLD_START_CTX.min(model_max_ctx), // cold: conservative, will grow after calibration
    };

    let predicted = predict_vram_mb(model.file_bytes, block_count.max(1), ngl, ctx, cal);
    let feas = predicted.map(|p| feasibility(p, free));

    let mut rationale = if calibrated {
        format!(
            "ctx {ctx}/{model_max_ctx}, {ngl}/{block_count} layers on GPU; predicted {:.0} MiB of {:.0} free",
            predicted.unwrap_or(0.0), free
        )
    } else {
        format!(
            "ctx {ctx}/{model_max_ctx} (cold start — conservative until first-launch calibration), {ngl}/{block_count} layers, weights ≈ {weights_mb:.0} MiB of {free:.0} free"
        )
    };
    if model.is_moe() {
        rationale.push_str(&format!(
            "; MoE ({} experts) → --cpu-moe available if VRAM is tight ({})",
            model.expert_count,
            if cpu_moe { "ON" } else { "off" }
        ));
    }

    EnginePlan { ctx, n_gpu_layers: ngl, cpu_moe, predicted_vram_mb: predicted, feasibility: feas, rationale }
}

/// Binary-search the largest ctx whose predicted footprint stays within the headroom.
fn largest_fitting_ctx(model: &ModelInfo, gpu: &GpuInfo, ngl: u32, cal: &[CalPoint]) -> Option<u32> {
    let max_ctx = model.context_length? as u32;
    let block_count = model.block_count.unwrap_or(0).max(1) as u32;
    let budget = gpu.vram_free_mb as f32 * VRAM_HEADROOM_FRACTION;
    let (mut lo, mut hi) = (256u32, max_ctx);
    let mut best = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let p = predict_vram_mb(model.file_bytes, block_count, ngl, mid, cal)?;
        if p <= budget {
            best = Some(mid);
            lo = mid + 256;
        } else {
            hi = mid.saturating_sub(256);
        }
    }
    best
}

// ── GPU detection ─────────────────────────────────────────────────────────────

/// Probe the best available discrete GPU (NVIDIA preferred, then AMD). Public so the UI
/// and other consumers can show real hardware without re-implementing detection.
pub fn detect_gpu() -> Option<GpuInfo> {
    detect_nvidia_gpu().or_else(detect_amd_gpu)
}

/// Logical CPU cores (public probe).
pub fn cpu_cores() -> usize {
    num_cores()
}

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

    // ── VRAM prediction (Option A) ──
    // Real 8B: file_bytes=5_044_779_712 (≈4811 MiB), block_count=24.
    // Point A is the REAL measurement (5074 MiB @ ctx 4096, full offload); Point B is
    // synthetic here until a second real measurement is gathered.
    const F8B: u64 = 5_044_779_712;

    #[test]
    fn predict_reproduces_a_calibration_point() {
        let cal = [
            CalPoint { ctx: 4096,  n_gpu_layers: 24, measured_vram_mb: 5074.0 },
            CalPoint { ctx: 16384, n_gpu_layers: 24, measured_vram_mb: 5800.0 },
        ];
        let p = predict_vram_mb(F8B, 24, 24, 4096, &cal).unwrap();
        assert!((p - 5074.0).abs() < 1.0, "should reproduce point A, got {p}");
    }

    #[test]
    fn predict_scales_down_with_fewer_layers() {
        let cal = [
            CalPoint { ctx: 4096,  n_gpu_layers: 24, measured_vram_mb: 5074.0 },
            CalPoint { ctx: 16384, n_gpu_layers: 24, measured_vram_mb: 5800.0 },
        ];
        let full = predict_vram_mb(F8B, 24, 24, 4096, &cal).unwrap();
        let half = predict_vram_mb(F8B, 24, 12, 4096, &cal).unwrap();
        assert!(half < full, "partial offload must predict less VRAM: {half} !< {full}");
    }

    #[test]
    fn predict_cold_start_is_none() {
        // No calibration → no guess. Caller falls back to safe ceilings.
        assert!(predict_vram_mb(F8B, 24, 24, 4096, &[]).is_none());
        let one = [CalPoint { ctx: 4096, n_gpu_layers: 24, measured_vram_mb: 5074.0 }];
        assert!(predict_vram_mb(F8B, 24, 24, 4096, &one).is_none()); // 1 point can't fit a slope
    }

    #[test]
    fn feasibility_levels() {
        assert_eq!(feasibility(4000.0, 5787.0), Feasibility::Green); // 0.69
        assert_eq!(feasibility(5074.0, 5787.0), Feasibility::Amber); // 0.88 — over headroom, still fits
        assert_eq!(feasibility(6000.0, 5787.0), Feasibility::Red);   // > free
    }

    // ── planner ──
    fn model_8b() -> ModelInfo {
        ModelInfo {
            arch: "lfm2moe".into(),
            context_length: Some(128000),
            block_count: Some(24),
            embedding_length: Some(2048),
            head_count: Some(32),
            head_count_kv: None,
            expert_count: 32,
            param_count: None,
            file_bytes: F8B,
        }
    }
    fn gpu_6gb() -> GpuInfo {
        GpuInfo { vendor: GpuVendor::Nvidia, vram_total_mb: 6144, vram_free_mb: 5787 }
    }
    fn cal_8b() -> Vec<CalPoint> {
        vec![
            CalPoint { ctx: 4096,  n_gpu_layers: 24, measured_vram_mb: 5074.0 },
            CalPoint { ctx: 16384, n_gpu_layers: 24, measured_vram_mb: 5800.0 },
        ]
    }

    #[test]
    fn plan_cpu_only_when_no_gpu() {
        let p = plan_engine(&model_8b(), None, &EnginePrefs::default(), &[]);
        assert_eq!(p.n_gpu_layers, 0);
        assert!(p.ctx <= 128000);
    }

    #[test]
    fn plan_cold_start_uses_real_layer_count_not_99() {
        let p = plan_engine(&model_8b(), Some(&gpu_6gb()), &EnginePrefs::default(), &[]);
        assert_eq!(p.n_gpu_layers, 24); // the REAL block_count, not the -ngl 99 hack
        assert_eq!(p.ctx, 4096); // conservative cold-start
        assert!(p.predicted_vram_mb.is_none()); // honest: no prediction without calibration
        assert!(p.rationale.contains("cold start"));
    }

    #[test]
    fn plan_calibrated_fits_within_headroom() {
        let p = plan_engine(&model_8b(), Some(&gpu_6gb()), &EnginePrefs::default(), &cal_8b());
        assert!(p.predicted_vram_mb.is_some());
        // On a 6 GB card a ~5 GB model leaves little KV room — the planner picks a ctx
        // that stays GREEN rather than over-committing. (Reveals the card is tight.)
        assert_eq!(p.feasibility, Some(Feasibility::Green));
        assert!(p.ctx >= 256 && p.ctx <= 128000);
    }

    #[test]
    fn plan_user_override_is_honored_and_flagged() {
        let prefs = EnginePrefs { ctx: Some(16384), ..Default::default() };
        let p = plan_engine(&model_8b(), Some(&gpu_6gb()), &prefs, &cal_8b());
        assert_eq!(p.ctx, 16384); // user's choice respected (advise, don't block)
        assert_eq!(p.feasibility, Some(Feasibility::Red)); // ...but flagged as won't-fit
    }

    #[test]
    fn plan_moe_offers_cpu_moe_in_rationale() {
        let p = plan_engine(&model_8b(), Some(&gpu_6gb()), &EnginePrefs::default(), &cal_8b());
        assert!(p.rationale.contains("MoE") && p.rationale.contains("cpu-moe"));
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
