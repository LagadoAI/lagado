#!/usr/bin/env python3
"""
eval_g3_embed.py — G3 retrieval eval using LFM2-ColBERT-350M embeddings (cosine),
to compare against the Jaccard floor from eval_g3_retrieval.py.

Pooled-vector cosine (NOT late-interaction MaxSim) — the path that matches the
existing memory_tiers `embedding BLOB` + find_similar_by_embedding cosine infra.
If this beats Jaccard meaningfully, that's the Board's embedding path; only if it
disappoints do we consider heavier late-interaction.

Prereq: ColBERT embedding server on :8082
  llama-server --model ~/.laputa-secure/models/LFM2-ColBERT-350M-Q4_K_M.gguf \
    --port 8082 --host 127.0.0.1 -ngl 0 --embedding --pooling mean

Usage: python3 evals/eval_g3_embed.py --db ~/.laputa-secure/g3_eval.db
"""
import argparse, json, math, os, sqlite3, sys, urllib.request

from eval_g3_retrieval import EVAL_QUERIES  # single source of truth for queries

EMBED_URL = "http://127.0.0.1:8082/v1/embeddings"


def embed_batch(texts):
    """Embed a list of texts in one request; returns list of vectors."""
    body = json.dumps({"input": texts}).encode()
    req = urllib.request.Request(EMBED_URL, data=body,
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        data = json.loads(r.read())
    # preserve input order via 'index'
    rows = sorted(data["data"], key=lambda d: d["index"])
    return [row["embedding"] for row in rows]


def cosine(a, b):
    dot = sum(x * y for x, y in zip(a, b))
    na = math.sqrt(sum(x * x for x in a))
    nb = math.sqrt(sum(y * y for y in b))
    return dot / (na * nb) if na and nb else 0.0


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--db", default=os.path.expanduser("~/.laputa-secure/g3_eval.db"))
    p.add_argument("--k", type=int, default=15)
    args = p.parse_args()

    map_path = os.path.join(os.path.dirname(__file__), "g3_seed_ids.json")
    if not os.path.exists(map_path):
        print("ERROR: run eval_g3_retrieval.py --seed first", file=sys.stderr); sys.exit(1)
    with open(map_path) as f:
        id_to_topic = json.load(f)

    con = sqlite3.connect(args.db)
    rows = con.execute("SELECT id, text, tier FROM memory_entries").fetchall()
    con.close()

    ids   = [r[0] for r in rows]
    texts = [r[1] for r in rows]
    tiers = {r[0]: r[2] for r in rows}

    print(f"Embedding {len(texts)} entries + {len(EVAL_QUERIES)} queries via ColBERT-350M (cosine)...")
    entry_vecs = embed_batch(texts)
    query_vecs = embed_batch([q for q, _ in EVAL_QUERIES])
    id_to_vec = dict(zip(ids, entry_vecs))

    k = args.k
    tot_p = tot_r = 0.0
    print(f"\nG3 retrieval eval (ColBERT cosine) — K={k}, entries={len(texts)}\n")
    for (query, relevant_topic), qv in zip(EVAL_QUERIES, query_vecs):
        scored = sorted(((cosine(qv, id_to_vec[i]), i) for i in ids),
                        key=lambda x: x[0], reverse=True)
        top_k = scored[:k]
        seed_relevant = {i for i, t in id_to_topic.items() if t == relevant_topic}
        retrieved = {i for _, i in top_k}
        tp = len(retrieved & seed_relevant)
        prec = tp / k if k else 0.0
        rec  = tp / len(seed_relevant) if seed_relevant else 0.0
        f1   = (2 * prec * rec / (prec + rec)) if (prec + rec) else 0.0
        tot_p += prec; tot_r += rec
        print(f"Query: '{query}'  (relevant: {relevant_topic})")
        print(f"  Precision@{k}: {prec:.2f}  Recall@{k}: {rec:.2f}  F1: {f1:.2f}")
        for score, i in top_k[:3]:
            topic = id_to_topic.get(i, "other")
            mark = "✓" if i in seed_relevant else "✗"
            txt = next(t for (eid, t) in zip(ids, texts) if eid == i)
            print(f"    {mark} [{tiers[i]:4}][{topic:8}] cos={score:.3f}  '{txt[:58]}'")
        print()

    n = len(EVAL_QUERIES)
    mp, mr = tot_p / n, tot_r / n
    mf = (2 * mp * mr / (mp + mr)) if (mp + mr) else 0.0
    print(f"=== Mean across {n} queries (ColBERT cosine) ===")
    print(f"  Mean Precision@{k}: {mp:.2f}")
    print(f"  Mean Recall@{k}:    {mr:.2f}")
    print(f"  Mean F1:            {mf:.2f}")
    print(f"\n  Jaccard floor was: P=0.30  R=0.75  F1=0.43")


if __name__ == "__main__":
    main()
