"""uno_daemon.py — resident UNO session over a host-owned op log.

The native-session-plane guest daemon (LAGADO_NATIVE_SESSION_PLANE_v1 §3-4). It holds
a live headless LibreOffice + an open Calc Component, and serves single-op verbs over a
line-delimited JSON socket so the host agent can drive the doc ONE op at a time WITH
per-op observation (`read`/`structure`) — the capability the stateless one-shot lacks.

CENTRAL DESIGN (§1): this daemon is a NON-AUTHORITATIVE, replayable CACHE. The host owns
the authoritative op log; the daemon holds only a local mirror for sanity. Anything it
holds is reconstructable as apply(op_log) to a fresh load of the original file. So a crash
loses nothing — the host replays. The daemon adds exactly one thing: cheap live reads.

SAFETY (host-dev hazard, do NOT regress): this process manages ONLY ITS OWN soffice —
launched via a Popen handle, on a DEDICATED UNO port and a DEDICATED UserInstallation
profile. It NEVER runs a global `pkill soffice` (which would kill a user's LibreOffice on
a dev host). Teardown terminates the owned Popen and removes only the owned lock/profile.

Transport: a Unix-domain socket; the host invokes `uno_client.py <verb> <json>` (via the
OSWorld `/execute` channel) which opens the socket, sends one request line, reads one
response line. One in-flight request at a time (the line protocol is serial).

Protocol (request -> response), all JSON, one per line:
  open      {file}            -> {ok, structure?}      load/relaod doc; identity-guarded
  apply     {op}             -> {ok, error?}          one op to the live doc + local-mirror append
  read      {sheet, range}   -> {ok, cells}           live values (the effect-sensor)
  structure {}               -> {ok, sheets, headers, extents}
  health    {}               -> {ok, soffice_alive, doc_open, file}
  reconcile {gui?}           -> {ok}                   store xlsx; release lock; optional GUI reload
  close     {}               -> {ok}                   teardown: kill own soffice, rm lock, exit
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

# Shared single apply implementation (lives beside this file).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import uno_ops  # noqa: E402

DEFAULT_SOCK = "/tmp/lagado_uno_daemon.sock"
DEFAULT_UNO_PORT = 2002
XLSX_FILTER = "Calc MS Excel 2007 XML"

# Pre-seeded into our owned profile to turn crash recovery + autosave OFF (kills the "Document Recovery"
# dialog on a VISIBLE relaunch after a hard kill). Owned-profile only — never touches the user's LibreOffice.
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


class Daemon:
    def __init__(self, uno_port=DEFAULT_UNO_PORT):
        self.uno_port = uno_port
        # Dedicated, owned profile — isolates our office instance from any other (incl. a
        # user's LibreOffice on a dev host). Keyed to our PID so two daemons never share.
        self.profile_dir = "/tmp/lagado_uno_daemon_profile_%d" % os.getpid()
        self.profile = "file://" + self.profile_dir
        self.proc = None          # our soffice Popen — the ONLY process we ever kill
        self.ctx = None
        self.desktop = None
        self.doc = None
        self.file = None
        self.op_log = []          # local mirror; the host holds the authoritative log

    # ── soffice lifecycle (owned-process only) ──────────────────────────────────
    def _start_soffice(self):
        if self.proc is not None and self.proc.poll() is None:
            return
        # Suppress the "Document Recovery" dialog: we may hard-kill a prior soffice, which otherwise makes the
        # next (VISIBLE) launch open on a recovery prompt instead of the doc. Pre-seed a registrymodifications.xcu
        # that turns crash recovery + autosave OFF. Harmless headless; only matters when a human is watching.
        try:
            _udir = os.path.join(self.profile_dir, "user")
            os.makedirs(_udir, exist_ok=True)
            _rmf = os.path.join(_udir, "registrymodifications.xcu")
            if not os.path.exists(_rmf):
                open(_rmf, "w").write(_RECOVERY_OFF_XCU)
        except Exception:
            pass
        # VISIBLE mode (opt-in via LAGADO_VISIBLE, host-only) shows the real LibreOffice window so a human can
        # WATCH the agent drive the app. Default stays headless/invisible (the VM path never sets the var).
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
        """Kill ONLY our own soffice (by Popen handle). Never a global pkill."""
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
        """Remove the owned per-pid UserInstallation dir — final teardown only, so a sweep of
        tasks doesn't accumulate one profile per daemon (real disk in a 4 GB guest). Kept
        across mid-life office restarts (reconcile) to avoid re-init latency."""
        shutil.rmtree(self.profile_dir, ignore_errors=True)

    def _clear_own_lock(self):
        """Remove the .~lock for the CURRENT file only (our own session's lock)."""
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
        # Identity guard: a request for a DIFFERENT file closes the current doc first.
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
            hidden = not bool(os.environ.get("LAGADO_VISIBLE"))   # show the doc when watching (host-only)
            self.doc = self.desktop.loadComponentFromURL(url, "_blank", 0, (_pv("Hidden", hidden),))
            # Live recompute so a `read` after a formula op returns the computed result, not
            # a stale value. The one-shot relies on an explicit calculateAll() before store;
            # the session reads BETWEEN ops, so it needs automatic calc on.
            try:
                self.doc.enableAutomaticCalculation(True)
            except Exception:
                pass
            self.op_log = []
        return {"ok": True, "structure": self._structure()}

    def op_apply(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        op = req["op"]
        try:
            resolve_sheet = uno_ops.make_resolve_sheet(self.doc)
            uno_ops.apply_one_op(self.doc, resolve_sheet, op)
            self.op_log.append(op)  # local mirror only
            return {"ok": True}
        except Exception as e:
            # Bad op: do NOT commit it to the mirror; report so the host drops it too.
            return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}

    def op_read(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        resolve_sheet = uno_ops.make_resolve_sheet(self.doc)
        sh = resolve_sheet(req.get("sheet"))
        rng = sh.getCellRangeByName(req["range"])
        addr = rng.getRangeAddress()
        cells = []
        for r in range(addr.StartRow, addr.EndRow + 1):
            row = []
            for c in range(addr.StartColumn, addr.EndColumn + 1):
                cell = sh.getCellByPosition(c, r)
                t = cell.getType().value
                if t == "EMPTY":
                    row.append(None)
                elif t == "VALUE":
                    row.append(cell.getValue())
                elif t == "FORMULA":
                    # Return the COMPUTED result (auto-calc is on): numeric formulas as a
                    # number, string formulas as text. FormulaResultType.value mirrors the
                    # getType().value idiom ("VALUE"/"TEXT"/"ERROR"); getString() alone would
                    # give the formatted face ("20") and mask numeric completeness checks.
                    try:
                        frt = cell.FormulaResultType.value
                    except Exception:
                        frt = "VALUE"
                    row.append(cell.getValue() if frt == "VALUE" else cell.getString())
                else:  # TEXT
                    row.append(cell.getString())
            cells.append(row)
        return {"ok": True, "cells": cells}

    def _structure(self):
        sheets = self.doc.Sheets
        out = []
        for i in range(sheets.Count):
            sh = sheets.getByIndex(i)
            cur = sh.createCursor()
            cur.gotoEndOfUsedArea(False)
            addr = cur.getRangeAddress()
            # Header row = the used-area's first row, as strings.
            headers = []
            for c in range(addr.StartColumn, addr.EndColumn + 1):
                headers.append(sh.getCellByPosition(c, addr.StartRow).getString())
            # PERCEIVE each column's number-format CATEGORY from a representative data cell, so the harness
            # can GROUND a result's type (a column of dates is date-typed) instead of reverse-engineering it
            # from a bare serial. com.sun.star.util.NumberFormat: DATE bit = 2 (DATETIME=6 also has it set).
            coltypes, colfmt = [], []
            drow = addr.StartRow + 1 if addr.EndRow > addr.StartRow else addr.StartRow
            try:
                nfmts = self.doc.getNumberFormats()
            except Exception:
                nfmts = None
            for c in range(addr.StartColumn, addr.EndColumn + 1):
                cat, dbg = "text", None
                try:
                    cell = sh.getCellByPosition(c, drow)
                    if cell.getType().value in ("VALUE", "FORMULA") and nfmts is not None:
                        props = nfmts.getByKey(cell.NumberFormat)
                        ftype = int(props.Type)
                        fstr = str(props.FormatString)
                        dbg = [ftype, fstr]
                        # Robust date test: the UNO Type bit (DATE=2) OR the format STRING carries date tokens
                        # (what openpyxl effectively keys on). Belt-and-suspenders — the Type bitmask has been
                        # observed to miss imported xlsx date formats ("mm-dd-yy" came back non-date).
                        u = fstr.upper()
                        is_date = bool(ftype & 2) or ("MMM" in u) or ("YY" in u) or ("D" in u and "Y" in u)
                        cat = "date" if is_date else "number"
                except Exception as e:
                    cat, dbg = "text", ["err", str(e)[:60]]
                coltypes.append(cat)
                colfmt.append(dbg)
            out.append({
                "name": sh.Name,
                "extent": {"cols": addr.EndColumn + 1, "rows": addr.EndRow + 1},
                "headers": headers,
                "coltypes": coltypes,
                "colfmt": colfmt,
            })
        return {"sheets": [s["name"] for s in out], "detail": out}

    def op_structure(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        s = self._structure()
        return {"ok": True, "sheets": s["sheets"], "detail": s["detail"]}

    def op_health(self, req):
        return {"ok": True, "soffice_alive": self.soffice_alive(),
                "doc_open": self.doc is not None, "file": self.file,
                "ops_applied": len(self.op_log)}

    def op_reconcile(self, req):
        if self.doc is None:
            return {"ok": False, "error": "no doc open"}
        try:
            self.doc.calculateAll()
            url = uno.systemPathToFileUrl(os.path.abspath(self.file))
            self.doc.storeToURL(url, (_pv("FilterName", XLSX_FILTER),))
            self.doc.close(False)
        except Exception as e:
            return {"ok": False, "error": "store failed: %s" % e}
        # Freeze is VIEW state — a headless store drops it (no view). Re-impose any logged
        # freeze_panes onto the SAVED file (stdlib zip patch; idempotent, last op wins). Done
        # post-store/pre-GUI-reload so the reloaded window (and its ctrl+s re-save) carries it.
        patch_err = None
        for o in self.op_log:
            if o.get("op") == "freeze_panes":
                try:
                    fc, fr = uno_ops.freeze_counts(o)
                    uno_ops.patch_xlsx_freeze(os.path.abspath(self.file), o.get("sheet"), fc, fr)
                except Exception as e:
                    patch_err = "freeze patch: %s" % e   # store succeeded; report, don't fail it
        self.doc = None
        # Release the lock by tearing down OUR headless instance before any GUI attach
        # (single-instance: a survivor would make the GUI reload attach to the headless
        # and never raise its own window).
        saved_file = self.file
        self._terminate_soffice()
        self._clear_own_lock()
        # GUI reload is ONLY for the guest evaluator (activate-by-title + ctrl+s on the
        # live instance). Gated so a dev host never spawns a GUI office. Default OFF.
        gui = req.get("gui")
        if gui is None:
            gui = os.environ.get("LAGADO_RECONCILE_GUI") == "1"
        if gui and os.environ.get("DISPLAY"):
            env = dict(os.environ)
            env.setdefault("DISPLAY", ":0")
            subprocess.Popen(
                ["soffice", "--calc", os.path.abspath(saved_file)],
                env=env, start_new_session=True,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        out = {"ok": True, "reloaded_gui": bool(gui and os.environ.get("DISPLAY"))}
        if patch_err:
            out["patch_err"] = patch_err
        return out

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
    # Signal readiness on stdout so a launcher can wait for the socket to be live.
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
