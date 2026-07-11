"""impress_daemon.py — resident UNO session over a host-owned op log, the Impress analog of
uno_daemon.py. It holds a live headless LibreOffice + an open Impress Component, and serves
single-op verbs over a line-delimited JSON socket so the host agent can drive the deck ONE op
at a time WITH per-op observation (`read`/`structure`).

Same CENTRAL DESIGN as uno_daemon.py (LAGADO_NATIVE_SESSION_PLANE_v1 §3-4): this daemon is a
NON-AUTHORITATIVE, replayable CACHE. The host owns the authoritative op log; a crash loses
nothing (the host replays against a fresh open). Same host-dev SAFETY invariant: this process
manages ONLY ITS OWN soffice (dedicated Popen, dedicated UNO port, dedicated UserInstallation
profile) — never a global `pkill soffice`.

Run as a SEPARATE process from uno_daemon.py (own socket, own UNO port, own profile dir) so a
Calc session and an Impress session can coexist on the same guest without collision — deliberate,
since a benchmark campaign may be exercising the Calc daemon concurrently.

Protocol (identical shape to uno_daemon.py, verbs/fields specialized for slides):
  open      {file}                       -> {ok, structure?}
  apply     {op}                         -> {ok, error?}
  read      {slide, shape}               -> {ok, text, props}   live shape text + char/para props
  structure {}                           -> {ok, slides, detail}
  health    {}                           -> {ok, soffice_alive, doc_open, file}
  reconcile {gui?}                       -> {ok}   store pptx/odp; release lock; optional GUI reload
  close     {}                           -> {ok}
"""

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import time

import uno
from com.sun.star.beans import PropertyValue

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import impress_ops  # noqa: E402

DEFAULT_SOCK = "/tmp/lagado_impress_daemon.sock"
DEFAULT_UNO_PORT = 2003          # distinct from uno_daemon's 2002 — coexistence, not collision
PPTX_FILTER = "Impress MS PowerPoint 2007 XML"
ODP_FILTER = "impress8"

_RECOVERY_OFF_XCU = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<oor:items xmlns:oor="http://openoffice.org/2001/registry" '
    'xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">\n'
    ' <item oor:path="/org.openoffice.Office.Recovery/RecoveryInfo">'
    '<prop oor:name="Enabled" oor:op="fput"><value>false</value></prop></item>\n'
    ' <item oor:path="/org.openoffice.Office.Recovery/AutoSave">'
    '<prop oor:name="Enabled" oor:op="fput"><value>false</value></prop></item>\n'
    '</oor:items>\n')


def _pv(name, value):
    p = PropertyValue()
    p.Name = name
    p.Value = value
    return p


def _filter_for(path):
    ext = path.rsplit(".", 1)[-1].lower() if "." in path else "pptx"
    return ODP_FILTER if ext == "odp" else PPTX_FILTER


class Daemon:
    def __init__(self, uno_port=DEFAULT_UNO_PORT):
        self.uno_port = uno_port
        self.profile_dir = "/tmp/lagado_impress_daemon_profile_%d" % os.getpid()
        self.profile = "file://" + self.profile_dir
        self.proc = None
        self.ctx = None
        self.desktop = None
        self.doc = None
        self.file = None
        self.op_log = []

    # ── soffice lifecycle (owned-process only, identical pattern to uno_daemon.py) ──────────
    def _start_soffice(self):
        if self.proc is not None and self.proc.poll() is None:
            return
        try:
            _udir = os.path.join(self.profile_dir, "user")
            os.makedirs(_udir, exist_ok=True)
            _rmf = os.path.join(_udir, "registrymodifications.xcu")
            if not os.path.exists(_rmf):
                open(_rmf, "w").write(_RECOVERY_OFF_XCU)
        except Exception:
            pass
        visible = bool(os.environ.get("LAGADO_VISIBLE"))
        vis_flags = [] if visible else ["--headless", "--invisible"]
        self.proc = subprocess.Popen(
            ["soffice"] + vis_flags + ["--nodefault", "--norestore",
             "--nologo", "--nofirststartwizard",
             "--accept=socket,host=localhost,port=%d;urp;StarOffice.ComponentContext" % self.uno_port,
             "-env:UserInstallation=%s" % self.profile],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        local = uno.getComponentContext()
        resolver = local.ServiceManager.createInstanceWithContext(
            "com.sun.star.bridge.UnoUrlResolver", local)
        self.ctx = None
        for _ in range(40):
            try:
                self.ctx = resolver.resolve(
                    "uno:socket,host=localhost,port=%d;urp;StarOffice.ComponentContext" % self.uno_port)
                break
            except Exception:
                time.sleep(0.5)
        if self.ctx is None:
            self._terminate_soffice()
            raise RuntimeError("UNO connect FAILED on port %d" % self.uno_port)
        smgr = self.ctx.ServiceManager
        self.desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", self.ctx)

    def _terminate_soffice(self):
        try:
            if self.desktop is not None:
                self.desktop.terminate()
        except Exception:
            pass
        if self.proc is not None:
            try:
                self.proc.send_signal(signal.SIGTERM)
                self.proc.wait(timeout=10)
            except Exception:
                try:
                    self.proc.kill()
                except Exception:
                    pass
        self.proc = None
        self.ctx = None
        self.desktop = None
        self.doc = None

    def _cleanup_profile(self):
        shutil.rmtree(self.profile_dir, ignore_errors=True)

    def _clear_own_lock(self):
        if not self.file:
            return
        d = os.path.dirname(os.path.abspath(self.file))
        b = os.path.basename(self.file)
        try:
            os.remove(os.path.join(d, ".~lock.%s#" % b))
        except OSError:
            pass

    def soffice_alive(self):
        return self.proc is not None and self.proc.poll() is None

    # ── verbs ────────────────────────────────────────────────────────────────────
    def op_open(self, req):
        path = req["file"]
        if self.doc is not None and self.file and os.path.abspath(self.file) != os.path.abspath(path):
            try:
                self.doc.close(False)
            except Exception:
                pass
            self.doc = None
        if not self.soffice_alive():
            self._start_soffice()
        self.file = path
        if self.doc is None:
            url = uno.systemPathToFileUrl(os.path.abspath(path))
            hidden = not bool(os.environ.get("LAGADO_VISIBLE"))
            self.doc = self.desktop.loadComponentFromURL(url, "_blank", 0, (_pv("Hidden", hidden),))
            self.op_log = []
        return {"ok": True, "structure": self._structure()}

    def op_apply(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        op = req["op"]
        try:
            impress_ops.apply_impress_op(self.doc, op)
            self.op_log.append(op)
            return {"ok": True}
        except Exception as e:
            return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}

    def op_read(self, req):
        """Live read-back of ONE shape: its text plus a compact prop summary (font name/size/
        color/bold/underline/strike/align), the falsifier's effect-sensor (mirrors uno_daemon's
        cell `read`). An optional `cell` field ({row, col}, 1-based) reads ONE table cell's
        string instead (set_table_cell's falsifier).

        SCOPE-AWARE (2026-07-10 fix — was a real false-pass hole): an optional `lines` field
        restricts sampling to those 1-based paragraph(s) (matching the WRITE path's `lines`
        scoping in impress_ops._scoped_ranges) — absent/""/"all" samples the WHOLE shape. Each
        property is reported ONLY IF every sampled text portion AGREES on it; a scoped read that
        finds the named lines DISAGREE (e.g. line 1 struck-through, line 2 not, when lines="1,2"
        was asked) reports None for that property rather than one line's value — a disagreement
        must read as a MISMATCH against the expected value, never silently pass as whichever
        portion happened to be sampled first."""
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        try:
            pages = self.doc.DrawPages
            slide = pages.getByIndex(int(req["slide"]) - 1)
            shape_ref = req.get("shape")
            shape = impress_ops.resolve_shape(slide, shape_ref) if shape_ref else None
            if shape is None:
                return {"ok": False, "error": "no shape ref given"}
            cell = req.get("cell")
            if cell:
                c = shape.Model.getCellByPosition(int(cell["col"]) - 1, int(cell["row"]) - 1)
                return {"ok": True, "text": c.getString(), "props": {}}
            text = shape.getString()
            props = {}
            try:
                paras = impress_ops._paragraphs(shape)
                idxs = impress_ops._parse_lines(req.get("lines"))
                target_paras = paras if idxs is None else [paras[i - 1] for i in idxs if 1 <= i <= len(paras)]
                if not target_paras:
                    target_paras = paras
                portions = []
                for p in target_paras:
                    en = p.createEnumeration()
                    while en.hasMoreElements():
                        portions.append(en.nextElement())
                if not portions:
                    portions = [shape.Text]

                def agree(attr, default=None):
                    vals = [getattr(pt, attr, default) for pt in portions]
                    return vals[0] if vals and all(v == vals[0] for v in vals) else None

                color_raw = agree("CharColor")
                bold_raw = agree("CharWeight")
                underline_raw = agree("CharUnderline")
                strike_raw = agree("CharStrikeout")
                align_vals = [getattr(p, "ParaAdjust", 0) for p in target_paras] or [0]
                align_agree = align_vals[0] if all(v == align_vals[0] for v in align_vals) else None
                props = {
                    "font_name": agree("CharFontName"),
                    "size_pt": agree("CharHeight"),
                    "color": ("#%06X" % color_raw) if color_raw is not None else None,
                    "bold": (bold_raw >= 150.0) if bold_raw is not None else None,
                    "underline": bool(underline_raw) if underline_raw is not None else None,
                    "strike": bool(strike_raw) if strike_raw is not None else None,
                    "align": {0: "left", 1: "right", 2: "justify", 3: "center"}.get(align_agree)
                    if align_agree is not None else None,
                    "para_count": len(paras),
                }
            except Exception as e:
                props = {"prop_err": str(e)}
            return {"ok": True, "text": text, "props": props}
        except Exception as e:
            return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}

    def _structure(self):
        """Per-slide observation: title/content/notes text, shape inventory (kind + text preview
        + basic geometry), background/transition presence. The detector's raw material (mirrors
        uno_daemon._structure's per-sheet headers/coltypes)."""
        pages = self.doc.DrawPages
        out = []
        for i in range(pages.Count):
            slide = pages.getByIndex(i)
            shapes = []
            title_text, content_text = "", ""
            for j in range(slide.Count):
                s = slide.getByIndex(j)
                kind = ("title" if impress_ops._is_title(s) else
                        "content" if impress_ops._is_content_placeholder(s) else
                        "table" if impress_ops._is_table(s) else
                        "image" if impress_ops._is_image(s) else
                        "media" if impress_ops._is_media(s) else "textbox")
                txt = impress_ops._shape_text(s)
                if kind == "title":
                    title_text = txt
                elif kind == "content":
                    content_text = txt
                entry = {"kind": kind, "text": txt[:200],
                        "x": s.Position.X, "y": s.Position.Y,
                        "w": s.Size.Width, "h": s.Size.Height}
                if kind == "table":
                    try:
                        entry["rows"] = s.Model.Rows.Count
                        entry["cols"] = s.Model.Columns.Count
                    except Exception:
                        pass
                shapes.append(entry)
            notes_text = ""
            try:
                npage = slide.NotesPage
                for j in range(npage.Count):
                    s = npage.getByIndex(j)
                    if impress_ops._is_notes_shape(s):
                        notes_text = impress_ops._shape_text(s)
                        break
            except Exception:
                pass
            background = None
            try:
                bg = slide.Background
                if bg is not None:
                    background = "#%06X" % bg.FillColor
            except Exception:
                pass
            transition = None
            try:
                transition = [int(slide.TransitionType), int(slide.TransitionSubtype)]
            except Exception:
                pass
            out.append({"index": i + 1, "title": title_text, "content": content_text,
                       "notes": notes_text, "shapes": shapes, "background": background,
                       "transition": transition,
                       "width": slide.Width, "height": slide.Height})
        return {"slides": pages.Count, "detail": out}

    def op_structure(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        s = self._structure()
        return {"ok": True, "slides": s["slides"], "detail": s["detail"]}

    def op_health(self, req):
        return {"ok": True, "soffice_alive": self.soffice_alive(),
                "doc_open": self.doc is not None, "file": self.file,
                "ops_applied": len(self.op_log)}

    def op_reconcile(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        try:
            url = uno.systemPathToFileUrl(os.path.abspath(self.file))
            self.doc.storeToURL(url, (_pv("FilterName", _filter_for(self.file)),))
            self.doc.close(False)
        except Exception as e:
            return {"ok": False, "error": "store failed: %s" % e}
        self.doc = None
        saved_file = self.file
        self._terminate_soffice()
        self._clear_own_lock()
        gui = req.get("gui")
        if gui is None:
            gui = os.environ.get("LAGADO_RECONCILE_GUI") == "1"
        if gui and os.environ.get("DISPLAY"):
            env = dict(os.environ)
            env.setdefault("DISPLAY", ":0")
            gui_profile = "file:///tmp/lagado_reconcile_gui_profile_impress"
            subprocess.Popen(
                ["soffice", "--impress", "-env:UserInstallation=%s" % gui_profile,
                 os.path.abspath(saved_file)],
                env=env, start_new_session=True,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return {"ok": True, "reloaded_gui": bool(gui and os.environ.get("DISPLAY"))}

    def op_close(self, req):
        try:
            if self.doc is not None:
                self.doc.close(False)
        except Exception:
            pass
        self.doc = None
        self._terminate_soffice()
        self._clear_own_lock()
        self._cleanup_profile()
        return {"ok": True}

    DISPATCH = {
        "open": op_open, "apply": op_apply, "read": op_read, "structure": op_structure,
        "health": op_health, "reconcile": op_reconcile, "close": op_close,
    }

    def dispatch(self, req):
        verb = req.get("verb")
        handler = self.DISPATCH.get(verb)
        if handler is None:
            return {"ok": False, "error": "unknown verb: %r" % verb}
        try:
            return handler(self, req)
        except Exception as e:
            return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}


def serve(sock_path, uno_port):
    if os.path.exists(sock_path):
        os.remove(sock_path)
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(sock_path)
    srv.listen(8)
    daemon = Daemon(uno_port=uno_port)
    sys.stdout.write("DAEMON READY %s\n" % sock_path)
    sys.stdout.flush()
    try:
        while True:
            conn, _ = srv.accept()
            try:
                f = conn.makefile("rwb")
                line = f.readline()
                if not line:
                    continue
                try:
                    req = json.loads(line.decode("utf-8"))
                except Exception as e:
                    resp = {"ok": False, "error": "bad json: %s" % e}
                else:
                    resp = daemon.dispatch(req)
                f.write((json.dumps(resp) + "\n").encode("utf-8"))
                f.flush()
                exit_after = req.get("verb") == "close" if isinstance(req, dict) else False
            finally:
                try:
                    conn.close()
                except Exception:
                    pass
            if exit_after:
                break
    finally:
        daemon._terminate_soffice()
        daemon._cleanup_profile()
        try:
            os.remove(sock_path)
        except OSError:
            pass


def main(argv):
    sock_path = DEFAULT_SOCK
    uno_port = DEFAULT_UNO_PORT
    for a in argv:
        if a.startswith("--sock="):
            sock_path = a[len("--sock="):]
        elif a.startswith("--port="):
            uno_port = int(a[len("--port="):])
    serve(sock_path, uno_port)


if __name__ == "__main__":
    main(sys.argv[1:])
