"""op_probe.py — verify the Wave-1 op-vocab additions on the live guest BEFORE the full re-test.

Exercises each new op on a real open doc (035f41ba columns: A=Year,B=Sales,C=Sales Return,D=Disc,
E=Net Sales,F=Materials,G=Labor,H=Overhead,I=COGS,J=Gross Profit) and reads back what is verifiable
via VALUE (sort order, total sums, number kept numeric). format/merge/number-format are confirmed by
apply-ok (the daemon read returns values, not style). Riskiest = sort_range (SortField struct).

Throwaway probe over the proven session. Does NOT score. Run:
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python /home/alucard/projects/lagado/docs/osworld/op_probe.py
"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import battery_calc as B
from run_session_task import Guest, deploy_daemon, pick_uno_python, task_input_path
from desktop_env.desktop_env import DesktopEnv

TASK = "/home/alucard/projects/OSWorld/evaluation_examples/examples/libreoffice_calc/035f41ba-6653-43ab-aa63-c86d449d62e5.json"


def col(g, rng):
    r = g.client("read", {"sheet": "Sheet1", "range": rng})
    return [row[0] if row else None for row in r.get("cells", [])] if r.get("ok") else "FAIL:%s" % r.get("error")


def main():
    task = json.load(open(TASK)); fp = task_input_path(task)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    def step(msg):
        print("[%6.1fs] %s" % (time.time() - t0, msg), flush=True)
    t0 = time.time()
    try:
        step("reset()..."); env.reset(task_config=task); time.sleep(2); step("reset done")
        g = Guest(env); unopy = pick_uno_python(g); step("uno=%s" % unopy)
        g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
        g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(fp), os.path.basename(fp)))
        time.sleep(1)
        step("deploy_daemon..."); assert deploy_daemon(g, unopy), "daemon not ready"; step("daemon ready")
        step("open..."); assert g.client("open", {"file": fp}).get("ok"), "open failed"; step("opened")

        def op(label, payload):
            step("apply %s ..." % label)
            r = g.client("apply", {"op": payload})
            step("apply %s -> %s" % (label, r))
            return r

        r1 = op("format_cells", {"op": "format_cells", "sheet": "Sheet1", "range": "A1:A1",
                                 "font_color": "#ffffff", "fill_color": "#0000ff", "bold": "true"})
        r2 = op("merge_cells", {"op": "merge_cells", "sheet": "Sheet1", "range": "L1:N1"})
        r3 = op("set_number_format", {"op": "set_number_format", "sheet": "Sheet1",
                                      "range": "B2:B10", "format": "0.00"})
        step("total_row via apply_B ...")
        B.apply_B(g, [{"kind": "total_row", "sheet": "Sheet1", "label": "Total",
                       "columns": "{Sales},{Materials Charges}"}], {})
        b = col(g, "B2:B12"); step("total_row read B2:B12 -> %s" % b)
        r5 = op("sort_range", {"op": "sort_range", "sheet": "Sheet1", "range": "A1:J10",
                               "key_index": 1, "ascending": "true", "has_header": "true"})
        b_sorted = col(g, "B2:B10"); step("sort read B2:B10 -> %s" % b_sorted)
        # copy_sheet MID-INSERT (the case that actually failed on 0cecd4f3): build [Sheet1,S2,S3], then copy
        # Sheet1 -> DupSheet BEFORE S3 → must land at index 2, i.e. [Sheet1, S2, DupSheet, S3].
        op("add_sheet", {"op": "add_sheet", "name": "S2"})
        op("add_sheet", {"op": "add_sheet", "name": "S3"})
        r6 = op("copy_sheet", {"op": "copy_sheet", "source": "Sheet1", "new": "DupSheet", "before": "S3"})
        names = g.client("structure").get("sheets", [])
        rd = g.client("read", {"sheet": "DupSheet", "range": "B2:B2"})
        dup_val = (rd.get("cells", [[None]])[0][0]) if rd.get("ok") else None
        # position: DupSheet immediately before S3 (index 2); data: B2 numeric (Sheet1 was sorted earlier)
        copy_ok = (names == ["Sheet1", "S2", "DupSheet", "S3"]) and isinstance(dup_val, (int, float))
        step("copy_sheet MID -> order=%s  DupSheet!B2=%s  mid-insert+data OK=%s" % (names, dup_val, copy_ok))

        b_sum = next((x for x in b if isinstance(x, (int, float)) and x > 100000), None)
        nums = [x for x in b_sorted if isinstance(x, (int, float))]
        ok_sorted = nums == sorted(nums)
        print("\n=== VERDICT ===", flush=True)
        print("  format=%s merge=%s numfmt=%s sort=%s copy=%s" % (
            r1.get("ok"), r2.get("ok"), r3.get("ok"), r5.get("ok"), r6.get("ok")))
        print("  total_row SUM numeric:", b_sum is not None, "(", b_sum, ")")
        print("  sort non-decreasing:", ok_sorted)
        print("  copy_sheet positioned + data copied:", copy_ok)
        print("  ALL OPS OK:", all(r.get("ok") for r in [r1, r2, r3, r5, r6]) and b_sum is not None and ok_sorted and copy_ok)
        g.client("close")
    finally:
        env.close()


if __name__ == "__main__":
    main()
