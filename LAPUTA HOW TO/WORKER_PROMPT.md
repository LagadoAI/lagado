# WORKER PROMPT — read this file and do exactly what it says

> This file is the handoff channel. Whatever is below the line is the current task
> for the implementation worker (Code OSS). It is overwritten each time a new task
> is handed off. Read everything below `===` and execute it. Do not act on anything
> above the line.

===

TASK 6b — perception/arbiter.rs: mean-pool patch embeddings + deterministic output order

You are MODIFYING one existing file: lagado-agent/src/perception/arbiter.rs.
Two changes only. Do not change the public API signatures, the Sense enum, the
FusedElement struct, MATCH_THRESHOLD, the iou() function, the inflate() function,
steps 1 and 2 of fuse(), or any other file. Pure sync code.

READ FIRST: lagado-agent/src/perception/arbiter.rs (the whole file). Understand step 3
(embedding attachment) and the end of fuse() before editing.

── CHANGE 1: mean-pool overlapping spatial patches (replace single-patch pick) ──

Today step 3 picks the SINGLE highest-IoU spatial patch and attaches its embedding.
Replace this with mean-pooling over ALL overlapping spatial patches.

Exact new behavior for each element, in step 3:
- Keep the existing overview filter (C3) and the existing ±1 inflate (C2) EXACTLY as they are.
- For the element, iterate every spatial patch. A patch is "overlapping" iff
  iou(inflate(elem.bbox, patch.patch_w, patch.patch_h), patch_bbox) > 0.0
  (same membership test as before — just collect ALL overlapping patches, not the argmax).
- If zero patches overlap → leave patch_embd = None (unchanged).
- If one or more overlap → set patch_embd = Some(mean), where `mean` is the element-wise
  average of those patches' embd vectors:
      mean[d] = (sum over overlapping patches of patch.embd[d]) / (count of overlapping patches)
  All embd vectors are length n_embd (equal length); average dimension by dimension.
  Do NOT weight by IoU — a plain arithmetic mean, matching lagado_encode_image()'s mean-pool.

Implementation notes:
- Determine the embedding length from the first overlapping patch's embd.len() (do NOT hardcode
  any length or n_embd constant — C4 still applies).
- If overlapping patches somehow have mismatched lengths, skip the shorter/garbage ones rather
  than panic; never index past the end. (In practice they are always equal length.)

── CHANGE 2: deterministic output order ──

The returned Vec is currently ordered by HashMap iteration (non-deterministic per run).
At the VERY END of fuse(), AFTER step 3 completes, sort `elements` in place by the bbox
key (y, then x, then w, then h):

    elements.sort_by_key(|e| (e.bbox.1, e.bbox.0, e.bbox.2, e.bbox.3));

CRITICAL: this sort MUST be the last thing before `elements` is returned. Step 2 uses
indices into `elements` (best_a11y_idx) during the merge loop — sorting earlier would
corrupt those indices. Sort only at the end.

── TESTS: update + add (all must pass) ──

- Existing tests `embedding_attached_for_overlapping_spatial_patch` and
  `edge_fuzz_attaches_just_outside_raw_bbox` use a single overlapping patch — the mean of
  one vector is itself, so they should still pass unchanged. Confirm they do; do not weaken them.
- ADD `mean_pool_averages_multiple_overlapping_patches`: build a single spatial TilePatches whose
  `patches` vec contains TWO PatchEmbedding entries that both overlap the element (e.g. patch A at
  (0,0,27,25) embd [2.0,4.0], patch B at (20,0,27,25) embd [4.0,8.0]); fuse with one a11y box at
  (0,0,50,50); assert patch_embd == Some(vec![3.0, 6.0]) (element-wise average).
- ADD `deterministic_order_sorted_by_bbox`: build an a11y map with several boxes whose (y,x) order
  differs from insertion order; call fuse twice; assert the two result Vecs have identical bbox
  sequences AND that the sequence is sorted ascending by (y, x, w, h).
- Keep all other existing arbiter tests passing untouched.

── VERIFY BEFORE REPORTING DONE ──
  cargo check --workspace
  LD_LIBRARY_PATH=/home/d/laputa/lagado-agent/vendored/llama.cpp-2/build/bin cargo test -p lagado-agent arbiter

── REPORT ──
End with a "## TASK COMPLETE" report: files changed, test count delta, and an adversarial
self-review confirming: (a) mean-pool is a plain arithmetic mean over ALL overlapping patches with
no hardcoded length, (b) the sort is the final statement before return, (c) C2/C3/C4 and the
no-wildcard-_-on-Sense invariant are all still intact. Flag anything you'd raise reviewing cold.

DO NOT: change any other file, change the public API, touch vision/mod.rs / shim.c / cv_proposer.rs,
add async, or wire the arbiter into the agent loop (that is TASK 7).
