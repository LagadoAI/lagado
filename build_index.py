#!/usr/bin/env python3
"""Build FAISS index from vault facts and chunks using local MiniLM embeddings.

Indexes:
  - Facts (JSON files in vault/facts/) by their "fact" field.
  - Chunks (.zst files in vault/chunks/) by their DECOMPRESSED content (first 512 chars).

CRITICAL FIX (May 2026): Previously chunks were indexed by the literal string
"Literal chunk <id>", which produced semantically useless embeddings. Now we
decompress each chunk and use its actual text content for the embedding.
"""
import json, os, sys
import numpy as np
import faiss
import zstandard as zstd
from sentence_transformers import SentenceTransformer

VAULT       = os.path.expanduser("~/laputa/vault")
FACT_DIR    = os.path.join(VAULT, "facts")
CHUNK_DIR   = os.path.join(VAULT, "chunks")
INDEX_PATH  = os.path.join(VAULT, "vault_index.faiss")
META_PATH   = os.path.join(VAULT, "vault_meta.json")

# Max characters of chunk text used for embedding (matches model's effective ctx).
# Full chunk is still preserved in compressed storage; this is purely for the
# embedding signature.
EMBED_TEXT_CHARS = 512

print("Loading MiniLM embedding model (80 MB, cached after first download)...")
model = SentenceTransformer("all-MiniLM-L6-v2")

print("Building vault index...")
texts = []
metas = []

# ── 1. Index facts ─────────────────────────────────────────────────────────
if os.path.isdir(FACT_DIR):
    for fname in sorted(os.listdir(FACT_DIR)):
        if not fname.endswith('.json'):
            continue
        try:
            with open(os.path.join(FACT_DIR, fname)) as f:
                data = json.load(f)
            fact_text = data.get("fact", "").strip()
            if not fact_text:
                continue
            texts.append(fact_text)
            metas.append({"type": "fact", "id": data.get("id", fname.replace('.json', ''))})
        except Exception as e:
            print(f"  ! Skipping fact {fname}: {e}", file=sys.stderr)

# ── 2. Index chunks (decompress for embedding signature) ───────────────────
# One decompressor instance, reused across all files.
dctx = zstd.ZstdDecompressor()
chunks_indexed = 0
chunks_skipped = 0

if os.path.isdir(CHUNK_DIR):
    for fname in sorted(os.listdir(CHUNK_DIR)):
        if not fname.endswith('.zst'):
            continue
        chunk_id = fname.replace('.zst', '')
        chunk_path = os.path.join(CHUNK_DIR, fname)

        # Skip empty files outright
        try:
            if os.path.getsize(chunk_path) == 0:
                print(f"  ! Skipping empty chunk {fname}", file=sys.stderr)
                chunks_skipped += 1
                continue
        except OSError as e:
            print(f"  ! Cannot stat chunk {fname}: {e}", file=sys.stderr)
            chunks_skipped += 1
            continue

        # Decompress + decode safely
        try:
            with open(chunk_path, "rb") as f:
                compressed = f.read()
            raw = dctx.decompress(compressed)
            text = raw.decode("utf-8", errors="ignore").strip()
        except zstd.ZstdError as e:
            print(f"  ! Corrupt chunk {fname}: {e}", file=sys.stderr)
            chunks_skipped += 1
            continue
        except Exception as e:
            print(f"  ! Failed to read chunk {fname}: {e}", file=sys.stderr)
            chunks_skipped += 1
            continue

        if not text:
            print(f"  ! Empty content in {fname}", file=sys.stderr)
            chunks_skipped += 1
            continue

        # Use first EMBED_TEXT_CHARS for the embedding signature
        texts.append(text[:EMBED_TEXT_CHARS])
        metas.append({"type": "chunk", "id": chunk_id, "file": fname})
        chunks_indexed += 1

print(f"  → Facts: {sum(1 for m in metas if m['type'] == 'fact')}")
print(f"  → Chunks indexed: {chunks_indexed}, skipped: {chunks_skipped}")

if not texts:
    print("No content to index. Vault is empty.")
    sys.exit(0)

# ── 3. Generate embeddings ─────────────────────────────────────────────────
print(f"Embedding {len(texts)} items...")
embeddings = model.encode(texts, normalize_embeddings=True, show_progress_bar=True)
embeddings = np.array(embeddings, dtype=np.float32)

# ── 4. Build & save FAISS index ────────────────────────────────────────────
dim   = embeddings.shape[1]
index = faiss.IndexFlatIP(dim)  # inner product on normalized vectors = cosine similarity
index.add(embeddings)

faiss.write_index(index, INDEX_PATH)
with open(META_PATH, "w") as f:
    json.dump(metas, f, indent=2)

print(f"✓ Index saved: {INDEX_PATH} ({index.ntotal} vectors, {dim} dims)")
print(f"✓ Metadata:    {META_PATH}")
