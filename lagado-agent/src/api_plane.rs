//! API plane — operate the app's REAL programmatic surface (UNO for LibreOffice; CDP / code-CLI later). The
//! richest in-task plane: the model authors native ops (cell formulas, sheet ops), the harness applies them
//! through the LIVE app so it computes/types naturally, then M1 reconciles. This is the de-skewed M2 shape
//! (no evaluator-knowledge, no per-task hacks): the model expresses the transform in the app's own language.
//!
//! "Rust only" destination: Rust OWNS the op model + drives the apply step. The apply step itself is the
//! proven `uno_apply` shape (shape-tested in Python); Rust renders the op list it consumes and invokes it,
//! to be folded fully into Rust as the UNO bridge matures. The op JSON here is the contract between the two.

use serde_json::{json, Value};
use std::collections::HashMap;

/// What a cell holds. A FORMULA is the app's native tool (the live engine computes it); a literal value is
/// for data the model writes directly. Whole numbers stay integers (the app assigns the natural type).
#[derive(Debug, Clone, PartialEq)]
pub enum CellContent {
    /// An Excel-A1 formula, e.g. `=B2-C2-D2-SUM(F2:H2)` (the apply step translates to the engine's syntax).
    Formula(String),
    Number(f64),
    Text(String),
}

/// A native app operation the model can author. Spreadsheet-shaped first (UNO); the enum grows by typed
/// variant, not per-app branches.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiOp {
    /// Set a cell to a formula or literal value (A1 ref, e.g. "J2").
    SetCell { sheet: String, cell: String, content: CellContent },
    /// Add a sheet (index 0 = first).
    AddSheet { name: String, index: Option<usize> },
    /// Rename a sheet.
    RenameSheet { old: String, new: String },
    /// Forward-fill blank cells in a range along a direction (down/up/left/right) — the GENERAL base
    /// for the human "Fill Down/Up/Left/Right" gestures. ONE op replaces dozens of per-cell SetCells
    /// AND removes the need for the model to see which cells are blank — the app carries the value
    /// along the axis. `range` is an A1 range ("B1:E30"); `direction` ∈ {down,up,left,right}.
    Fill { sheet: String, range: String, direction: String },
    /// Apply a formula across a whole range with RELATIVE-REFERENCE adjustment per cell — the app's
    /// "Fill Down a formula" (=B2-C2 in the seed → =B3-C3 below). The model authors the formula ONCE
    /// (for the first cell); the app (UNO `fillAuto`) propagates it down/across the range, adjusting
    /// refs — so a whole computed column is one op, not N. Distinct from `Fill` (which carries VALUES
    /// into blanks; this OVERWRITES the range with the ref-adjusted formula). `range` e.g. "I2:I30".
    SetFormulaRange { sheet: String, range: String, formula: String },
}

/// Map a parsed typed-verb call (verb + kwargs, from the shared Pythonic parser) into an `ApiOp`. The model
/// SELECTS the verb and fills slots; this validates + types it. A `formula` slot → native formula (the live
/// engine computes it); a `value` slot → literal (numeric if it parses, else text). `None` = malformed.
pub fn from_call(verb: &str, kw: &HashMap<String, String>) -> Option<ApiOp> {
    match verb {
        "set_cell" => {
            let sheet = kw.get("sheet")?.clone();
            let cell = kw.get("cell")?.clone();
            let content = if let Some(f) = kw.get("formula") {
                CellContent::Formula(f.clone())
            } else if let Some(v) = kw.get("value") {
                match v.parse::<f64>() {
                    Ok(n) => CellContent::Number(n),
                    Err(_) => CellContent::Text(v.clone()),
                }
            } else {
                return None;
            };
            Some(ApiOp::SetCell { sheet, cell, content })
        }
        "add_sheet" => Some(ApiOp::AddSheet {
            name: kw.get("name")?.clone(),
            index: kw.get("index").and_then(|i| i.parse().ok()),
        }),
        "rename_sheet" => Some(ApiOp::RenameSheet { old: kw.get("old")?.clone(), new: kw.get("new")?.clone() }),
        // General fill base + the named-direction gestures layered over it (all → ApiOp::Fill).
        "fill" | "fill_down" | "fill_up" | "fill_left" | "fill_right" => {
            let direction = match verb {
                "fill" => kw.get("direction").map(|d| d.to_lowercase()).unwrap_or_else(|| "down".into()),
                other => other.trim_start_matches("fill_").to_string(),
            };
            if !matches!(direction.as_str(), "down" | "up" | "left" | "right") { return None; }
            Some(ApiOp::Fill { sheet: kw.get("sheet")?.clone(), range: kw.get("range")?.clone(), direction })
        }
        "set_formula_range" => Some(ApiOp::SetFormulaRange {
            sheet: kw.get("sheet")?.clone(), range: kw.get("range")?.clone(), formula: kw.get("formula")?.clone() }),
        _ => None,
    }
}

/// Render one op as the JSON object the apply step (`uno_apply`) consumes. `None` if a required slot is
/// empty (never emit a half-formed op).
pub fn op_to_json(op: &ApiOp) -> Option<Value> {
    match op {
        ApiOp::SetCell { sheet, cell, content } => {
            if sheet.trim().is_empty() || cell.trim().is_empty() {
                return None;
            }
            let mut o = json!({ "op": "set", "sheet": sheet, "cell": cell });
            let m = o.as_object_mut().unwrap();
            match content {
                CellContent::Formula(f) => { m.insert("formula".into(), json!(f)); }
                CellContent::Number(n) => { m.insert("value".into(), json!(n)); }
                CellContent::Text(t) => { m.insert("value".into(), json!(t)); }
            }
            Some(o)
        }
        ApiOp::AddSheet { name, index } => {
            if name.trim().is_empty() {
                return None;
            }
            let mut o = json!({ "op": "add_sheet", "name": name });
            if let Some(i) = index {
                o.as_object_mut().unwrap().insert("index".into(), json!(i));
            }
            Some(o)
        }
        ApiOp::RenameSheet { old, new } => {
            if old.trim().is_empty() || new.trim().is_empty() {
                return None;
            }
            Some(json!({ "op": "rename_sheet", "old": old, "new": new }))
        }
        ApiOp::Fill { sheet, range, direction } => {
            if sheet.trim().is_empty() || range.trim().is_empty()
                || !matches!(direction.as_str(), "down" | "up" | "left" | "right") {
                return None;
            }
            Some(json!({ "op": "fill", "sheet": sheet, "range": range, "direction": direction }))
        }
        ApiOp::SetFormulaRange { sheet, range, formula } => {
            if sheet.trim().is_empty() || range.trim().is_empty() || formula.trim().is_empty() {
                return None;
            }
            Some(json!({ "op": "set_formula_range", "sheet": sheet, "range": range, "formula": formula }))
        }
    }
}

/// Render an op list to the JSON array the apply step consumes. Any malformed op drops the WHOLE batch
/// (`None`) — we apply a fully-valid plan or none, never a partial mutation.
pub fn ops_to_json(ops: &[ApiOp]) -> Option<Value> {
    let arr: Option<Vec<Value>> = ops.iter().map(op_to_json).collect();
    arr.map(Value::Array)
}

/// Build the GUEST-side execution step for the live OSWorld harness: a SINGLE self-contained command
/// string that (1) kills any running soffice + clears the lock file, (2) applies the native ops to
/// `file_path` through a private headless LibreOffice via UNO (the proven `uno_apply` logic — live calc
/// engine computes formulas, saves with the Excel-2007 filter), then (3) reloads the corrected file into
/// a GUI LibreOffice on the guest display so the OSWorld evaluator's activate-by-title + ctrl+s saves the
/// LIVE instance with corrected content (the `m1_reconcile` reload-into-focus pattern).
///
/// Returned string is meant for `Actuator::run_command`, which runs it as `subprocess.run(cmd,
/// shell=True)` ON THE GUEST. It is a quoted heredoc (`python3 - <<'EOF' … EOF`) so the only
/// shell-interpreted token is the delimiter — every quote/dollar/backtick inside the body is inert.
/// `file_path` and `ops_json` are embedded as Python raw triple-quoted literals (shell- and Python-inert;
/// serde_json output never contains `"""`), so the OSWorld guest sees them verbatim.
///
/// `ops_json` is the stringified array `ops_to_json(...)` produces (e.g. via `.to_string()`).
pub fn build_guest_apply(file_path: &str, ops_json: &str) -> String {
    const TEMPLATE: &str = r#"python3 - <<'LAGADO_GUEST_APPLY_EOF'
import sys, os, json, time, subprocess, signal

# --- inputs (embedded verbatim; raw triple-quoted = shell- and Python-inert) ---
FILE_PATH = r"""__LAGADO_FILE_PATH__"""
OPS = json.loads(r"""__LAGADO_OPS_JSON__""")

PORT = 2019
PROFILE = "file:///tmp/lagado_guest_louser"


def excel_to_calc(f):
    """Excel-A1 -> Calc-A1 for setFormula: sheet refs '!'->'.', arg sep ','->';' (outside strings)."""
    out, in_str = [], False
    for ch in f:
        if ch == '"':
            in_str = not in_str
            out.append(ch)
        elif not in_str and ch == '!':
            out.append('.')
        elif not in_str and ch == ',':
            out.append(';')
        else:
            out.append(ch)
    return "".join(out)


def kill_soffice():
    """Hammer until no soffice remains (single-instance: a survivor would make the reload ATTACH to it
    and never raise its own window)."""
    for _ in range(25):
        subprocess.run("pkill -9 soffice; pkill -9 soffice.bin; true", shell=True)
        n = subprocess.run("pgrep -c soffice; true", shell=True,
                           capture_output=True, text=True).stdout.strip().splitlines()
        if (n[-1] if n else "0") == "0":
            return
        time.sleep(1)


def clear_lock():
    d = os.path.dirname(os.path.abspath(FILE_PATH))
    b = os.path.basename(FILE_PATH)
    try:
        os.remove(os.path.join(d, ".~lock.%s#" % b))
    except OSError:
        pass


def apply_ops():
    """Apply OPS to FILE_PATH via a private headless LibreOffice + UNO socket (uno_apply shape)."""
    soffice = subprocess.Popen(
        ["soffice", "--headless", "--invisible", "--nodefault", "--norestore", "--nologo",
         "--nofirststartwizard",
         "--accept=socket,host=localhost,port=%d;urp;StarOffice.ComponentContext" % PORT,
         "-env:UserInstallation=%s" % PROFILE],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    import uno
    from com.sun.star.beans import PropertyValue

    def pv(n, v):
        p = PropertyValue()
        p.Name = n
        p.Value = v
        return p

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
        soffice.kill()
        print("UNO connect FAILED")
        sys.exit(2)

    smgr = ctx.ServiceManager
    desktop = smgr.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    url = uno.systemPathToFileUrl(os.path.abspath(FILE_PATH))
    doc = desktop.loadComponentFromURL(url, "_blank", 0, (pv("Hidden", True),))

    sheets = doc.Sheets
    def resolve_sheet(name):
        # Tolerate a placeholder/unknown sheet name (the model often copies the prompt's "S"): exact
        # match wins; a single-sheet book resolves unambiguously to its only sheet; else active/first.
        if name and sheets.hasByName(name):
            return sheets.getByName(name)
        if sheets.Count == 1:
            return sheets.getByIndex(0)
        try:
            return doc.CurrentController.ActiveSheet
        except Exception:
            return sheets.getByIndex(0)
    # Dependency-correct ordering (deterministic, harness-owned): a container must exist before it's
    # populated. Apply STRUCTURAL ops (add_sheet, rename_sheet) first — stable within the group — then
    # content ops. Models author ops in goal-narrative order ("fill the column ... in a new sheet"),
    # not dependency order; the harness fixes it so a write never lands on a not-yet-created sheet.
    STRUCTURAL = ("add_sheet", "rename_sheet")
    ordered = [o for o in OPS if o.get("op") in STRUCTURAL] + [o for o in OPS if o.get("op") not in STRUCTURAL]
    for op in ordered:
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
            sh = resolve_sheet(op["sheet"])
            cell = sh.getCellRangeByName(op["cell"]).getCellByPosition(0, 0)
            if "formula" in op and op["formula"] is not None:
                cell.setFormula(excel_to_calc(str(op["formula"])))
            else:
                v = op.get("value")
                if isinstance(v, (int, float)):
                    cell.setValue(float(v))
                else:
                    cell.setString("" if v is None else str(v))
        elif kind == "fill":
            # The human "Fill Down/Up/Left/Right" gestures, generalized: forward-fill blanks along the
            # axis, carrying the last non-empty value. The app sees the cells; the model never
            # enumerates them. getType().value is the enum NAME ("EMPTY"/"VALUE"/"TEXT"/"FORMULA") —
            # string compare avoids pyuno enum import quirks.
            sh = resolve_sheet(op["sheet"])
            addr = sh.getCellRangeByName(op["range"]).getRangeAddress()
            c0, r0, c1, r1 = addr.StartColumn, addr.StartRow, addr.EndColumn, addr.EndRow
            direction = op.get("direction", "down")
            def fill_line(cells):
                carry = None  # (is_numeric, value)
                for c in cells:
                    t = c.getType().value
                    if t == "EMPTY":
                        if carry is not None:
                            if carry[0]:
                                c.setValue(carry[1])
                            else:
                                c.setString(carry[1])
                    elif t == "VALUE":
                        carry = (True, c.getValue())
                    else:
                        carry = (False, c.getString())
            if direction in ("down", "up"):
                for col in range(c0, c1 + 1):
                    rows = range(r0, r1 + 1) if direction == "down" else range(r1, r0 - 1, -1)
                    fill_line([sh.getCellByPosition(col, row) for row in rows])
            else:  # left / right
                for row in range(r0, r1 + 1):
                    cols = range(c0, c1 + 1) if direction == "right" else range(c1, c0 - 1, -1)
                    fill_line([sh.getCellByPosition(col, row) for col in cols])
        elif kind == "set_formula_range":
            # Apply a formula across the range with RELATIVE-REF adjustment — the app's "Fill Down a
            # formula". Set the seed on the top-left cell, then let UNO fillAuto propagate it (adjusting
            # refs like a fill-handle drag), so a whole computed column is ONE op, not N.
            sh = resolve_sheet(op["sheet"])
            rng = sh.getCellRangeByName(op["range"])
            a = rng.getRangeAddress()
            sh.getCellByPosition(a.StartColumn, a.StartRow).setFormula(excel_to_calc(str(op["formula"])))
            if a.EndRow > a.StartRow:
                rng.fillAuto(uno.Enum("com.sun.star.sheet.FillDirection", "TO_BOTTOM"), 1)
            elif a.EndColumn > a.StartColumn:
                rng.fillAuto(uno.Enum("com.sun.star.sheet.FillDirection", "TO_RIGHT"), 1)

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
    print("APPLIED %d ops" % len(OPS))


def reload_into_focus():
    """Relaunch the corrected file in a GUI Calc on the guest display so the evaluator's
    activate-by-title + ctrl+s saves the LIVE instance with corrected content (m1_reconcile)."""
    env = dict(os.environ)
    env.setdefault("DISPLAY", ":0")
    subprocess.Popen(
        ["soffice", "--calc", os.path.abspath(FILE_PATH)],
        env=env, start_new_session=True,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print("RELOADED into GUI")


def main():
    kill_soffice()          # step 1: kill running soffice + clear lock
    clear_lock()
    apply_ops()             # step 2: apply ops via UNO, save Excel-2007
    kill_soffice()          # ensure the headless instance is GONE before the GUI reload attaches
    reload_into_focus()     # step 3: reload corrected file into GUI for the evaluator


if __name__ == "__main__":
    main()
LAGADO_GUEST_APPLY_EOF"#;

    TEMPLATE
        .replace("__LAGADO_FILE_PATH__", file_path)
        .replace("__LAGADO_OPS_JSON__", ops_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cell_formula_matches_uno_apply_contract() {
        let op = ApiOp::SetCell {
            sheet: "Sheet1".into(), cell: "J2".into(),
            content: CellContent::Formula("=B2-C2-D2-SUM(F2:H2)".into()),
        };
        assert_eq!(op_to_json(&op).unwrap(),
            json!({"op":"set","sheet":"Sheet1","cell":"J2","formula":"=B2-C2-D2-SUM(F2:H2)"}));
    }

    #[test]
    fn set_cell_value_uses_value_key() {
        let num = ApiOp::SetCell { sheet: "Sheet1".into(), cell: "A1".into(), content: CellContent::Number(55000.0) };
        assert_eq!(op_to_json(&num).unwrap(), json!({"op":"set","sheet":"Sheet1","cell":"A1","value":55000.0}));
        let txt = ApiOp::SetCell { sheet: "Sheet2".into(), cell: "A1".into(), content: CellContent::Text("Year_Profit".into()) };
        assert_eq!(op_to_json(&txt).unwrap(), json!({"op":"set","sheet":"Sheet2","cell":"A1","value":"Year_Profit"}));
    }

    #[test]
    fn add_and_rename_sheet() {
        assert_eq!(op_to_json(&ApiOp::AddSheet { name: "Sheet2".into(), index: Some(0) }).unwrap(),
                   json!({"op":"add_sheet","name":"Sheet2","index":0}));
        assert_eq!(op_to_json(&ApiOp::AddSheet { name: "Sheet2".into(), index: None }).unwrap(),
                   json!({"op":"add_sheet","name":"Sheet2"}));
        assert_eq!(op_to_json(&ApiOp::RenameSheet { old: "Sheet 1".into(), new: "LARS".into() }).unwrap(),
                   json!({"op":"rename_sheet","old":"Sheet 1","new":"LARS"}));
    }

    #[test]
    fn empty_required_slot_drops_the_op_and_the_batch() {
        let bad = ApiOp::SetCell { sheet: "".into(), cell: "J2".into(), content: CellContent::Number(1.0) };
        assert_eq!(op_to_json(&bad), None);
        // one bad op nukes the whole batch — apply-all-or-none (no partial mutation)
        let batch = vec![
            ApiOp::AddSheet { name: "Sheet2".into(), index: Some(0) },
            ApiOp::RenameSheet { old: "".into(), new: "x".into() },
        ];
        assert_eq!(ops_to_json(&batch), None);
    }

    #[test]
    fn from_call_types_the_native_ops() {
        let mut f = HashMap::new();
        f.insert("sheet".into(), "Sheet1".into()); f.insert("cell".into(), "J2".into());
        f.insert("formula".into(), "=B2-C2".into());
        assert_eq!(from_call("set_cell", &f),
                   Some(ApiOp::SetCell { sheet: "Sheet1".into(), cell: "J2".into(),
                                         content: CellContent::Formula("=B2-C2".into()) }));
        // a numeric value slot → Number; a non-numeric → Text
        let mut n = HashMap::new();
        n.insert("sheet".into(), "S".into()); n.insert("cell".into(), "A1".into()); n.insert("value".into(), "55000".into());
        assert_eq!(from_call("set_cell", &n).unwrap(),
                   ApiOp::SetCell { sheet: "S".into(), cell: "A1".into(), content: CellContent::Number(55000.0) });
        let mut t = HashMap::new();
        t.insert("sheet".into(), "S".into()); t.insert("cell".into(), "A1".into()); t.insert("value".into(), "2015_55000".into());
        assert_eq!(from_call("set_cell", &t).unwrap(),
                   ApiOp::SetCell { sheet: "S".into(), cell: "A1".into(), content: CellContent::Text("2015_55000".into()) });
        let mut a = HashMap::new(); a.insert("name".into(), "Sheet2".into()); a.insert("index".into(), "0".into());
        assert_eq!(from_call("add_sheet", &a).unwrap(), ApiOp::AddSheet { name: "Sheet2".into(), index: Some(0) });
        // missing required slot / unknown verb → None
        assert_eq!(from_call("set_cell", &HashMap::new()), None);
        assert_eq!(from_call("nope", &HashMap::new()), None);
    }

    #[test]
    fn fill_general_base_and_direction_aliases_map_to_one_op() {
        let mut g = HashMap::new();
        g.insert("sheet".into(), "Data".into()); g.insert("range".into(), "B1:E30".into());
        g.insert("direction".into(), "right".into());
        // general base: fill(direction=...) → Fill
        assert_eq!(from_call("fill", &g).unwrap(),
                   ApiOp::Fill { sheet: "Data".into(), range: "B1:E30".into(), direction: "right".into() });
        // fill() with no direction defaults to down
        let mut d = HashMap::new(); d.insert("sheet".into(), "S".into()); d.insert("range".into(), "B1:E30".into());
        assert_eq!(from_call("fill", &d).unwrap(),
                   ApiOp::Fill { sheet: "S".into(), range: "B1:E30".into(), direction: "down".into() });
        // overlay aliases bind the direction → SAME op type
        assert_eq!(from_call("fill_up", &d).unwrap(),
                   ApiOp::Fill { sheet: "S".into(), range: "B1:E30".into(), direction: "up".into() });
        // round-trips to the apply JSON the UNO step consumes
        assert_eq!(op_to_json(&from_call("fill_left", &d).unwrap()).unwrap(),
                   json!({"op":"fill","sheet":"S","range":"B1:E30","direction":"left"}));
        // bad direction → rejected (never a half-formed op)
        let mut bad = d.clone(); bad.insert("direction".into(), "sideways".into());
        assert_eq!(from_call("fill", &bad), None);
    }

    #[test]
    fn set_formula_range_round_trips() {
        let mut f = HashMap::new();
        f.insert("sheet".into(), "Sheet1".into()); f.insert("range".into(), "I2:I30".into());
        f.insert("formula".into(), "=B2-C2-D2".into());
        let op = from_call("set_formula_range", &f).unwrap();
        assert_eq!(op, ApiOp::SetFormulaRange { sheet: "Sheet1".into(), range: "I2:I30".into(), formula: "=B2-C2-D2".into() });
        assert_eq!(op_to_json(&op).unwrap(),
                   json!({"op":"set_formula_range","sheet":"Sheet1","range":"I2:I30","formula":"=B2-C2-D2"}));
        // missing the formula slot → no half-formed op
        let mut nf = HashMap::new(); nf.insert("sheet".into(), "S".into()); nf.insert("range".into(), "I2:I30".into());
        assert_eq!(from_call("set_formula_range", &nf), None);
    }

    #[test]
    fn build_guest_apply_has_kill_apply_reload_and_embeds_path_and_ops() {
        let ops = ops_to_json(&[
            ApiOp::AddSheet { name: "Sheet2".into(), index: Some(0) },
            ApiOp::SetCell {
                sheet: "Sheet1".into(), cell: "J2".into(),
                content: CellContent::Formula("=B2-C2-D2-SUM(F2:H2)".into()),
            },
        ]).unwrap().to_string();
        let path = "/root/spreadsheet.xlsx";
        let cmd = build_guest_apply(path, &ops);

        // single self-contained command via a quoted heredoc (only the delimiter is shell-interpreted)
        assert!(cmd.starts_with("python3 - <<'LAGADO_GUEST_APPLY_EOF'"));
        assert!(cmd.trim_end().ends_with("LAGADO_GUEST_APPLY_EOF"));

        // step 1: kill running soffice + remove the lock file
        assert!(cmd.contains("pkill -9 soffice"));
        assert!(cmd.contains(".~lock."));

        // step 2: apply via UNO (uno_apply logic), with the Excel-2007 save filter + syntax conversion
        assert!(cmd.contains("import uno"));
        assert!(cmd.contains("loadComponentFromURL"));
        assert!(cmd.contains("excel_to_calc"));
        assert!(cmd.contains("calculateAll"));
        assert!(cmd.contains("Calc MS Excel 2007 XML"));

        // step 3: reload the corrected file into a GUI Calc for the evaluator (m1_reconcile)
        assert!(cmd.contains("soffice"));
        assert!(cmd.contains("--calc"));

        // file path + ops JSON embedded verbatim (guest reads them literally)
        assert!(cmd.contains(path));
        assert!(cmd.contains(&ops));
        assert!(cmd.contains("\"op\": \"add_sheet\"") || cmd.contains("\"op\":\"add_sheet\""));
    }

    #[test]
    fn gross_profit_plan_renders_as_a_batch() {
        // the de-skewed M2 plan: J2:J4 formulas + a Sheet2 with a derived text column — authored as native
        // ops, not openpyxl value-codegen
        let mut ops = vec![ApiOp::AddSheet { name: "Sheet2".into(), index: Some(0) }];
        for r in 2..=4 {
            ops.push(ApiOp::SetCell {
                sheet: "Sheet1".into(), cell: format!("J{r}"),
                content: CellContent::Formula(format!("=B{r}-C{r}-D{r}-SUM(F{r}:H{r})")),
            });
        }
        let arr = ops_to_json(&ops).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 4);
    }
}
