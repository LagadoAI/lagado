#!/usr/bin/env bash
set -euo pipefail

LLAMA_SERVER="/home/d/laputa/laputa-agent/vendored/llama.cpp-2/build/bin/llama-server"
MODEL="/home/d/.laputa-secure/models/LFM2.5-8B-A1B-Q4_K_M.gguf"
CTX="${LAPUTA_CTX:-32768}"

if [ ! -x "$LLAMA_SERVER" ]; then
  echo "ERROR: llama-server not found/executable at $LLAMA_SERVER" >&2
  exit 1
fi
if [ ! -f "$MODEL" ]; then
  echo "ERROR: model not found at $MODEL" >&2
  exit 1
fi

"$LLAMA_SERVER" \
  --model "$MODEL" \
  --port 8080 \
  --ctx-size "$CTX" \
  --cache-type-k q8_0 \
  --cache-type-v q8_0 \
  --n-gpu-layers 99 \
  --flash-attn on \
  --jinja \
  --temp 0.2 \
  --top-k 80 \
  --repeat-penalty 1.05
