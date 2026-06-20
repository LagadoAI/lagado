# Comprehensive stress run — 2026-06-20 (autonomous)

Running while the user is away (~30–50 min). Goal: stress-test the agent in as many ways as possible,
wire in the validated ReAct loop, attempt real OSWorld/CUA tasks, fix bugs found, log everything.

Environment: Fedora 44 + Cinnamon guest VM on :2222; GPU 8B brain (LFM2-8B-A1B Q4) on :8080.

## Plan
1. Wire the validated ReAct command loop (observe→reason→act→deterministic-verify, 6/6 in probe) into the
   production agent_loop. Re-bench user_stress END-TO-END (everything derived, nothing hardcoded).
2. Expanded stress batteries: complexity, traps, destructive-confirm, large file sets, content
   extraction, nested ops, mixed file+GUI. Run + record.
3. Real OSWorld task ingestion (subagent, read-only): extract CLI/file-amenable task defs, run the subset.
4. Log all bugs + fixes + data below.

## Log

### ReAct loop wired into production (agent.rs) — `discover_environment` / `derive_expected` / `react_next_command` + the loop branch before `loop {`. Compiles clean.

### BUG FOUND (important): model-derived completion check is unreliable in production
End-to-end `user_stress` with the wired loop: world-state improved over the 2/8 baseline (rename, extract, organize, count all reach PASS — the per-step grounding works), BUT the **`derive_expected` model-listed check is unreliable**, two failure modes:
- **FALSE SUCCESS** (the lie): `gather-patient-files` claimed accomplished but world-state FAIL — the model's derived expected paths were too WEAK (passed with an empty Smith/).
- **Under-claims**: `organize-pdfs`, `count-documents` succeeded (world PASS) but handed back — derived check too strict/wrong.
Root cause = same lesson as the whole verification arc: **the weak model cannot author the success check.** The loop MECHANISM (observe→reason→act→verify→feedback) is right; the VERIFIER must be DETERMINISTIC.

### FIX: deterministic `goal_completion_checks(goal)` replaces the model-derived completion check
Extracts the goal's NAMED target artifact (a `x.ext` file after to/into/called/named, or a "folder called X" → that dir must be NON-EMPTY), resolves the folder word (Documents/Downloads/Desktop), → `test -e` / non-empty-dir. Deterministic, conservative (no named target ⇒ no claim ⇒ honest handback). The model hint still guides ACTIONS; the deterministic check is the judge. Eliminates the gather false-success (Smith must be non-empty) and lets named-target goals claim correctly.

### RUN 1 DATA (deterministic check + narrow observe) — a REGRESSION, root cause found
- **user_stress 3/8** (organize✅uc, rename✅claim, tidy✅uc; new-folder/copy/gather/extract/count ❌). Note extract+count REGRESSED from PASS.
- **hard_stress 4/12** (delete-empty✅uc, move-and-rename✅claim, size-filter✅claim, destructive-trap✅uc; find-list = ⚠FALSE SUCCESS; rest ❌).
- **osworld_real 0/8** (ALL fail). Infeasible-traps (python4, undefined-paths) ✅ correctly REFUSED/handed back.

**ROOT CAUSE (two bugs I introduced):**
1. `discover_environment` was **hardcoded to the user_stress folders, top-level only**. OSWorld tasks live in `~/Desktop/photos/vacation/…`, `/tmp/test_files`, nested trees → the model literally **could not see the files** → couldn't act → 0/8.
2. The new **no-progress guard (`stale≥2`) fired prematurely** — that narrow env never changed (agent worked in undiscovered dirs) → early handback. This also regressed extract/count.

### FIX 2: `discover_environment` is now GOAL-RELEVANT + RECURSIVE
Roots = standard folders + `/tmp` + any absolute path named in the goal; `find -maxdepth 4 -not -path '*/.*' | sort -u | head -100` so nested files are VISIBLE and the env is stable for the no-progress compare. RUN 2 in progress to measure.

### STANDING FINDINGS (for the benchmark plan)
- The ReAct loop's success is bounded by what OBSERVE shows the model — observe must surface the goal's actual files (now recursive). Watch the prompt-size cost of large trees (cap=100 lines).
- `find-list` FALSE SUCCESS (claimed but FAIL): goal "save list to images.txt" → `goal_completion_checks` found `images.txt` and `test -e` passed, but the file lacked the jpg names → existence ≠ content. Content-bearing goals need a content check (the contract's "derive the final artifact, or stay silent" — here we have neither; tighten or stay silent).
- **GUI/MCP builds the benchmark needs** (from the OSWorld subagent): LibreOffice Calc/Writer + GIMP → UNO/Script-Fu batch MCP + AT-SPI; Chrome → browser DOM/CDP channel (some settings shell-pokeable via `Preferences` JSON); VLC/Thunderbird/multi_apps → app + GUI. These are the next surfaces; shell core is the current scope.
- Real OSWorld repo cloned at `/tmp/osworld` (369 tasks; multi_apps=101) for pulling more.

### RUN 2 DATA (deterministic check + RECURSIVE observe over /tmp+$HOME) — WORSE
- **user_stress 1/8, hard_stress 3/12, osworld_real 0/8.** The recursive `find` over `/tmp` + all of `$HOME` is a FIREHOSE (100 lines of tine/, system temp, irrelevant paths) that DROWNS the signal — the model reasons worse with the noise than with a too-narrow view. Measured: HALVED user_stress vs run 1.
- **Lesson: observe quality is the dominant lever, and it's a Goldilocks problem** — too narrow = blind to OSWorld trees (0/8); too broad = noise kills reasoning (1/8). Must be FOCUSED-recursive.

### FIX 3: focused observe = the 3 work dirs (Desktop/Documents/Downloads) at depth 4, no /tmp/$HOME-root.
RUN 3 DATA: **user 2/8, hard 1/12, osworld 1/8** (one real OSWorld task finally passed). Focused observe recovered OSWorld from 0→1 but didn't lift the rest.

### CONCLUSION: free-form authoring is CAPPED (~1–3/8) regardless of observe tuning
| observe variant | user_stress | hard_stress | osworld_real |
|---|---|---|---|
| narrow ls (run1) | 3/8 | 4/12 | 0/8 |
| recursive firehose (run2) | 1/8 | 3/12 | 0/8 |
| focused find (run3) | 2/8 | 1/12 | 1/8 |

### CAPABILITY LAYER PROBE — VALIDATED (the breakthrough)
`capability_probe`: model SELECTS typed verbs (`move source_dir=… selector=*.pdf dest=…`), harness does resolve→exec→verify. Same tasks + deterministic stop as free-form.
- **Raw 7/16**, then **9/16** after 4 well-formedness fixes (strip verb colon, drop `<>` menu placeholders, `dest`→`dest_file` alias, handle file-as-source).
- **A/B: user_stress free-form 2/8 → capability 5/8 (2.5×).** First result all session to BEAT the free-form ceiling.
- Failure modes: (1) **malformed calls** — `make_folder:` colon, echoed `<placeholder>`, wrong param name → **GBNF eliminates by construction**; (2) **interface-usage** — forgot `recursive=true` (copy-jpgs), `selector=empty` instead of `filter=empty` (delete-empty), mode confusion (extract) → GBNF enums + required-params help; (3) **genuine reasoning** — `gather` (which files are "Smith's"?), `copy-many` (3 separate copies) → the residual, harder tail.
- CONCLUSION: structured selection >> free-form authoring for LFM. Next = GBNF grammar (well-formedness + required-params + enums for mode/filter/recursive), then wire into the production loop (swap reason→capability-select, verify→declared postcondition). The probe UNDERSTATES the ceiling — most failures are exactly what the grammar removes.

### (earlier conclusion) The ceiling is the **free-form interface**, not the observe. The probe hit 6/6 only with hardcoded oracles. **The fix is the STRUCTURED-ACTION (Capability) layer** — typed verbs the model SELECTS via GBNF (grounded to observe paths), with DECLARED postconditions (the verify ships with the verb, not derived). This makes LFM a strong candidate (constrained action space + verifiable rewards = RLVR home turf) and the batteries become the RL reward harness. Spec'd in this session; build order = `Capability`/`ShellTemplate` type → 6 file-ops verbs (move/copy/rename/make_folder/delete/extract_to_file) → GBNF grammar with `path` bound to observe → swap the ReAct loop's reason/verify → re-bench.

### THE HONEST BOTTOM LINE (so far)
The ReAct loop hit **6/6 in the probe** but production has NOT reproduced it. The probe used a HARDCODED-perfect expected hint + the ground-truth check; production DERIVES both (`derive_expected` model-listed hint + `goal_completion_checks` heuristic) and **both are weaker than the probe's oracles** — that gap (not the loop mechanism) is why production sits at 1–3/8. Observe quality (FIX 3) is the first lever; the second is the expected-hint/completion derivation. Net: the loop MECHANISM is validated, the production DERIVATIONS are the open work. The wiring is in and honest (the deterministic check killed the gather false-success; only `find-list`'s existence-vs-content gap remains — content goals need a content check or silence).

