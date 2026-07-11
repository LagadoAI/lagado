"""failure_atlas.py — turn a whole-suite run into a precise per-failure map.

Correlates three captured sources into one atlas so every 0-scored task is explained by
WHERE it failed and WHAT the fix class is:
  - /tmp/lagado_battery/full_single.jsonl   — per-task {domain,id,score,flags}
  - ~/.local/share/lagado/chronos.log       — per-task trace, split on 'goal_received:'
  - /tmp/lagado_battery/solve_*.json        — rich calc authoring dumps (nameops/falsifiers)

Emits: per-domain gold/total, and for the failures a categorized breakdown:
  no_plane        — app has no API/plane; agent fell to GUI and couldn't complete (fix: build the plane)
  solver_ran_0    — calc solver applied ops but oracle=0 (fix: dump-level authoring analysis)
  backdoor_miss   — os settings route fired but read-back didn't verify (fix: verb/grounding)
  gave_up_early   — handed back with no plane engagement (fix: routing / capability gap)
  false_pass      — self-reported done but score 0 (INTEGRITY — must be zero)

Usage: python3 failure_atlas.py    (reads the live files; safe to run mid-run)
"""
import json, glob, os, re, collections

JSONL = "/tmp/lagado_battery/full_single.jsonl"
CHRONOS = os.path.expanduser("~/.local/share/lagado/chronos.log")
EXDIR = "/home/alucard/projects/OSWorld/evaluation_examples/examples"


def load_instructions():
    """(domain,id8) -> instruction, from the eval task files (the resume jsonl omits it)."""
    out = {}
    for tf in glob.glob(f"{EXDIR}/*/*.json"):
        try:
            t = json.load(open(tf))
            dom = os.path.basename(os.path.dirname(tf))
            out[(dom, (t.get("id") or "")[:8])] = (t.get("instruction") or "")
        except: pass
    return out


def load_scores():
    rows = []
    for ln in open(JSONL):
        try: rows.append(json.loads(ln))
        except: pass
    return rows


def split_tasks(log_text):
    """Split the chronos stream into per-task segments keyed by goal instruction prefix."""
    segs = []
    cur_goal, cur = None, []
    for ln in log_text.splitlines():
        m = re.search(r'goal_received:\s*(.*)', ln)
        if m:
            if cur_goal is not None:
                segs.append((cur_goal, "\n".join(cur)))
            cur_goal, cur = m.group(1).strip()[:70], []
        else:
            cur.append(ln)
    if cur_goal is not None:
        segs.append((cur_goal, "\n".join(cur)))
    # last segment per goal-prefix wins (retries reuse the goal)
    by_goal = {}
    for g, seg in segs:
        by_goal[g] = by_goal.get(g, "") + "\n" + seg
    return by_goal


NO_PLANE_DOMS = ("libreoffice_impress", "libreoffice_writer", "vlc", "thunderbird", "gimp")


def classify(trace, dom):
    """Return (category, note) for a failed task from its trace. Fix class per category:
      solver_ran_0        -> per-dump authoring analysis (chart/total/formula shape)
      backdoor_miss       -> verb/grounding on the settings route
      no_actuation_plane  -> chrome: DOM sight exists, needs CDP click/type actuation
      no_plane            -> build the app's UNO/API plane
      composite_fail      -> multi_apps: a sub-app with no plane starved the chain
      gave_up_early       -> routing/capability gap (no plane engaged)
      false_pass          -> INTEGRITY: self-reported done, scored 0 (must be 0)"""
    t = trace or ""
    if "self_report_done: True" in t or '"self_report_done": true' in t or "goal accomplished" in t.lower():
        # only a real integrity breach if the agent asserted done; verify against score upstream
        if "couldn't verify" not in t and "handing back" not in t:
            return "false_pass", "self-reported done, scored 0 — INTEGRITY"
    if "calc solver exit=" in t or (dom == "libreoffice_calc" and "api_plane" in t):
        return "solver_ran_0", "solver applied ops, oracle=0 (silent-wrong; see solve dump)"
    if "back_door" in t:
        return ("backdoor_miss",
                "settings write didn't verify on read-back" if "MISMATCH" in t else "back-door engaged, unverified")
    if dom == "chrome":
        return "no_actuation_plane", "DOM floor gives sight; needs CDP actuation (click/type/navigate)"
    if dom == "multi_apps":
        return "composite_fail", "composite chain — a sub-app plane (impress/writer/browser) missing"
    if dom in NO_PLANE_DOMS or "no API-addressable" in t or "falling through to GUI" in t:
        return "no_plane", "no API plane for this app → build the UNO/API plane"
    return "gave_up_early", "handed back — routing/capability gap"


def main():
    rows = load_scores()
    instrs = load_instructions()
    log = open(CHRONOS).read() if os.path.exists(CHRONOS) else ""
    by_goal = split_tasks(log)

    dom_g = collections.Counter(); dom_c = collections.Counter()
    cat = collections.Counter(); cat_by_dom = collections.defaultdict(collections.Counter)
    false_passes = []
    for r in rows:
        dom, tid, score = r["domain"], r["id"], r.get("score", 0)
        dom_c[dom] += 1
        if score >= 1:
            dom_g[dom] += 1
            continue
        # match trace by instruction prefix (instruction looked up by id — jsonl omits it)
        instr = (instrs.get((dom, tid)) or "")[:70].strip()
        trace = next((v for g, v in by_goal.items() if g and instr and (g in instr or instr in g)), "")
        c, note = classify(trace, dom)
        cat[c] += 1; cat_by_dom[dom][c] += 1
        if c == "false_pass":
            false_passes.append((dom, tid, note))

    total_g = sum(dom_g.values()); total = sum(dom_c.values())
    print(f"=== FAILURE ATLAS | {total_g}/{total} gold ({100*total_g/max(total,1):.1f}%) ===\n")
    print("per-domain:")
    for d in sorted(dom_c):
        print(f"  {d:24s} {dom_g[d]:3d}/{dom_c[d]:<3d}  fails: {dict(cat_by_dom[d])}")
    print("\nfailure categories (fix class):")
    for c, n in cat.most_common():
        print(f"  {c:16s} {n}")
    print(f"\nFALSE PASSES (must be 0): {len(false_passes)}")
    for d, t, n in false_passes:
        print(f"  ! {d}/{t}: {n}")


if __name__ == "__main__":
    main()
