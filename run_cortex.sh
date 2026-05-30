#!/bin/bash
export LD_LIBRARY_PATH="$HOME/laputa/bin:$LD_LIBRARY_PATH"

$HOME/laputa/bin/llama-server --host 0.0.0.0 \
  -m ~/laputa/models/Qwen3-8B-ShiningValiant3.IQ4_XS.gguf \
  -ngl 99 \
  --ctx-size 16384 \
  --cache-type-k q4_0 --cache-type-v q4_0 \
  --mlock --no-mmap \
  --threads 8 \
  --batch-size 256 \
  --port 8080 \
  --flash-attn auto
