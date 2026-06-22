#!/usr/bin/python3
"""
UNO applier — apply native spreadsheet operations to a real LibreOffice (the host's), using the live
calc engine to compute formulas (faithful to how a human types a formula). Runs under SYSTEM python3
(which has the `uno` module); driven as a subprocess by the .venv orchestrator.

Usage:  /usr/bin/python3 uno_apply.py <xlsx_path> <ops_json_path>
ops = JSON array of:
  {"op":"set","sheet":S,"cell":"J2","formula":"=B2-C2-D2-SUM(F2:H2)"}   # Excel A1 syntax
  {"op":"set","sheet":S,"cell":"A1","value": <number|string>}
  {"op":"add_sheet","name":N,"index":<int optional>}
  {"op":"rename_sheet","old":O,"new":N}
Exit 0 + prints APPLIED on success.
"""
import sys, os, json, time, subprocess, signal

PORT = 2018
PROFILE = "file:///tmp/m2_louser"


def excel_to_calc(f):
    """Minimal Excel-A1 -> Calc-A1 conversion for setFormula: sheet refs '!'->'.', arg sep ','->';'
    (outside string literals)."""
    out, in_str = [], False
    for ch in f:
        if ch == '"':
            in_str = not in_str; out.append(ch)
        elif not in_str and ch == '!':
            out.append('.')
        elif not in_str and ch == ',':
            out.append(';')
        else:
            out.append(ch)
    return "".join(out)


def main():
    path, ops_path = sys.argv[1], sys.argv[2]
    ops = json.load(open(ops_path))

    # 1) launch a private headless LibreOffice with a UNO socket
    soffice = subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--nodefault", "--norestore", "--nologo",
         "--nofirststartwizard",
         "--accept=socket,host=localhost,port=%d;urp;StarOffice.ComponentContext" % PORT,
         "-env:UserInstallation=%s" % PROFILE],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    import uno
    from com.sun.star.beans import PropertyValue

    def pv(n, v):
        p = PropertyValue(); p.Name = n; p.Value = v; return p

    localContext = uno.getComponentContext()
    resolver = localContext.ServiceManager.createInstanceWithContext(
        "com.sun.star.bridge.UnoUrlResolver", localContext)
    ctx = None
    for _ in range(40):
        try:
            ctx = resolver.resolve(
                "uno:socket,host=localhost,port=%d;urp;StarOffice.ComponentContext" % PORT)
            break
        except Exception:
            time.sleep(0.5)
    if ctx is None:
        print("UNO connect FAILED"); soffice.kill(); sys.exit(2)

    smgr = ctx.ServiceManager
    desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    url = uno.systemPathToFileUrl(os.path.abspath(path))
    doc = desktop.loadComponentFromURL(url, "_blank", 0, (pv("Hidden", True),))

    sheets = doc.Sheets
    for op in ops:
        kind = op.get("op")
        if kind == "add_sheet":
            name = op["name"]
            idx = op.get("index", sheets.Count)
            if not sheets.hasByName(name):
                sheets.insertNewByName(name, idx)
        elif kind == "rename_sheet":
            if sheets.hasByName(op["old"]):
                sheets.getByName(op["old"]).Name = op["new"]
        elif kind == "set":
            sh = sheets.getByName(op["sheet"])
            cell = sh.getCellRangeByName(op["cell"]).getCellByPosition(0, 0)
            if "formula" in op and op["formula"] is not None:
                cell.setFormula(excel_to_calc(str(op["formula"])))
            else:
                v = op.get("value")
                if isinstance(v, (int, float)):
                    cell.setValue(float(v))
                else:
                    cell.setString("" if v is None else str(v))

    doc.calculateAll()
    doc.storeToURL(url, (pv("FilterName", "Calc MS Excel 2007 XML"),))
    doc.close(False)
    try:
        desktop.terminate()
    except Exception:
        pass
    soffice.send_signal(signal.SIGTERM)
    try:
        soffice.wait(timeout=10)
    except Exception:
        soffice.kill()
    print("APPLIED %d ops" % len(ops))


if __name__ == "__main__":
    main()
