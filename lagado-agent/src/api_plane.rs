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
    }
}

/// Render an op list to the JSON array the apply step consumes. Any malformed op drops the WHOLE batch
/// (`None`) — we apply a fully-valid plan or none, never a partial mutation.
pub fn ops_to_json(ops: &[ApiOp]) -> Option<Value> {
    let arr: Option<Vec<Value>> = ops.iter().map(op_to_json).collect();
    arr.map(Value::Array)
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
