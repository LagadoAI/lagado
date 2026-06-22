"""p1_session_test.py — Native Session Plane, Phase 1 gate (host-local, no model).

Proves the resident UNO daemon + client in ISOLATION (spec §9 P1):
  - per-op OBSERVATION works: `read`/`structure` reflect `apply` against the live
    in-memory doc BETWEEN ops, with no store/reload (the capability the one-shot lacks);
  - a hand-driven session REPRODUCES 01b269ae's filled file (== the OSWorld gold), and the
    stored file round-trips (reload from disk still matches);
  - teardown leaks NO soffice (checked by OUR profile string only — never a global pkill).

Run on the dev host (which has soffice + python-uno):
    python3 docs/osworld/p1_session_test.py
Exit 0 = PASS.
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import uno_client  # noqa: E402

SOCK = "/tmp/lagado_p1_test.sock"
UNO_PORT = 2002
PROFILE_TAG = "lagado_uno_daemon_profile_"  # scoped leak-check token (never global soffice)

INPUT_CANDIDATES = [
    "/tmp/m2cache/01b269ae_in_Student_Level_Fill_Blank.xlsx",
    "/home/alucard/projects/OSWorld/cache/01b269ae-2111-4a07-81fd-3fcd711993b0/Student_Level_Fill_Blank.xlsx",
]
GOLD_CANDIDATES = [
    "/tmp/m2cache/01b269ae_gold.xlsx",
]


def first_existing(paths, what):
    for p in paths:
        if os.path.exists(p):
            return p
    sys.stderr.write("FATAL: no %s found; tried:\n  %s\n" % (what, "\n  ".join(paths)))
    sys.exit(3)


def cells_equal(a, b, tol=1e-9):
    """Cell-for-cell compare of two read() grids; floats tolerant, None/str exact."""
    if len(a) != len(b):
        return False
    for ra, rb in zip(a, b):
        if len(ra) != len(rb):
            return False
        for x, y in zip(ra, rb):
            if isinstance(x, float) and isinstance(y, float):
                if abs(x - y) > tol:
                    return False
            elif x != y:
                return False
    return True


def has_blank(grid):
    return any(v is None for row in grid for v in row)


class Session:
    """Spawns the daemon subprocess and proxies verbs through uno_client over the socket."""

    def __init__(self):
        if os.path.exists(SOCK):
            os.remove(SOCK)
        self.proc = subprocess.Popen(
            [sys.executable, os.path.join(HERE, "uno_daemon.py"),
             "--sock=%s" % SOCK, "--port=%d" % UNO_PORT],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        # Wait for the readiness line (socket bound).
        deadline = time.time() + 20
        while time.time() < deadline:
            line = self.proc.stdout.readline()
            if "DAEMON READY" in line:
                return
            if self.proc.poll() is not None:
                raise RuntimeError("daemon exited early:\n" + line + self.proc.stdout.read())
        raise RuntimeError("daemon did not signal readiness in time")

    def call(self, verb, **args):
        req = dict(args)
        req["verb"] = verb
        r = uno_client.call(SOCK, req)
        if not r.get("ok"):
            raise AssertionError("verb %s failed: %s" % (verb, r.get("error")))
        return r

    def kill(self):
        try:
            self.proc.send_signal(2)  # SIGINT
            self.proc.wait(timeout=8)
        except Exception:
            try:
                self.proc.kill()
            except Exception:
                pass


def check(cond, msg):
    if not cond:
        raise AssertionError("CHECK FAILED: " + msg)
    print("  ok  - " + msg)


def main():
    input_xlsx = first_existing(INPUT_CANDIDATES, "01b269ae input fixture")
    gold_xlsx = first_existing(GOLD_CANDIDATES, "01b269ae gold")
    print("input:", input_xlsx)
    print("gold :", gold_xlsx)

    tmp = tempfile.mkdtemp(prefix="lagado_p1_")
    work1 = os.path.join(tmp, "work1.xlsx")
    work2 = os.path.join(tmp, "work2.xlsx")
    shutil.copy(input_xlsx, work1)
    shutil.copy(input_xlsx, work2)

    s = Session()
    try:
        # ── Session A — the 01b269ae gate (reproduce gold + store round-trips) ──
        print("\n[A] 01b269ae fill gate")
        gold = s.call("open", file=gold_xlsx)
        gold_cells = s.call("read", sheet=None, range="B1:E30")["cells"]
        check(not has_blank(gold_cells), "gold B1:E30 has no blanks (sanity)")

        s.call("open", file=work1)  # identity guard closes gold, opens work1
        s.call("apply", op={"op": "fill", "sheet": "Sheet1", "range": "B1:E30", "direction": "down"})
        produced = s.call("read", sheet=None, range="B1:E30")["cells"]
        check(not has_blank(produced), "after fill, live read of B1:E30 has NO blanks (apply→read observed)")
        check(cells_equal(produced, gold_cells), "live produced B1:E30 == gold cell-for-cell")

        s.call("reconcile", gui=False)  # stores work1, tears down our office
        s.call("open", file=work1)       # reload from DISK (office restarts)
        stored = s.call("read", sheet=None, range="B1:E30")["cells"]
        check(cells_equal(stored, gold_cells), "stored-then-reloaded B1:E30 == gold (reconcile produced the right file)")

        # ── Session B — per-op observation (the daemon's reason for being) ──
        print("\n[B] per-op observation (apply→structure/read between ops)")
        s.call("open", file=work2)
        struct0 = s.call("structure")
        check("Probe" not in struct0["sheets"], "Probe sheet absent before add_sheet")

        s.call("apply", op={"op": "add_sheet", "name": "Probe", "index": 0})
        struct1 = s.call("structure")
        check("Probe" in struct1["sheets"], "structure() reflects add_sheet WITHOUT reload")

        for cell, val in (("B1", 10.0), ("B2", 20.0), ("B3", 30.0)):
            s.call("apply", op={"op": "set", "sheet": "Probe", "cell": cell, "value": val})
        bvals = s.call("read", sheet="Probe", range="B1:B3")["cells"]
        check(cells_equal(bvals, [[10.0], [20.0], [30.0]]), "read() reflects three set ops on the new sheet")

        # set_formula_range with relative-ref propagation, read back COMPUTED (auto-calc on)
        s.call("apply", op={"op": "set_formula_range", "sheet": "Probe", "range": "C1:C3", "formula": "=B1*2"})
        cvals = s.call("read", sheet="Probe", range="C1:C3")["cells"]
        check(cells_equal(cvals, [[20.0], [40.0], [60.0]]),
              "set_formula_range computed + relative-adjusted, read back live: C=[20,40,60]")

        # single-cell formula via `set`, read back computed
        s.call("apply", op={"op": "set", "sheet": "Probe", "cell": "C5", "formula": "=SUM(C1:C3)"})
        c5 = s.call("read", sheet="Probe", range="C5:C5")["cells"]
        check(cells_equal(c5, [[120.0]]), "set-formula SUM read back computed: C5=120")

        s.call("close")

        # ── teardown leak check — SCOPED to our profile token, never global soffice ──
        print("\n[C] teardown / leak check")
        time.sleep(1.0)
        # Match by process NAME (`pgrep -a soffice` -> soffice.bin), NOT full-cmdline, so the
        # check can't self-match its own shell wrapper; then keep only lines bearing OUR
        # profile token. This is scoped to our owned office — never a global soffice sweep.
        out = subprocess.run("pgrep -a soffice 2>/dev/null || true", shell=True,
                             capture_output=True, text=True).stdout
        leaked = [ln for ln in out.splitlines() if PROFILE_TAG in ln]
        check(not leaked, "no leaked soffice for our profile token after close (got: %r)" % leaked)
        import glob
        prof_dirs = glob.glob("/tmp/%s*" % PROFILE_TAG)
        check(not prof_dirs, "no leaked UserInstallation profile dir after close (got: %r)" % prof_dirs)

    finally:
        s.kill()
        shutil.rmtree(tmp, ignore_errors=True)
        if os.path.exists(SOCK):
            os.remove(SOCK)

    print("\nP1 GATE: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
