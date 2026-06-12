#!/usr/bin/env python3
"""perceive.py — AT-SPI2 screen reader for the Laputa agent.

Calls `python3 -m tine.cli tree` to read the desktop accessibility tree and
emits a representation tuned to the active mode.

tine contract:
  - `tine.cli tree [--app NAME]` prints an indented text tree AND saves a ref
    cache sidecar to TINE_REF_CACHE (default /tmp/tine-refs.json).
  - Ref cache format: {"ref_N": {"role","name","bbox":{"x","y","w","h",
    "coord":"window"}, "actions":[...]}, "_app":"..."}.  Keys starting with
    "_" are metadata and are skipped.
  - Bboxes in the cache are WINDOW-RELATIVE, not screen-absolute.
    In --focused mode each bbox is converted to screen coords by adding the
    window origin from `xdotool getwindowgeometry`.  In other modes bboxes
    are emitted window-relative (acceptable for JSON consumers).
  - Garbage/unrealized bboxes (x or y outside [-32768, 32768], or w/h ≤ 0)
    are filtered out.  In --focused mode elements with no valid bbox are
    dropped entirely — a ref the agent cannot click is noise.
  - `tine click` is NOT used: it requires GNOME Mutter DisplayConfig D-Bus or
    TINE_SCALE/TINE_PHYSICAL_* env, and its window-offset resolution fails on
    XFCE ("could not list windows — using offset (0,0)").  All clicking goes
    through xdotool in SshActuator.
  - `tine.cli tree --json` DOES NOT EXIST and is dead code — removed.

Modes (mutually exclusive; first one specified wins):
  (default)            Full desktop JSON: {"apps":[...], "elements":[...],
                       "terminals":[]}.  Bboxes are window-relative.
  --focused            Text dump of ONLY the focused window's interactive
                       elements, one per line, with screen-absolute bounding
                       boxes and window-relative position hints.  This is the
                       format intended for the agent loop.
  --interactive-only   Same JSON shape as default, but non-interactive
                       elements removed across all apps.
  --text               (No-op — tine does not expose terminal text.)
  --raw                Raw `tine.cli tree` output.
  --print-focus        Print the focused window title and exit (dry-run).

All subprocess calls use a hard timeout so a wedged desktop can't hang
the agent.  Output is always UTF-8 text on stdout.  Errors go to stderr.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from typing import Any, Optional

# ── Configuration ────────────────────────────────────────────────────────────
# tine is a local Python package at guest:/home/laputa/tine/; importable when
# PYTHONPATH=/home/laputa (so `import tine` resolves via /home/laputa/tine/__init__.py).
TINE_CMD = ["env", "PYTHONPATH=/home/laputa", "python3", "-m", "tine.cli", "tree"]

SUBPROCESS_TIMEOUT  = 5      # seconds — hard cap on every external call
XDOTOOL_TIMEOUT     = 2      # seconds — xdotool is fast; fail-fast on wedge
MAX_ELEMENTS_FULL   = 120    # cap for full-desktop JSON output
MAX_ELEMENTS_FOCUS  = 50     # cap for focused-mode text output

# Default tine ref cache path; override with TINE_REF_CACHE env var.
_DEFAULT_REF_CACHE = "/tmp/tine-refs.json"

# The desktop panel's AT-SPI application name. The panel is docked at the screen
# edge and its frame origin is the screen origin, so its window-relative bboxes
# are screen-absolute — the one app whose elements are clickable with no focused
# window. Override with LAGADO_PANEL_APP for other desktop environments.
PANEL_APP = os.environ.get("LAGADO_PANEL_APP", "xfce4-panel")

# AT-SPI2 roles that are actionable / worth surfacing to the agent.
INTERACTIVE_ROLES = {
    # buttons
    "button", "push button", "toggle button",
    # checkable
    "check box", "radio button", "check menu item", "radio menu item",
    # text input
    "entry", "text", "editable text", "password text", "spin button",
    # selection
    "combo box", "combobox", "list", "list box", "list item",
    "tree", "tree item", "tree table",
    # navigation / chrome
    "link", "menu", "menu item", "menu bar",
    "tab", "page tab", "tab list",
    # ranges
    "slider", "scroll bar", "scrollbar",
    # containers treated as click targets in app launchers / file managers
    "icon", "tool tip", "tool bar",
    # pickers
    "date editor", "color chooser",
}

# Roles explicitly dropped as visual noise even in full mode.
NOISE_ROLES = {"separator", "filler", "layered pane", "redundant object"}


# ── Subprocess helpers ────────────────────────────────────────────────────────
def _have(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def _run(cmd: list[str], timeout: int = SUBPROCESS_TIMEOUT) -> tuple[bool, str, str]:
    """Run a subprocess. Returns (ok, stdout, stderr). Never raises."""
    try:
        cp = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return (cp.returncode == 0, cp.stdout or "", cp.stderr or "")
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError) as e:
        return (False, "", str(e))


# ── tine ref cache ────────────────────────────────────────────────────────────
def _ref_cache_path() -> str:
    return os.environ.get("TINE_REF_CACHE", _DEFAULT_REF_CACHE)


def is_garbage_bbox(bbox: Optional[list]) -> bool:
    """True if bbox contains sentinel/unrealized values or zero/negative size.

    Unrealized AT-SPI2 elements use placeholder values like x=-2147483643
    (INT_MIN-ish).  Filter condition: x or y outside [-32768, 32768], or
    w ≤ 0, or h ≤ 0.
    """
    if not bbox or len(bbox) < 4:
        return True
    x, y, w, h = bbox[0], bbox[1], bbox[2], bbox[3]
    return (x < -32768 or x > 32768 or
            y < -32768 or y > 32768 or
            w <= 0 or h <= 0)


def read_ref_cache() -> list[dict]:
    """Read the tine ref cache sidecar.  Returns a flat element list with
    window-relative bboxes (callers must add window origin for screen coords).

    Skips metadata keys (prefixed with "_").  Returns [] on missing / corrupt
    cache — callers fall back to parse_text_tree().
    """
    path = _ref_cache_path()
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError):
        return []

    elements: list[dict] = []
    for key, val in data.items():
        if key.startswith("_") or not isinstance(val, dict):
            continue
        bbox: Optional[list[int]] = None
        bbox_raw = val.get("bbox")
        if isinstance(bbox_raw, dict):
            try:
                bbox = [int(bbox_raw["x"]), int(bbox_raw["y"]),
                        int(bbox_raw["w"]), int(bbox_raw["h"])]
            except (KeyError, TypeError, ValueError):
                bbox = None
        elements.append({
            "ref":   key,
            "role":  str(val.get("role", "")).strip(),
            "label": str(val.get("name", "")).strip(),
            "state": "",
            "bbox":  bbox,
        })
    return elements


# ── tine invocation ───────────────────────────────────────────────────────────
def tine_text(app: Optional[str] = None) -> str:
    """Run `tine.cli tree [--app APP]`.  Populates the ref cache sidecar.
    Returns raw text-tree output, or empty string on failure.
    """
    cmd = list(TINE_CMD)
    if app:
        cmd += ["--app", app]
    ok, out, _ = _run(cmd)
    return out if ok else ""


# Parser for text-tree lines — fallback when ref cache is absent/empty.
_TEXT_LINE_RE = re.compile(
    r"\[(?P<ref>[\w.-]+)\]\s+"
    r"(?P<role>[a-z][a-z\s-]*?)\s+"
    r'"(?P<label>[^"]*)"'
    r"(?:\s+state=(?P<state>\S+))?"
    r"(?:\s+bbox=(?P<bbox>\d+(?:,\d+){3}))?"
)


def parse_text_tree(text: str) -> list[dict]:
    """Best-effort parser for tine's text-tree format. Returns a flat element list."""
    elements: list[dict] = []
    for line in text.splitlines():
        m = _TEXT_LINE_RE.search(line)
        if not m:
            continue
        bbox = None
        if m.group("bbox"):
            try:
                bbox = [int(x) for x in m.group("bbox").split(",")]
            except ValueError:
                bbox = None
        elements.append({
            "ref":   m.group("ref"),
            "role":  m.group("role").strip(),
            "label": m.group("label"),
            "state": m.group("state") or "",
            "bbox":  bbox,
        })
    return elements


# ── Focused window detection ──────────────────────────────────────────────────
def get_focused_window() -> tuple[Optional[str], Optional[dict]]:
    """Return (title, {x,y,width,height}) for the active window, or (None, None)."""
    if not _have("xdotool"):
        return (None, None)

    ok, wid_out, _ = _run(["xdotool", "getactivewindow"], timeout=XDOTOOL_TIMEOUT)
    if not ok or not wid_out.strip():
        return (None, None)
    wid = wid_out.strip()

    _, title_out, _ = _run(["xdotool", "getwindowname", wid], timeout=XDOTOOL_TIMEOUT)
    title = title_out.strip() or None

    ok, geo_out, _ = _run(["xdotool", "getwindowgeometry", "--shell", wid],
                           timeout=XDOTOOL_TIMEOUT)
    if not ok:
        return (title, None)

    geo: dict[str, int] = {}
    for line in geo_out.splitlines():
        if "=" not in line:
            continue
        k, v = line.split("=", 1)
        k = k.strip().lower()
        if k in ("x", "y", "width", "height"):
            try:
                geo[k] = int(v.strip())
            except ValueError:
                pass
    if not all(k in geo for k in ("x", "y", "width", "height")):
        return (title, None)
    return (title, geo)


def _get_classname() -> Optional[str]:
    """Return the WM_CLASS of the active window via xdotool, or None."""
    if not _have("xdotool"):
        return None
    ok, out, _ = _run(["xdotool", "getactivewindow", "getwindowclassname"],
                      timeout=XDOTOOL_TIMEOUT)
    return out.strip() if ok and out.strip() else None


# ── Filtering ─────────────────────────────────────────────────────────────────
def is_interactive(role: str) -> bool:
    return role.strip().lower() in INTERACTIVE_ROLES


def is_noise(role: str) -> bool:
    return role.strip().lower() in NOISE_ROLES


def bbox_inside(bbox: Optional[list[int]], win: Optional[dict], slack: int = 8) -> bool:
    """True if the element's centre point sits within the window's rectangle."""
    if not bbox or len(bbox) < 4 or not win:
        return False
    ex, ey, ew, eh = bbox
    cx, cy = ex + ew // 2, ey + eh // 2
    return (win["x"] - slack <= cx <= win["x"] + win["width"]  + slack and
            win["y"] - slack <= cy <= win["y"] + win["height"] + slack)


def position_hint(bbox: list[int], win: dict) -> str:
    """Window-relative coarse position: top-left, center, bottom-right, etc."""
    if not bbox or not win or win["width"] <= 0 or win["height"] <= 0:
        return ""
    ex, ey, ew, eh = bbox
    cx = (ex + ew // 2) - win["x"]
    cy = (ey + eh // 2) - win["y"]
    fx = cx / win["width"]
    fy = cy / win["height"]
    horiz = "left"  if fx < 0.34 else ("right"  if fx > 0.66 else "center")
    vert  = "top"   if fy < 0.34 else ("bottom" if fy > 0.66 else "middle")
    if horiz == "center" and vert == "middle":
        return "center"
    return f"{vert}-{horiz}"


# ── Output formatters ─────────────────────────────────────────────────────────
def format_focused(elements: list[dict], window_title: str,
                   win_geo: Optional[dict]) -> str:
    """Text format consumed by the agent loop in focused mode.

    Each element line:  ref_N  role  "label"  (sx,sy,w,h)  [hint]  state=...
    Bboxes in elements must already be screen-absolute when win_geo is provided.
    When win_geo is None, no coord tuple or position hint is emitted.
    """
    lines: list[str] = []
    lines.append(f"[focused: {window_title or '(unknown)'}]")
    if win_geo:
        lines.append(f"[window: x={win_geo['x']} y={win_geo['y']} "
                     f"w={win_geo['width']} h={win_geo['height']}]")

    shown = 0
    for e in elements:
        if shown >= MAX_ELEMENTS_FOCUS:
            lines.append(f"… {len(elements) - shown} more elements truncated …")
            break

        ref   = e.get("ref",   "")
        role  = e.get("role",  "")
        label = e.get("label", "")
        bbox  = e.get("bbox")
        state = e.get("state", "")

        parts = [f"{ref:>10}", f"{role:<14}", f'"{label}"']
        if bbox:
            parts.append(f"({bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]})")
            if win_geo:
                hint = position_hint(bbox, win_geo)
                if hint:
                    parts.append(f"[{hint}]")
        if state:
            parts.append(f"state={state}")
        lines.append("  ".join(parts))
        shown += 1

    return "\n".join(lines)


def emit_default_json(elements: list[dict], apps: list[str],
                      terminals: list[dict],
                      interactive_only: bool = False) -> str:
    if interactive_only:
        elements = [e for e in elements if is_interactive(e["role"])]
    elements = [e for e in elements if not is_noise(e["role"])]
    elements = elements[:MAX_ELEMENTS_FULL]
    return json.dumps({
        "apps":      apps,
        "elements":  elements,
        "terminals": terminals[:5],
    }, indent=2)


# ── Modes ─────────────────────────────────────────────────────────────────────
def gather_all(app: Optional[str] = None) -> tuple[list[dict], list[str], list[dict]]:
    """Run tine tree [--app APP], read the ref cache sidecar.
    Returns (elements, apps, terminals=[]).

    Bboxes in elements are WINDOW-RELATIVE.  Callers that need screen coords
    must add the window origin.  Falls back to parsing the text output if the
    ref cache is unreadable (text-tree parse yields no bbox data).
    """
    raw = tine_text(app)          # runs tine, populates ref cache sidecar
    elements = read_ref_cache()
    if not elements:
        elements = parse_text_tree(raw)   # fallback: no bbox data
    apps = sorted({e.get("app", "") for e in elements if e.get("app")})
    return (elements, apps, [])


def mode_focused() -> str:
    title, win    = get_focused_window()
    classname     = _get_classname()

    if not classname or not win:
        # Bootstrap fallback: nothing is focused (fresh desktop, no windows yet).
        # The desktop panel is the only actionable surface, and its AT-SPI frame
        # origin IS the screen origin — so for panel elements, window-relative
        # bboxes are already screen-absolute. Scope the tine run to the panel so
        # every ref in the cache is a panel element, and emit those coordinates.
        # Without this the agent can see panel buttons but never reach them —
        # locked out of the very click that would bootstrap a session.
        raw = tine_text(app=PANEL_APP)
        elements = read_ref_cache()
        if not elements:
            elements = parse_text_tree(raw)
        usable = [
            e for e in elements
            if e.get("bbox") and not is_garbage_bbox(e["bbox"])
        ]
        scoped = [e for e in usable if is_interactive(e.get("role", ""))]
        return format_focused(scoped, title or "(desktop)", None)

    # Scoped run: tine filters to the active app by classname.
    raw = tine_text(app=classname)
    elements = read_ref_cache()
    if not elements:
        elements = parse_text_tree(raw)

    # Convert window-relative bboxes → screen-absolute by adding the window origin.
    # Garbage bboxes (INT_MIN-ish placeholders) are filtered BEFORE conversion.
    # In focused mode, elements with no valid bbox are dropped (unclickable = noise).
    #
    # PANEL EXCEPTION: the panel's frame origin IS the screen origin, so its bboxes
    # are already screen-absolute. When the focused app is the panel itself (e.g.
    # right after clicking it), adding the active window's origin would double-offset
    # every coordinate — observed live: Applications at x=1068 on a 1280px screen.
    if classname.lower() == PANEL_APP.lower():
        win_x, win_y = 0, 0
    else:
        win_x, win_y = win["x"], win["y"]
    screen_elements: list[dict] = []
    for e in elements:
        bbox = e.get("bbox")
        if not bbox or is_garbage_bbox(bbox):
            continue
        screen_bbox = [bbox[0] + win_x, bbox[1] + win_y, bbox[2], bbox[3]]
        screen_elements.append({**e, "bbox": screen_bbox})

    scoped = [e for e in screen_elements if is_interactive(e.get("role", ""))]
    return format_focused(scoped, title or "", win)


def mode_print_focus() -> str:
    title, _ = get_focused_window()
    return title or "(no focused window detected)"


def mode_text() -> str:
    """Terminal text content — not available via tine ref cache."""
    return ""


def mode_raw() -> str:
    return tine_text()


def mode_default(interactive_only: bool) -> str:
    elements, apps, terminals = gather_all()
    return emit_default_json(elements, apps, terminals,
                             interactive_only=interactive_only)


# ── CLI ───────────────────────────────────────────────────────────────────────
def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description="AT-SPI2 screen reader for the Laputa agent.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--focused",          action="store_true",
                      help="Output focused window's interactive elements only (text format).")
    mode.add_argument("--interactive-only", action="store_true",
                      help="Default JSON, but filter out non-interactive elements.")
    mode.add_argument("--text",             action="store_true",
                      help="Terminal text content only.")
    mode.add_argument("--raw",              action="store_true",
                      help="Raw tine.cli tree output.")
    mode.add_argument("--print-focus",      action="store_true",
                      help="Print the focused window title and exit.")
    return ap.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.focused:
            sys.stdout.write(mode_focused())
        elif args.interactive_only:
            sys.stdout.write(mode_default(interactive_only=True))
        elif args.text:
            sys.stdout.write(mode_text())
        elif args.raw:
            sys.stdout.write(mode_raw())
        elif args.print_focus:
            sys.stdout.write(mode_print_focus())
        else:
            sys.stdout.write(mode_default(interactive_only=False))
        sys.stdout.write("\n")
        return 0
    except Exception as e:
        print(f"perceive.py error: {e}", file=sys.stderr)
        sys.stdout.write('{"apps":[],"elements":[],"terminals":[]}\n')
        return 0


if __name__ == "__main__":
    sys.exit(main())
