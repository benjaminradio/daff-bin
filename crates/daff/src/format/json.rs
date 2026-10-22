//! JSON and NDJSON tables, ported from `Coopy.jsonToTable`, `Coopy.jsonify`
//! and `coopy.Ndjson`.
use crate::value::{parse_json, Cell, Value};
use crate::{Error, Result, Table};

fn field<'a>(v: &'a Value, name: &str) -> Option<std::borrow::Cow<'a, Value>> {
    use std::borrow::Cow;
    match v {
        Value::Object(fields) | Value::Hash(fields) => {
            fields.iter().find(|(k, _)| k == name).map(|(_, v)| Cow::Borrowed(v))
        }
        // JavaScript property access on an array: indexes and `length`.
        Value::Array(items) => {
            if name == "length" {
                return Some(Cow::Owned(Value::Num(items.len() as f64)));
            }
            let idx: usize = name.parse().ok().filter(|i: &usize| i.to_string() == name)?;
            items.get(idx).map(Cow::Borrowed)
        }
        _ => None,
    }
}

fn field_count(v: &Value) -> usize {
    match v {
        Value::Object(f) | Value::Hash(f) => f.len(),
        Value::Array(a) => a.len(),
        _ => 0,
    }
}

/// `Coopy.jsonToTable`: the table form is `{"name": {"columns": [...], "rows": [...]}}`.
///
/// Note that the reference does not put the column names into the table: the
/// first row is treated as the header by the diff.
pub fn table_from_json(json: &Value) -> Result<Option<Table>> {
    let Value::Object(entries) = json else { return Ok(None) };
    let mut output = None;
    for (_, t) in entries {
        let Some(Value::Array(columns)) = field(t, "columns").map(|c| c.into_owned()) else { continue };
        let Some(Value::Array(rows)) = field(t, "rows").map(|c| c.into_owned()) else { continue };
        let mut table = Table::new(columns.len(), rows.len());
        let mut has_hash = false;
        let mut known = false;
        for (i, row) in rows.iter().enumerate() {
            if !known {
                has_hash = field_count(row) == columns.len();
                known = true;
            }
            for (j, col) in columns.iter().enumerate() {
                let val = if has_hash {
                    match col {
                        Value::Str(name) => field(row, name).map(|v| v.into_owned()),
                        _ => None,
                    }
                } else {
                    match row {
                        Value::Array(items) => items.get(j).cloned(),
                        _ => None,
                    }
                };
                table.set_cell(j, i, val.and_then(Cell::from_value));
            }
        }
        output = Some(table);
    }
    if let Some(t) = &mut output {
        t.trim_blank();
    }
    Ok(output)
}

pub fn parse_table(text: &str) -> Result<Option<Table>> {
    let json = parse_json(text).map_err(Error::Json)?;
    table_from_json(&json)
}

/// `Coopy.jsonify` plus `Json.stringify(..., "  ")`.
pub fn render_json(t: &Table) -> String {
    let rows: Vec<Value> = (0..t.height())
        .map(|y| {
            Value::Array(
                (0..t.width()).map(|x| t.get_cell(x, y).map_or(Value::Null, Cell::to_value)).collect(),
            )
        })
        .collect();
    Value::Object(vec![("sheet".into(), Value::Array(rows))]).to_json(true)
}

// --------------------------------------------------------------- NDJSON ---

/// `Ndjson.render`.
pub fn render_ndjson(t: &Table) -> String {
    let mut txt = String::new();
    if t.height() == 0 || t.width() == 0 {
        return txt;
    }
    let offset = usize::from(t.get(0, 0) == Some("@:@"));
    let header_row = offset;
    for r in header_row + 1..t.height() {
        let mut row: Vec<(String, Value)> = Vec::new();
        for c in 0..t.width() {
            let mut key = t.get(c, header_row).unwrap_or("").to_string();
            if c == 0 && header_row == 1 {
                key = "@:@".into();
            }
            let v = t.get_cell(c, r).map_or(Value::Null, Cell::to_value);
            match row.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 = v,
                None => row.push((key, v)),
            }
        }
        txt.push_str(&Value::Object(row).to_json(false));
        txt.push('\n');
    }
    txt
}

/// `Ndjson.parse`: rows are added last to first, so columns appear in the
/// order the last rows mention them.
pub fn parse_ndjson(text: &str) -> Result<Table> {
    let mut tab = Table::new(0, 0);
    let rows: Vec<&str> = text.split('\n').collect();
    let mut h = rows.len();
    if rows[h - 1].is_empty() {
        h -= 1;
    }
    let mut columns: Vec<(String, usize)> = Vec::new();
    for i in 0..h {
        let at = h - i - 1;
        let r = at + 1;
        let json = parse_json(rows[at]).map_err(Error::Json)?;
        let fields: Vec<(String, Value)> = match json {
            Value::Object(f) => f,
            _ => Vec::new(),
        };
        let mut w = tab.width();
        let mut height = tab.height();
        let mut resize = false;
        for (name, _) in &fields {
            if !columns.iter().any(|(n, _)| n == name) {
                columns.push((name.clone(), w));
                w += 1;
                resize = true;
            }
        }
        if r >= height {
            height = r + 1;
            resize = true;
        }
        if resize {
            tab.resize(w, height);
        }
        for (name, v) in fields {
            let c = columns.iter().find(|(n, _)| *n == name).unwrap().1;
            tab.set_cell(c, r, Cell::from_value(v));
        }
    }
    if columns.is_empty() && h == 0 {
        return Err(Error::Json("empty NDJSON input".into()));
    }
    for (name, c) in &columns {
        tab.set(*c, 0, Some(name.clone()));
    }
    Ok(tab)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_table_keeps_types() {
        let t = parse_table(r#"{"t":{"columns":["a","b"],"rows":[{"a":"a","b":"b"},{"a":1,"b":null}]}}"#)
            .unwrap()
            .unwrap();
        assert_eq!(t.get(0, 1), Some("1"));
        assert_eq!(t.get(1, 1), None);
        assert!(render_json(&t).contains("\n      1,\n      null"));
    }
}
