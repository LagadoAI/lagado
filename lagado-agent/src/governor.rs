pub struct ServerConfig {
    pub ctx: usize,
    pub n_gpu_layers: u32,
    pub flash_attn: bool,
    pub threads: usize,
    pub n_parallel: usize,
}

pub fn detect_and_plan(default_ctx: usize) -> ServerConfig {
    let has_gpu = detect_gpu();
    // counts logical cores (includes hyper-threading) — refine later
    let threads = num_cores();
    if has_gpu {
        ServerConfig {
            ctx: default_ctx,
            n_gpu_layers: 99,
            flash_attn: true,
            threads,
            n_parallel: 4,
        }
    } else {
        let ctx = if available_ram_gb() < 12 { 8192 } else { default_ctx.min(16384) };
        ServerConfig {
            ctx,
            n_gpu_layers: 0,
            flash_attn: false,
            threads,
            n_parallel: 2,
        }
    }
}

fn detect_gpu() -> bool {
    std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

fn num_cores() -> usize {
    // counts logical cores (includes hyper-threading) — refine later
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
            return kb / 1_048_576; // kB → GB
        }
    }
    4
}
