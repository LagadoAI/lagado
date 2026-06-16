# Lagado Phase 0 Evals

Run these after the code changes from 2026-06-16 are verified.

## Prerequisites

### 1. Install Rust (not yet installed on rebuilt Fedora 44)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 2. Verify the code builds

```bash
cd ~/projects/lagado
LD_LIBRARY_PATH=~/projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin \
  cargo check --workspace
LD_LIBRARY_PATH=~/projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin \
  cargo test -p lagado-agent
```

---

## Eval 1 — Tool-routing compounding (eval_tool_routing.py)

**What it measures:** single-turn vs multi-turn accuracy degradation on the
LFM2.5-1.2B-Instruct classifier. Turns the borrowed 0.63^5 ≈ 10% figure into
a number we actually own from our real prompt + checkpoint.

### Start the classifier server

```bash
export LLAMA=~/projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server
export LD_LIBRARY_PATH=~/projects/lagado/lagado-agent/vendored/llama.cpp-2/build/bin

$LLAMA \
  --model ~/.laputa-secure/models/LFM2.5-1.2B-Instruct-Q4_K_M.gguf \
  --port 8081 --host 127.0.0.1 --ctx-size 512 --no-mmap &
```

### Run

```bash
python3 evals/eval_tool_routing.py
# or with custom turns:
python3 evals/eval_tool_routing.py --turns 5
```

**What to do with the numbers:** the single-turn accuracy is the baseline.
The per-turn drop curve tells you how aggressively the Board needs to isolate
each classifier call. If accuracy drops sharply by turn 2, single-turn-fresh
is non-negotiable. If it's flat, it validates the discipline empirically.

---

## Eval 2 — G3 retrieval baseline (eval_g3_retrieval.py)

**What it measures:** Precision@K / Recall@K / F1 for the current Jaccard-based
retrieval in retrieval.rs. This is the G3 baseline required before tuning
α/β/γ Park-score weights.

### Seed test memories (one time only)

```bash
# Lagado must have been run at least once to create memory.db
LAGADO_DATA_DIR=~/.laputa-secure python3 evals/eval_g3_retrieval.py --seed
```

### Run

```bash
LAGADO_DATA_DIR=~/.laputa-secure python3 evals/eval_g3_retrieval.py --eval
# or with custom K:
LAGADO_DATA_DIR=~/.laputa-secure python3 evals/eval_g3_retrieval.py --eval --k 15
```

**What to do with the numbers:** this is the Jaccard floor. When the Board
retriever (Park score) is implemented, it should exceed this F1 meaningfully.
If it doesn't, the Board complexity isn't earning its keep.
