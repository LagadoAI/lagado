"""Settle-monitor audit — the adversarial pass over a battery run's settle_wait entries.

The monitor's failure mode is EARLY RELEASE: declaring settle before the reconcile
GUI reload's paint actually lands, handing env.evaluate() a half-rendered window.
A miss caused that way looks identical to a semantic miss in the score column —
this audit is how we tell them apart, per the 2026-07-06 brutal-test directive.

Reads breadth_logs.jsonl records (each has id/score/false_pass/settle_wait) and reports:
  - per-task settle table (mode, ticks, seconds, settled flag)
  - fail-open count (mode=cfc_failopen — the deterministic floor stood in)
  - suspicious releases: settled=True with s < SUSPECT_S (faster than any observed
    honest reload settle in the gate data; gate latency floor was ~1.9s)
  - miss-vs-settle correlation: do score=0 tasks settle systematically faster?
    (mean/min settle s for golds vs misses — an early-release signature)
  - time SAVED vs the fixed 4.0s floor (the monitor's earn)

Usage: python3 settle_audit.py [logfile] [--tail N]   (default: the standard breadth log,
       last N=30 records = the current run)
"""
import json
import sys

LOG = "/tmp/lagado_battery/breadth_logs.jsonl"
FLOOR_S = 4.0
SUSPECT_S = 1.0   # no honest reload in the 52-episode gate corpus settled this fast


def main():
    path, tail = LOG, 30
    argv = sys.argv[1:]
    i = 0
    while i < len(argv):
        a = argv[i]
        if a.startswith("--tail"):
            tail = int(a.split("=", 1)[1]) if "=" in a else int(argv[i + 1])
            i += 1 if "=" in a else 2
        else:
            path = a
            i += 1
    recs = [json.loads(x) for x in open(path).read().splitlines() if x.strip()]
    recs = recs[-tail:]

    with_sw = [r for r in recs if r.get("settle_wait")]
    print("=== SETTLE AUDIT: %d records, %d with settle_wait ===" % (len(recs), len(with_sw)))
    if not with_sw:
        print("no settle_wait entries — monitor path never engaged (host run / flag off?)")
        return

    failopen, suspects, saved = [], [], 0.0
    golds, misses = [], []
    print("%-10s %-6s %-12s %5s %6s %8s" % ("id", "score", "mode", "ticks", "s", "flag"))
    for r in with_sw:
        sw = r["settle_wait"]
        s = float(sw.get("s", -1))
        flag = ""
        if sw.get("mode") == "cfc_failopen":
            failopen.append(r)
            flag = "FAILOPEN:" + sw.get("error", "")[:40]
        elif sw.get("settled") and s < SUSPECT_S:
            suspects.append(r)
            flag = "SUSPECT-EARLY"
        if sw.get("mode") == "cfc" and sw.get("settled"):
            saved += max(0.0, FLOOR_S - s)
            (golds if r.get("score") == 1.0 else misses).append(s)
        fp = " FALSE-PASS" if r.get("false_pass") else ""
        print("%-10s %-6s %-12s %5d %6.2f %8s%s" % (
            str(r.get("id", "?"))[:10], r.get("score"), sw.get("mode", "?"),
            sw.get("ticks", 0), s, flag, fp))

    print("\nfail-opens: %d  (floor stood in — production-safe, but each one is a monitor outage)"
          % len(failopen))
    print("suspicious early releases (<%.1fs): %d" % (SUSPECT_S, len(suspects)))
    if golds:
        print("gold  settle: mean %.2fs  min %.2fs  n=%d" % (sum(golds) / len(golds), min(golds), len(golds)))
    if misses:
        print("miss  settle: mean %.2fs  min %.2fs  n=%d" % (sum(misses) / len(misses), min(misses), len(misses)))
        if golds and (sum(misses) / len(misses)) < (sum(golds) / len(golds)) - 0.5:
            print("⚠ misses settle >0.5s faster than golds — EARLY-RELEASE SIGNATURE, "
                  "rerun those tasks with LAGADO_SETTLE_MONITOR=0 to attribute")
    print("time saved vs fixed %.1fs floor: %.1fs across %d monitored settles"
          % (FLOOR_S, saved, len(golds) + len(misses)))


if __name__ == "__main__":
    main()
