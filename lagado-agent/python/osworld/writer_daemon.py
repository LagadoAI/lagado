"""writer_daemon.py — resident UNO session for the Writer plane, mirroring uno_daemon.py's role
for Calc (LAGADO_NATIVE_SESSION_PLANE_v1 §3-4, adapted to com.sun.star.text.TextDocument).

Same non-authoritative-cache design as the Calc daemon: holds a live headless LibreOffice + an
open Writer Component, serves single-op verbs over a line-delimited JSON socket. The host (or
battery_writer.py's own driver loop) owns the authoritative op log; this process holds only a
local mirror for sanity, so a crash loses nothing (replay from the original file).

SAFETY (host-dev hazard, unchanged from uno_daemon.py): manages ONLY its own soffice — a Popen
handle on a DEDICATED port + DEDICATED UserInstallation profile. NEVER a global `pkill soffice`.

Runs on its OWN socket (default /tmp/lagado_writer_daemon.sock) and its OWN UNO port so it can
coexist with a concurrently-running Calc daemon in the same guest without collision.
uno_client.py (transport-generic — takes any --sock=) is reused UNCHANGED as the client.

Protocol (request -> response), identical verb names to uno_daemon.py where the shape carries
over, plus Writer-specific read surfaces the falsifiers in battery_writer.py are built against:
  open      {file}                    -> {ok, structure?}
  apply     {op}                      -> {ok, error?}
  read      {what, index?, match?}    -> {ok, ...}     see op_read for the `what` dialects
  structure {}                        -> {ok, paragraphs, n_tables, n_images}
  health    {}                        -> {ok, soffice_alive, doc_open, file}
  reconcile {gui?}                    -> {ok}           store; release lock; optional GUI reload
  close     {}                        -> {ok}
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
import writer_ops  # noqa: E402

DEFAULT_SOCK = "/tmp/lagado_writer_daemon.sock"
DEFAULT_UNO_PORT = 2072   # distinct from uno_daemon.py's 2002 — coexistence, not replacement

FILTER_BY_EXT = {
    ".docx": "MS Word 2007 XML",
    ".doc": "MS Word 97",
    ".odt": "writer8",
    ".rtf": "Rich Text Format",
}

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
    ext = os.path.splitext(path)[1].lower()
    return FILTER_BY_EXT.get(ext, "MS Word 2007 XML")


_ADJUST_NAME = {0: "left", 1: "right", 2: "justify", 3: "center", 4: "justify"}


def _adjust_name(v):
    v = writer_ops._enum_val(v)
    if isinstance(v, str):
        return v.lower()
    return _ADJUST_NAME.get(int(v), "left")


class Daemon:
    def __init__(self, uno_port=DEFAULT_UNO_PORT):
        self.uno_port = uno_port
        self.profile_dir = "/tmp/lagado_writer_daemon_profile_%d" % os.getpid()
        self.profile = "file://" + self.profile_dir
        self.proc = None
        self.ctx = None
        self.desktop = None
        self.doc = None
        self.file = None
        self.op_log = []

    # ── soffice lifecycle (owned-process only, same discipline as uno_daemon.py) ────
    def _start_soffice(self):
        if self.proc is not None and self.proc.poll() is None:
            return
        try:
            udir = os.path.join(self.profile_dir, "user")
            os.makedirs(udir, exist_ok=True)
            rmf = os.path.join(udir, "registrymodifications.xcu")
            if not os.path.exists(rmf):
                open(rmf, "w").write(_RECOVERY_OFF_XCU)
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
            writer_ops.apply_writer_op(self.doc, op)
            self.op_log.append(op)
            resp = {"ok": True}
            if "_matched" in op:   # scope/match diagnostics — see writer_ops.py's op["_matched"] convention
                resp["matched"] = op["_matched"]
            return resp
        except Exception as e:
            return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}

    def _paragraph_info(self, para, idx):
        ls = para.ParaLineSpacing
        return {"idx": idx, "text": para.getString(), "style": para.ParaStyleName or "",
                "align": _adjust_name(para.ParaAdjust), "ls_mode": int(ls.Mode), "ls_height": int(ls.Height),
                "page_style": getattr(para, "PageDescName", "") or ""}

    def _portion_info(self, portion):
        return {"text": portion.getString(),
                "font": portion.CharFontName,
                "size": round(float(portion.CharHeight), 2),
                "bold": float(portion.CharWeight) >= 150.0,
                "italic": writer_ops._enum_val(portion.CharPosture) == "ITALIC",
                "underline": int(portion.CharUnderline) != 0,
                "strike": int(portion.CharStrikeout) != 0,
                "color": int(portion.CharColor),
                "highlight": int(portion.CharHighlight),
                "escapement": int(portion.CharEscapement),
                "escapement_height": int(portion.CharEscapementHeight)}

    def op_read(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        what = req.get("what", "doc_text")
        paras = writer_ops._paragraphs(self.doc)
        if what == "doc_text":
            return {"ok": True, "text": self.doc.Text.getString()}
        if what == "paragraph":
            idx = int(req.get("index", 1))
            if not (1 <= idx <= len(paras)):
                return {"ok": False, "error": "paragraph index out of range"}
            return {"ok": True, **self._paragraph_info(paras[idx - 1], idx)}
        if what == "portions":
            idx = int(req.get("index", 1))
            if not (1 <= idx <= len(paras)):
                return {"ok": False, "error": "paragraph index out of range"}
            out = []
            pit = paras[idx - 1].createEnumeration()
            while pit.hasMoreElements():
                out.append(self._portion_info(pit.nextElement()))
            return {"ok": True, "portions": out}
        if what == "match":
            needle = str(req.get("match") or "")
            if not needle:
                return {"ok": True, "count": 0, "contexts": [], "paragraphs": []}
            sd = self.doc.createSearchDescriptor()
            sd.SearchString = needle
            sd.SearchCaseSensitive = True
            found = self.doc.findAll(sd)
            n = found.getCount()
            ctx, pidx = [], []
            for i in range(min(n, 5)):
                rng = found.getByIndex(i)
                ctx.append(rng.getString())
                pidx.append(self._paragraph_index_of(paras, rng))
            return {"ok": True, "count": n, "contexts": ctx, "paragraphs": pidx}
        if what == "counts":
            n_tables, n_images = self._counts()
            return {"ok": True, "tables": n_tables, "images": n_images}
        if what == "page_areas":
            # The PageNumber field's RENDERED text is empty until the doc is laid out/paginated
            # (MEASURED, 2026-07-10: getPresentation(False) on a freshly-inserted field returns ""
            # headless, even though the field genuinely exists) — the checkable fact headless is
            # STRUCTURAL presence of a page-number field, not its computed digits.
            style = writer_ops._page_style(self.doc)
            return {"ok": True, "header_on": bool(style.HeaderIsOn), "footer_on": bool(style.FooterIsOn),
                    "header_text": style.HeaderText.getString() if style.HeaderIsOn else "",
                    "footer_text": style.FooterText.getString() if style.FooterIsOn else "",
                    "header_has_page_field": self._has_page_field(style.HeaderText) if style.HeaderIsOn else False,
                    "footer_has_page_field": self._has_page_field(style.FooterText) if style.FooterIsOn else False}
        if what == "default_font":
            styles = self.doc.StyleFamilies.getByName("ParagraphStyles")
            for name in ("Default Paragraph Style", "Standard"):
                if styles.hasByName(name):
                    return {"ok": True, "font": styles.getByName(name).CharFontName}
            return {"ok": False, "error": "no default paragraph style found"}
        return {"ok": False, "error": "unknown read `what`: %r" % what}

    def _has_page_field(self, area_text):
        pit = area_text.createEnumeration()
        while pit.hasMoreElements():
            para = pit.nextElement()
            if not para.supportsService("com.sun.star.text.Paragraph"):
                continue
            portions = para.createEnumeration()
            while portions.hasMoreElements():
                portion = portions.nextElement()
                if portion.TextPortionType == "TextField" and \
                   portion.TextField.supportsService("com.sun.star.text.TextField.PageNumber"):
                    return True
        return False

    def _paragraph_index_of(self, paras, rng):
        """1-based index of the paragraph containing text range `rng` (or None) — used to point a
        found match back at a concrete paragraph for a follow-up portions readback.
        VERIFIED sign convention (probe, 2026-07-10): compareRegionStarts/Ends(a, b) is POSITIVE
        when `a` occurs BEFORE `b` (the opposite of a plain a-minus-b comparator) — so containment
        is start>=0 (para starts at/before the range) AND end<=0 (para ends at/after the range)."""
        cs, ce = self.doc.Text.compareRegionStarts, self.doc.Text.compareRegionEnds
        for i, para in enumerate(paras, start=1):
            try:
                if cs(para.Start, rng.Start) >= 0 and ce(para.End, rng.End) <= 0:
                    return i
            except Exception:
                continue
        return None

    def _counts(self):
        n_tables = n_images = 0
        it = self.doc.Text.createEnumeration()
        while it.hasMoreElements():
            el = it.nextElement()
            if el.supportsService("com.sun.star.text.TextTable"):
                n_tables += 1
            elif el.supportsService("com.sun.star.text.Paragraph"):
                pit = el.createEnumeration()
                while pit.hasMoreElements():
                    portion = pit.nextElement()
                    if portion.TextPortionType == "Frame":
                        n_images += 1
        return n_tables, n_images

    def _structure(self):
        paras = writer_ops._paragraphs(self.doc)
        n_tables, n_images = self._counts()
        return {"paragraphs": [self._paragraph_info(p, i) for i, p in enumerate(paras, start=1)],
                "n_tables": n_tables, "n_images": n_images}

    def op_structure(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        return {"ok": True, **self._structure()}

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
            # Isolated profile for the GUI reload — same reasoning as uno_daemon.py's reconcile:
            # a default-profile reload would single-instance-collide with the evaluator's own
            # LibreOffice invocation (--convert-to or activate+ctrl+s).
            gui_profile = "file:///tmp/lagado_writer_reconcile_gui_profile"
            subprocess.Popen(
                ["soffice", "--writer", "-env:UserInstallation=%s" % gui_profile,
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
