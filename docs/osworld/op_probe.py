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
    try:
        env.reset(task_config=task); time.sleep(2)
        g = Guest(env); unopy = pick_uno_python(g)
        g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
        g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(fp), os.path.basename(fp)))
        time.sleep(1)
        assert deploy_daemon(g, unopy), "daemon not ready"
        assert g.client("open", {"file": fp}).get("ok"), "open failed"

        results = {}
        # 1. format_cells — apply-ok (style not value-readable)
        results["format_cells"] = g.client("apply", {"op": {"op": "format_cells", "sheet": "Sheet1",
            "range": "A1:A1", "font_color": "#ffffff", "fill_color": "#0000ff", "bold": "true"}})
        # 2. merge_cells
        results["merge_cells"] = g.client("apply", {"op": {"op": "merge_cells", "sheet": "Sheet1", "range": "L1:N1"}})
        # 3. set_number_format — apply-ok; value stays numeric
        results["set_number_format"] = g.client("apply", {"op": {"op": "set_number_format", "sheet": "Sheet1",
            "range": "B2:B10", "format": "0.00"}})
        # 4. total_row via the harness verb (expands to set+SUM); verify the SUM landed numeric
        live = B.live_detect(g)
        B.apply_B(g, [{"kind": "total_row", "sheet": "Sheet1", "label": "Total",
                       "columns": "{Sales},{Materials Charges}"}], {})
        b = col(g, "B2:B12"); f = col(g, "F2:F12")
        # 5. sort_range — sort A1:J10 by Sales (B) ascending; read B back, must be non-decreasing
        results["sort_range"] = g.client("apply", {"op": {"op": "sort_range", "sheet": "Sheet1",
            "range": "A1:J10", "key_index": 1, "ascending": "true", "has_header": "true"}})
        b_sorted = col(g, "B2:B10")

        print("\n=== APPLY RESULTS ===")
        for k, v in results.items():
            print("  %-18s %s" % (k, v))
        print("\n=== total_row (SUM landed?) ===")
        print("  col B (Sales) 2..12 :", b)
        print("  col F (Materials)   :", f)
        b_sum = next((x for x in b if isinstance(x, (int, float)) and x > 100000), None)  # the SUM cell
        print("  >> Sales SUM cell present & numeric:", b_sum is not None, "(", b_sum, ")")
        print("\n=== sort_range (Sales ascending?) ===")
        nums = [x for x in b_sorted if isinstance(x, (int, float))]
        ok_sorted = nums == sorted(nums)
        print("  B2:B10 after sort:", b_sorted)
        print("  >> non-decreasing:", ok_sorted)
        print("\n=== VERDICT ===")
        all_ok = (all(r.get("ok") for r in results.values()) and b_sum is not None and ok_sorted)
        print("  ALL WAVE-1 OPS OK:", all_ok)
        if not all_ok:
            print("  (inspect above — any apply ok=False or sort not ascending = fix before sweep)")
        g.client("close")
    finally:
        env.close()


if __name__ == "__main__":
    main()
