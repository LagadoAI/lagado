#!/usr/bin/env bash
# start_brain.sh — launch the OSWorld test brain (Qwen2.5-Coder-7B) on :8080 LEAN.
#
# WHY THIS EXISTS (2026-06-23): the default launch mmap'd the 4.7GB GGUF into host RAM even with full
# GPU offload (-ngl 99). With the weights already in VRAM, those host pages go cold and the kernel pushes
# ~7GB into zram → the 15Gi box sits at the edge of OOM and a 3G nested VM can't boot (it thrashes for
# >900s). `--no-mmap` reads the file, uploads to GPU, and frees the host copy (weights land in reclaimable
# page cache, not pinned anonymous swap) → ~7GB reclaimed, MemAvailable ~4.7Gi → ~11Gi. ALWAYS start the
# brain with this script (or add --no-mmap -c 2048 to whatever launcher you use) so the OOM cannot recur.
set -euo pipefail

LLAMA="${LAGADO_LLAMA_SERVER:-/home/alucard/projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server}"
MODEL="${LAGADO_BRAIN_MODEL:-/home/alucard/.laputa-secure/models/Qwen2.5-Coder-7B-Instruct-Q4_K_M.gguf}"
PORT="${LAGADO_BRAIN_PORT:-8080}"
CTX="${LAGADO_BRAIN_CTX:-4096}"   # 2026-07-03: the op-vocab EMIT prompt reached ~2k tokens (measured: 2035-token
                                  # prompt + 13 generated => truncated=1 at ctx 2048 -> mid-string emission).
                                  # 4096 restores headroom; q8_0 KV keeps the extra RAM/VRAM small.

echo "stopping any existing brain on :$PORT ..."
pkill -f "llama-server.*--port $PORT" 2>/dev/null || true
sleep 1

echo "launching lean brain: ctx=$CTX, --no-mmap, full GPU offload, single slot"
# --parallel 1: multi-slot continuous batching made temp-0 SAME-SEED outputs vary run-to-run
# (the 2026-06-23 variance finding; server logs showed 4 slots). One slot = sequential decode =
# reproducible draws, so best-of-N seed diversity is CONTROLLED diversity, not noise on noise.
exec "$LLAMA" -m "$MODEL" --port "$PORT" -c "$CTX" -ngl 99 --threads 8 -fa on \
     -ctk q8_0 -ctv q8_0 --no-mmap --parallel 1
