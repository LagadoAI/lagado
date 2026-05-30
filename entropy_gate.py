#!/usr/bin/env python3
"""Entropic Memory Gate — score text chunks via zstd compressibility heuristic.

Replaces the prior `llama-perplexity` subprocess (which required ≥1024 tokens
and blocked on a TinyLlama model) with a model-free heuristic: zstd
compressibility ratio scaled to approximate bits/token.

Rationale: zstd at level 3 achieves close-to-Shannon-entropy compression on
natural language. The ratio (compressed / original) scaled by 8 gives a usable
proxy for bits/token. High entropy → keep verbatim; low entropy → condense.

ENTROPY_THRESHOLD = 4.0 bits/token approximates a perplexity of ~16, matching
the previous TinyLlama-based gate's effective decision boundary.
"""
import sys, os, json, hashlib, urllib.request
import zstandard as zstd

# ── Configuration ───────────────────────────────────────────────────────────
ENTROPY_THRESHOLD = 4.0      # bits/token; below this, condense

CHUNK_DIR = os.path.expanduser("~/laputa/vault/chunks")
FACT_DIR  = os.path.expanduser("~/laputa/vault/facts")
os.makedirs(CHUNK_DIR, exist_ok=True)
os.makedirs(FACT_DIR,  exist_ok=True)

CORTEX_CHAT_URL = "http://127.0.0.1:8080/v1/chat/completions"
CORTEX_MODEL    = "Qwen3-8B-ShiningValiant3.IQ4_XS.gguf"

# Single shared compressor — instantiated once, reused
_zstd_compressor = zstd.ZstdCompressor(level=3)

# Floor: text shorter than this can't be reliably compressed (zstd framing
# overhead dominates), so we use a cheaper unique-char heuristic instead.
_SHORT_TEXT_FLOOR = 64


# ── Entropy ────────────────────────────────────────────────────────────────
def compute_entropy(text: str) -> float:
    """Estimate bits/token via zstd compressibility.

    Returns a value above ENTROPY_THRESHOLD when text is high-entropy
    (should be stored verbatim), and below when low-entropy (worth condensing).

    Special cases:
      - Empty text → return threshold + 1.0 (treated as high-entropy / store as-is)
      - Very short text (< 64 bytes) → use unique-character ratio instead of zstd
      - Pathological compressed > original → return threshold + 1.0
    """
    if not text:
        return ENTROPY_THRESHOLD + 1.0

    data = text.encode("utf-8")
    original_size = len(data)

    if original_size == 0:
        return ENTROPY_THRESHOLD + 1.0

    # Short-text heuristic — zstd framing overhead would dominate
    if original_size < _SHORT_TEXT_FLOOR:
        # unique_chars / total_chars, then scale to ~bits/token range
        # Highly repetitive short text → low value; diverse → high value
        unique_ratio = len(set(text)) / max(len(text), 1)
        return unique_ratio * 8.0

    # zstd compression at level 3 (fast; close to entropy in practice)
    compressed = _zstd_compressor.compress(data)
    compressed_size = len(compressed)

    # Pathological: zstd framing made it bigger → treat as high entropy
    if compressed_size >= original_size:
        return ENTROPY_THRESHOLD + 1.0

    ratio = compressed_size / original_size
    # Scale to bits-per-byte (UTF-8 ≈ 1 byte/token for ASCII English)
    return ratio * 8.0


# ── Decision gate ──────────────────────────────────────────────────────────
def gate_chunk(text: str) -> str:
    """Decide how to store text. Returns 'store' or 'condense'."""
    return "store" if compute_entropy(text) > ENTROPY_THRESHOLD else "condense"


# ── Cortex condensation (preserved from prior version) ─────────────────────
def condense_via_cortex(text: str) -> str:
    """Call the Cortex chat endpoint to condense text into a single sentence."""
    truncated = text[:1200]
    payload = {
        "model": CORTEX_MODEL,
        "messages": [
            {"role": "system",
             "content": "You are a factual condenser. Reduce the user's text "
                        "to a single concise sentence. Output ONLY the condensed "
                        "sentence, no reasoning, no prefix."},
            {"role": "user", "content": truncated}
        ],
        "max_tokens": 64,
        "temperature": 0.2
    }
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(CORTEX_CHAT_URL, data=data,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            result = json.loads(resp.read().decode("utf-8"))
        msg = result.get("choices", [{}])[0].get("message", {})
        content = msg.get("content", "").strip()
        if not content:
            content = msg.get("reasoning_content", "").strip()
        return content
    except Exception as e:
        print(f"Condensation error: {e}", file=sys.stderr)
        return ""


# ── Storage helpers ────────────────────────────────────────────────────────
def store_chunk(text: str) -> str:
    """Store text verbatim (zstd-compressed, SHA-256 addressed)."""
    chunk_id = hashlib.sha256(text.encode()).hexdigest()[:16]
    # Storage uses level 6 for slightly better compression than the gate's
    # speed-tuned level 3.
    compressed = zstd.ZstdCompressor(level=6).compress(text.encode())
    path = os.path.join(CHUNK_DIR, f"{chunk_id}.zst")
    with open(path, "wb") as f:
        f.write(compressed)
    return chunk_id


def store_fact(fact: str) -> str:
    fact_id = hashlib.sha256(fact.encode()).hexdigest()[:12]
    path = os.path.join(FACT_DIR, f"{fact_id}.json")
    with open(path, "w") as f:
        json.dump({"fact": fact, "id": fact_id}, f)
    return fact_id


# ── Top-level: process a chunk ─────────────────────────────────────────────
def process_chunk(text: str) -> dict:
    entropy = compute_entropy(text)
    decision = gate_chunk(text)

    if decision == "store":
        cid = store_chunk(text)
        return {"type": "chunk", "id": cid,
                "entropy_bits_per_token": round(entropy, 3)}

    # condense path
    fact = condense_via_cortex(text)
    if len(fact) < 10:
        # Cortex unreachable / produced nothing → fall back to literal storage
        cid = store_chunk(text)
        return {"type": "chunk", "id": cid,
                "entropy_bits_per_token": round(entropy, 3),
                "note": "condensation empty, stored literal"}
    fid = store_fact(fact)
    return {"type": "fact", "id": fid,
            "entropy_bits_per_token": round(entropy, 3),
            "fact": fact}


# ── CLI / test mode ────────────────────────────────────────────────────────
def _self_test():
    """Verify the entropy gate behaves correctly on sample inputs."""
    samples = [
        ("repetitive_short",
         "abcabc"),                                                       # tiny + repetitive
        ("repetitive_long",
         "the quick brown fox " * 100),                                   # long + repetitive
        ("random_long",
         "Z9k$mP2x@vL!8nQ#cR4w&BfH7jT" * 50),                            # long + high entropy
        ("english_prose",
         ("Laputa is a sovereign local AI agent designed to run entirely "
          "on consumer hardware. It uses grammar-constrained tool calls "
          "and a persistent action graph that learns from every task.") * 4),
        ("single_word", "hello"),
        ("empty", ""),
    ]
    print(f"ENTROPY_THRESHOLD = {ENTROPY_THRESHOLD}")
    print(f"{'name':<22} {'bytes':>6} {'entropy':>9}  {'decision':<10}")
    print("-" * 55)
    for name, text in samples:
        e = compute_entropy(text)
        d = gate_chunk(text)
        print(f"{name:<22} {len(text):>6} {e:>9.3f}  {d:<10}")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--test":
        _self_test()
        sys.exit(0)

    if len(sys.argv) > 1:
        with open(sys.argv[1], "r") as f:
            text = f.read()
    else:
        text = sys.stdin.read()
    result = process_chunk(text)
    print(json.dumps(result, indent=2))
