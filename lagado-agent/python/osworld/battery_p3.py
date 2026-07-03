"""P3 — false-pass on a NO-ORACLE / plausible-wrong result (the integrity case OSWorld cannot give).

A wrong-but-well-formed formula passes every SOUND falsifier (numeric, full extent, no error, not a
text-op). So "no fault detected" is NOT "correct" — and a harness that equates them FALSE-PASSES with
confidence. OSWorld can never test this: every OSWorld task has a deterministic oracle by construction.

The honest discriminator with no oracle (per the advisor): INDEPENDENT RE-DERIVATION. Author the answer a
second time independently; if the two derivations AGREE on the computed values, mark it corroborated
(higher confidence — NOT proof; correlated errors bound it). If they DISAGREE, the result is contested →
the harness must ABSTAIN ("no fault detected, UNVERIFIED"), not claim done.

We test two scenarios on the gross-profit column (ground truth known = gold J values):
  CLEAN  — two honest independent derivations. Expect: agree → verified=True, and matches gold. (no cry-wolf)
  POISON — derivation 1 is a plausible-wrong formula (drops Overhead); derivation 2 honest. Expect: the
           sound falsifiers all PASS (would FALSE-PASS), but corroboration DISAGREES → verified=False
           (honest abstain). This is the false-pass turned into an honest "uncertain".

Run (from OSWorld dir, its venv, podman sock):
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python /home/alucard/projects/lagado/lagado-agent/python/osworld/battery_p3.py <task_json>
"""
import json, os, sys, time
import requests
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_session_task import Guest, deploy_daemon, pick_uno_python, task_input_path
from battery_calc import (detect, candidate_cards, REASON_PROMPT, EMIT_PROMPT, GRAMMAR_B,
                          parse_B_nameops, substitute_names, resolve_name, BRAIN, falsify)
from desktop_env.desktop_env import DesktopEnv

GOLD_J = [55000.0, 47662.0, 53451.0, 60819.0, 62864.0, 57924.0, 59634.0, 50852.0, 43649.0]
# A plausible-wrong gross profit: drops Overhead. Well-formed, numeric, full extent, no error.
POISON_NAMEFORMULA = "={Sales}-{Sales Return}-{Discounts and Allowances}-{Materials Charges}-{Labor Charges}"

def gp_nameformula(instr, detected, temperature):
    """One independent derivation of the gross-profit name-formula (reason→emit). Returns the formula
    string in {names}, or None. temperature>0 makes derivation 2 genuinely independent."""
    cards = candidate_cards(detected)
    r1 = requests.post(BRAIN, json={"prompt": REASON_PROMPT.format(instr=instr, cards=cards),
                                    "temperature": temperature, "n_predict": 400, "seed": int(temperature * 1000)},
                       timeout=200)
    reasoning = r1.json().get("content", "").strip()
    emit = EMIT_PROMPT.format(instr=instr, cards=cards, reasoning=reasoning)
    r2 = requests.post(BRAIN, json={"prompt": emit, "grammar": GRAMMAR_B, "temperature": temperature,
                                    "n_predict": 800, "seed": int(temperature * 1000)}, timeout=200)
    for nop in parse_B_nameops(r2.json().get("content", "")):
        if nop["kind"] == "compute_column" and nop.get("sheet") == "Sheet1":
            return nop["formula"]
    return None

def apply_gp(g, name_formula, detected):
    """Resolve a gross-profit name-formula → A1, apply to J2:J10, return the computed values (or None)."""
    if name_formula is None:
        return None
    fails = []
    a1 = substitute_names(name_formula.replace("'", '"'), "Sheet1", detected, fails, row=2)
    if a1 is None:
        return None
    g.client("apply", {"op": {"op": "set_formula_range", "sheet": "Sheet1", "range": "J2:J10", "formula": a1}})
    r = g.client("read", {"sheet": "Sheet1", "range": "J2:J10"})
    return [row[0] for row in r.get("cells", [])] if r.get("ok") else None

def scenario(env, task, file_path, name, der1_formula_or_none, instr, detected_holder):
    """Run one scenario; der1_formula_or_none=None means 'derive honestly', else use the given (poison) formula."""
    g = Guest(env)
    unopy = pick_uno_python(g)
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    deploy_daemon(g, unopy)
    g.client("open", {"file": file_path})
    detail = g.client("structure").get("detail", [])
    detected = detect(g, detail)
    detected_holder["d"] = detected

    f1 = der1_formula_or_none or gp_nameformula(instr, detected, 0.0)   # derivation 1
    f2 = gp_nameformula(instr, detected, 0.6)                            # derivation 2 (independent)
    v1 = apply_gp(g, f1, detected)                                      # apply der1 → values
    fired = falsify(g, [("Sheet1", "J2:J10", substitute_names((f1 or '').replace("'", '"'), "Sheet1", detected, [], 2) or '')])
    v2 = apply_gp(g, f2, detected)                                      # apply der2 → values (overwrites J)
    g.client("close")

    def approx(a, b):
        if not a or not b or len(a) != len(b):
            return False
        return all(isinstance(x, (int, float)) and isinstance(y, (int, float)) and abs(x - y) < 1e-6
                   for x, y in zip(a, b))
    oracle_correct = approx(v1, GOLD_J)            # ground truth for der1 (the applied answer)
    corroborated = approx(v1, v2)                  # independent agreement (no oracle needed)
    no_fault = (v1 is not None and len(fired) == 0)   # sound falsifiers found nothing
    return {
        "scenario": name, "f1": f1, "f2": f2, "v1": v1, "v2": v2,
        "oracle_correct": oracle_correct, "falsifiers_fired": [x["falsifier"] for x in fired],
        "no_fault_detected": no_fault, "corroborated": corroborated,
        # the honest "done" signal: no fault AND independently corroborated. Never on a contested result.
        "verified": bool(no_fault and corroborated),
        # what a NAIVE harness would claim (no-fault == done) — the false-pass generator:
        "naive_done": no_fault,
        "would_false_pass": bool(no_fault and not oracle_correct),
    }

def main():
    task = json.load(open(sys.argv[1]))
    instr = task["instruction"]
    file_path = task_input_path(task)
    print("=== P3 — plausible-wrong false-pass vs corroboration-based abstain ===", flush=True)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    out = []
    try:
        for name, der1 in [("CLEAN", None), ("POISON", POISON_NAMEFORMULA)]:
            env.reset(task_config=task); time.sleep(2)
            r = scenario(env, task, file_path, name, der1, instr, {})
            out.append(r)
            print("\n[%s]" % name, flush=True)
            print("  der1 formula:", r["f1"], flush=True)
            print("  der2 formula:", r["f2"], flush=True)
            print("  der1 values :", r["v1"], flush=True)
            print("  der2 values :", r["v2"], flush=True)
            print("  oracle_correct=%s  sound_falsifiers=%s  no_fault_detected=%s" % (
                r["oracle_correct"], r["falsifiers_fired"], r["no_fault_detected"]), flush=True)
            print("  >> NAIVE harness would say done=%s  (would_false_pass=%s)" % (r["naive_done"], r["would_false_pass"]), flush=True)
            print("  >> corroborated=%s  ==>  VERIFIED=%s  (honest)" % (r["corroborated"], r["verified"]), flush=True)
    finally:
        env.close()
    json.dump(out, open("/tmp/lagado_battery/p3.json", "w"), indent=1, default=str)
    print("\n=== P3 SUMMARY ===", flush=True)
    for r in out:
        print("  %-7s oracle_correct=%-5s naive_done=%-5s would_false_pass=%-5s VERIFIED=%-5s" % (
            r["scenario"], r["oracle_correct"], r["naive_done"], r["would_false_pass"], r["verified"]), flush=True)

if __name__ == "__main__":
    sys.exit(main())
