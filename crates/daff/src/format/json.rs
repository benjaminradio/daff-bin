//! JSON and NDJSON tables, ported from `Coopy.jsonToTable`, `Coopy.jsonify`
//! and `coopy.Ndjson`.
use crate::value::{parse_json, Cell, Value};
use crate::{Error, Flags, Result, Table, TableMeta};
use std::rc::Rc;

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

/// `JsonTable`: one table of a multi-table file, with its column names as the
/// first row.
fn nested_table(data: &Value, name: &str) -> Result<Table> {
    let bad = || Error::Json(format!("table '{name}' needs \"columns\" and \"rows\""));
    let Some(Value::Array(columns)) = field(data, "columns").map(|c| c.into_owned()) else { return Err(bad()) };
    let Some(Value::Array(rows)) = field(data, "rows").map(|c| c.into_owned()) else { return Err(bad()) };
    let mut t = Table::new(columns.len(), rows.len() + 1);
    let names: Vec<Option<String>> = columns
        .iter()
        .map(|c| match c {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    for (x, n) in names.iter().enumerate() {
        t.set(x, 0, n.clone());
        for (y, row) in rows.iter().enumerate() {
            let v = n.as_ref().and_then(|n| field(row, n)).map(|v| v.into_owned());
            t.set_cell(x, y + 1, v.and_then(Cell::from_value));
        }
    }
    t.set_meta(Some(TableMeta { nested: false, name: Some(name.to_string()) }));
    Ok(t)
}

/// `JsonTables`: `{"names": [...], "tables": {name: {"columns", "rows"}}}` as
/// a two-column table of `name` and `table`, whose second column holds tables.
pub fn tables_from_json(json: &Value, flags: &Flags) -> Result<Table> {
    let Some(Value::Array(names)) = field(json, "names").map(|c| c.into_owned()) else {
        return Err(Error::Json("multi-table JSON needs a \"names\" list".into()));
    };
    let names: Vec<String> = names
        .into_iter()
        .map(|n| match n {
            Value::Str(s) => Ok(s),
            _ => Err(Error::Json("table names must be strings".into())),
        })
        .collect::<Result<_>>()?;
    let allowed = |n: &str| flags.tables.as_ref().map_or(true, |t| t.iter().any(|a| a == n));
    let count = names.iter().filter(|n| allowed(n)).count();
    let mut t = Table::new(2, count + 1);
    t.set(0, 0, Some("name"));
    t.set(1, 0, Some("table"));
    let tables = field(json, "tables").map(|c| c.into_owned());
    let mut at = 1;
    for name in names.iter().filter(|n| allowed(n)) {
        t.set(0, at, Some(name.clone()));
        let data = tables
            .as_ref()
            .and_then(|tb| field(tb, name).map(|c| c.into_owned()))
            .ok_or_else(|| Error::Json(format!("no data for table '{name}'")))?;
        let inner = nested_table(&data, name)?;
        t.set_cell(1, at, Cell::from_value(Value::Table(Rc::new(inner))));
        at += 1;
    }
    t.set_meta(Some(TableMeta { nested: true, name: None }));
    Ok(t)
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
