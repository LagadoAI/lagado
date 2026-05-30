#!/usr/bin/env python3
"""perceive.py — AT-SPI2 screen reader for the Laputa agent.

Calls `python3 -m tine.cli tree` to read the desktop accessibility tree and
emits a representation tuned to the active mode.

Modes (mutually exclusive; first one specified wins):
  (default)            Full desktop JSON: {"apps":[...], "elements":[...], "terminals":[...]}.
                       Backward-compatible with prior behaviour. No position hints.
  --focused            Text dump of ONLY the focused window's interactive elements,
                       one per line, with screen-relative bounding boxes and
                       window-relative position hints. This is the format
                       intended for the agent loop.
  --interactive-only   Same JSON shape as default, but non-interactive elements
                       removed across all apps.
  --text               Terminal text content only (existing behaviour).
  --raw                Raw `tine.cli tree` output (existing behaviour).
  --print-focus        Print the focused window title and exit (dry-run).

All subprocess calls use a hard timeout so a wedged desktop can't hang the agent.

Output is always UTF-8 text on stdout. Errors and diagnostics go to stderr.
"""

import argparse
import json
import re
import shutil
import subprocess
import sys
from typing import Any, Optional

# ── Configuration ───────────────────────────────────────────────────────────
TINE_CMD = ["env", "PYTHONPATH=/home/laputa/tine", "python3", "-m", "tine.cli", "tree"]
TINE_JSON_CMD       = ["python3", "-m", "tine.cli", "tree", "--json"]
SUBPROCESS_TIMEOUT  = 5      # seconds
XDOTOOL_TIMEOUT     = 2      # seconds
MAX_ELEMENTS_FULL   = 120    # cap for full-desktop output (existing behaviour)
MAX_ELEMENTS_FOCUS  = 50     # cap for focused-mode output

# AT-SPI2 roles that are actionable / worth keeping.
# Roles are normalised to lowercase before lookup.
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
    # containers we treat as click targets in app launchers / file managers
    "icon", "tool tip", "tool bar",
    # date / color pickers
    "date editor", "color chooser",
}

# Roles we explicitly drop as visual noise even in full mode.
NOISE_ROLES = {"separator", "filler", "layered pane", "redundant object"}


# ── Subprocess helpers ──────────────────────────────────────────────────────
def _have(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def _run(cmd: list[str], timeout: int = SUBPROCESS_TIMEOUT) -> tuple[bool, str, str]:
    """Run a subprocess. Returns (ok, stdout, stderr). Never raises."""
    try:
        cp = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        return (cp.returncode == 0, cp.stdout or "", cp.stderr or "")
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError) as e:
        return (False, "", str(e))


# ── tine output ─────────────────────────────────────────────────────────────
def tine_json() -> Optional[dict[str, Any]]:
    """Try `tine.cli tree --json`. Returns parsed dict or None."""
    ok, out, _ = _run(TINE_JSON_CMD)
    if not ok or not out.strip():
        return None
    try:
        data = json.loads(out)
        return data if isinstance(data, dict) else None
    except json.JSONDecodeError:
        return None


def tine_text() -> str:
    """Raw text-tree output from tine. Empty string on failure."""
    ok, out, _ = _run(TINE_CMD)
    return out if ok else ""


# Parser for text-tree lines like:
#   [ref_42] button "Submit" state=enabled bbox=100,200,80,30
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


def normalize_elements(raw: Any) -> list[dict]:
    """Convert tine JSON elements (whatever shape) into our flat dict form."""
    if not isinstance(raw, list):
        return []
    out = []
    for e in raw:
        if not isinstance(e, dict):
            continue
        bbox = e.get("bbox") or e.get("bounding_box") or e.get("extents")
        if isinstance(bbox, dict):
            # convert {"x":..,"y":..,"width":..,"height":..} into list form
            try:
                bbox = [int(bbox["x"]), int(bbox["y"]),
                        int(bbox["width"]), int(bbox["height"])]
            except (KeyError, TypeError, ValueError):
                bbox = None
        elif isinstance(bbox, (list, tuple)) and len(bbox) >= 4:
            try:
                bbox = [int(x) for x in bbox[:4]]
            except (TypeError, ValueError):
                bbox = None
        else:
            bbox = None

        out.append({
            "ref":   str(e.get("ref")  or e.get("id")   or ""),
            "role":  str(e.get("role") or "").strip(),
            "label": str(e.get("label") or e.get("name") or ""),
            "state": str(e.get("state") or ""),
            "app":   str(e.get("app")   or e.get("application") or ""),
            "bbox":  bbox,
        })
    return out


# ── Focused window detection ────────────────────────────────────────────────
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


# ── Filtering ───────────────────────────────────────────────────────────────
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


# ── Output formatters ───────────────────────────────────────────────────────
def format_focused(elements: list[dict], window_title: str,
                   win_geo: Optional[dict]) -> str:
    """Text format consumed by the agent loop in focused mode."""
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


# ── Modes ───────────────────────────────────────────────────────────────────
def gather_all() -> tuple[list[dict], list[str], list[dict]]:
    """Try JSON first, fall back to parsing text tree. Returns (elements, apps, terminals)."""
    data = tine_json()
    if data is not None:
        elements  = normalize_elements(data.get("elements", []))
        apps      = list(data.get("apps", []))
        terminals = list(data.get("terminals", []))
        return (elements, apps, terminals)

    # Fallback: parse text tree, no terminal extraction possible
    raw_text = tine_text()
    elements = parse_text_tree(raw_text)
    apps = sorted({e.get("app", "") for e in elements if e.get("app")})
    return (elements, apps, [])


def mode_focused() -> str:
    title, win = get_focused_window()
    elements, _apps, _terms = gather_all()

    if not title and not win:
        # Couldn't detect focus — gracefully fall back to default JSON so the
        # agent still gets something useful.
        print("perceive.py: focused window detection failed, falling back to "
              "default JSON output", file=sys.stderr)
        return emit_default_json(elements, _apps, _terms,
                                 interactive_only=True)

    # Filter to elements within the focused window's geometry (if available)
    if win:
        scoped = [e for e in elements if e.get("bbox") is None or bbox_inside(e.get("bbox"), win)]
    else:
        # Fall back to app-name matching if we only have a title
        scoped = elements

    scoped = [e for e in scoped if is_interactive(e.get("role", ""))]
    return format_focused(scoped, title or "", win)


def mode_print_focus() -> str:
    title, _ = get_focused_window()
    return title or "(no focused window detected)"


def mode_text() -> str:
    """Terminal text content extracted from the tree."""
    _, _, terminals = gather_all()
    blobs = []
    for t in terminals:
        app  = t.get("app", "")
        text = t.get("text", "")
        if text:
            blobs.append(f"--- {app} ---\n{text}")
    return "\n\n".join(blobs)


def mode_raw() -> str:
    return tine_text()


def mode_default(interactive_only: bool) -> str:
    elements, apps, terminals = gather_all()
    return emit_default_json(elements, apps, terminals,
                             interactive_only=interactive_only)


# ── CLI ─────────────────────────────────────────────────────────────────────
def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser(
        description="AT-SPI2 screen reader for the Laputa agent.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    # Mutually exclusive group so flags can't be combined in confusing ways.
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
        # Never crash the agent — always return something
        print(f"perceive.py error: {e}", file=sys.stderr)
        sys.stdout.write('{"apps":[],"elements":[],"terminals":[]}\n')
        return 0


if __name__ == "__main__":
    sys.exit(main())
