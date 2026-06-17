# G3 Retrieval Eval — Results (2026-06-16)

The baseline that gates α/β/γ Board tuning (doctrine H-9 / G3). Set on the seeded
G3 fixture: 30 entries in 3 topic clusters (firefox/terminal/files) + noise, 6
labelled queries, K=15. Eval DB: `~/.laputa-secure/g3_eval.db`.

| Method | Mean P@15 | Mean R@15 | Mean F1 |
|---|---|---|---|
| **Jaccard** (retrieval.rs current) | 0.30 | 0.75 | 0.43 |
| **ColBERT-350M, mean pooling, cosine** | **0.37** | **0.92** | **0.52** |
| ColBERT-350M, cls pooling, cosine | 0.33 | 0.83 | 0.48 |

**Decision: mean-pooled ColBERT-350M cosine is the Board's relevance path.** It
clears the Jaccard floor where the headroom was — recall 0.75 → 0.92 — and fits the
existing `memory_tiers.embedding BLOB` + `find_similar_by_embedding` (cosine) infra.
Heavier late-interaction MaxSim is **deferred** (doctrine: the boring version earns
its keep; revisit only if ③ tuning shows relevance is too weak).

**Caveat to carry into ③ (Board tuning):** the pooled cosines are *compressed* —
~0.96–0.99 for nearly everything, including noise (ColBERT is trained for token-level
MaxSim, so single-vector pooling washes out discrimination). Consequences:
- The relevance term **ranks** adequately (recall 0.92) but **cannot threshold**, and
  mis-ranks noise high on abstract / lexically-distant queries ("what happened in the
  browser earlier" pulled noise above firefox).
- Mitigations: relevance is only 1 of 3 Park terms (recency + importance disambiguate);
  consider **rank-normalizing** the relevance term rather than using raw cosine; MaxSim
  is the future lever if relevance proves too weak after α/β/γ tuning.

**Precision ceiling note:** each topic has 6 relevant entries vs K=15, so precision is
structurally capped at 6/15 = 0.40. Read recall + ranking quality, not raw precision.

Reproduce:
```
# Jaccard floor:
python3 evals/eval_g3_retrieval.py --seed --db ~/.laputa-secure/g3_eval.db
python3 evals/eval_g3_retrieval.py --eval --db ~/.laputa-secure/g3_eval.db
# ColBERT cosine (needs embedding server on :8082, --pooling mean):
python3 evals/eval_g3_embed.py --db ~/.laputa-secure/g3_eval.db
```
