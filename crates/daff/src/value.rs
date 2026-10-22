//! Cell values. Most cells are plain strings, but JSON input can carry
//! numbers, booleans, arrays and objects, which the reference passes through
//! untouched while comparing them by their string form.

/// A JSON-like value (`Dynamic` in the reference).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Value>),
    /// A plain object parsed from JSON.
    Object(Vec<(String, Value)>),
    /// A `Map<String,Dynamic>` built by the diff (`{"before":…,"after":…}`).
    Hash(Vec<(String, Value)>),
}

/// A non-null table cell.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Str(String),
    /// A non-string value together with its string form (`"" + d`).
    Native { text: String, value: Value },
}

impl Cell {
    pub fn text(&self) -> &str {
        match self {
            Cell::Str(s) => s,
            Cell::Native { text, .. } => text,
        }
    }

    /// Wrap a value as a cell; `Null` becomes a null cell.
    pub fn from_value(v: Value) -> Option<Cell> {
        match v {
            Value::Null => None,
            Value::Str(s) => Some(Cell::Str(s)),
            other => Some(Cell::Native { text: other.to_text(), value: other }),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Cell::Str(s) => Value::Str(s.clone()),
            Cell::Native { value, .. } => value.clone(),
        }
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        Cell::Str(s)
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        Cell::Str(s.to_string())
    }
}

/// JavaScript `Number.prototype.toString` for finite numbers.
pub fn js_number_to_string(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let sci = format!("{:e}", v.abs());
    let (mant, exp) = sci.split_once('e').unwrap();
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let n = exp.parse::<i32>().unwrap() + 1;
    let k = digits.len() as i32;
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(&digits);
        out.push_str(&"0".repeat((n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-n) as usize));
        out.push_str(&digits);
    } else {
        let e = n - 1;
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.abs().to_string());
    }
    out
}

impl Value {
    /// The reference's string conversion (`Std.string`), as used for
    /// comparing and printing cells.
    pub fn to_text(&self) -> String {
        self.string_rec("")
    }

    fn string_rec(&self, indent: &str) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => js_number_to_string(*n),
            Value::Str(s) => s.clone(),
            Value::Array(items) => {
                // like the reference, arrays indent their contents too
                let inner = format!("{indent}\t");
                let parts: Vec<String> = items.iter().map(|v| v.string_rec(&inner)).collect();
                format!("[{}]", parts.join(","))
            }
            Value::Object(fields) => Self::fields_rec(fields, indent),
            Value::Hash(fields) => {
                let inner = Value::Object(fields.clone());
                Self::fields_rec(&[("h".to_string(), inner)], indent)
            }
        }
    }

    fn fields_rec(fields: &[(String, Value)], indent: &str) -> String {
        let inner = format!("{indent}\t");
        let mut s = String::from("{\n");
        for (i, (k, v)) in fields.iter().enumerate() {
            if i > 0 {
                s.push_str(", \n");
            }
            s.push_str(&inner);
            s.push_str(k);
            s.push_str(" : ");
            s.push_str(&v.string_rec(&inner));
        }
        s.push('\n');
        s.push_str(indent);
        s.push('}');
        s
    }
}

// ----------------------------------------------------------------- JSON ---

/// JS orders integer-like keys first, ascending.
fn js_key_order(fields: Vec<(String, Value)>) -> Vec<(String, Value)> {
    let is_index = |k: &str| {
        !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) && (k == "0" || !k.starts_with('0')) && k.parse::<u32>().is_ok_and(|n| n < u32::MAX)
    };
    let (mut idx, rest): (Vec<_>, Vec<_>) = fields.into_iter().partition(|(k, _)| is_index(k));
    idx.sort_by_key(|(k, _)| k.parse::<u32>().unwrap());
    idx.extend(rest);
    idx
}

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn err<T>(&self, msg: &str) -> Result<T, String> {
        Err(format!("JSON parse error at {}: {msg}", self.i))
    }

    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        let Some(&c) = self.s.get(self.i) else { return self.err("unexpected end") };
        match c {
            b'{' => {
                self.i += 1;
                let mut fields: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Object(fields));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return self.err("expected key");
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return self.err("expected ':'");
                    }
                    self.i += 1;
                    let v = self.value()?;
                    match fields.iter_mut().find(|(fk, _)| *fk == k) {
                        Some(f) => f.1 = v,
                        None => fields.push((k, v)),
                    }
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Object(js_key_order(fields)));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Array(items));
                        }
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
            }
            b'"' => Ok(Value::Str(self.string()?)),
            b't' if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Value::Bool(true))
            }
            b'f' if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Value::Bool(false))
            }
            b'n' if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Value::Null)
            }
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                if self.s[self.i] == b'-' {
                    self.i += 1;
                }
                let digits = |p: &mut Self| {
                    let st = p.i;
                    while p.i < p.s.len() && p.s[p.i].is_ascii_digit() {
                        p.i += 1;
                    }
                    p.i > st
                };
                if self.s.get(self.i) == Some(&b'0') {
                    self.i += 1;
                } else if !digits(self) {
                    return self.err("bad number");
                }
                if self.s.get(self.i) == Some(&b'.') {
                    self.i += 1;
                    if !digits(self) {
                        return self.err("bad number");
                    }
                }
                if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
                    self.i += 1;
                    if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                        self.i += 1;
                    }
                    if !digits(self) {
                        return self.err("bad number");
                    }
                }
                let txt = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                Ok(Value::Num(txt.parse().map_err(|_| "bad number".to_string())?))
            }
            _ => self.err("unexpected character"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut out: Vec<u16> = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else { return self.err("unterminated string") };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(String::from_utf16_lossy(&out));
                }
                b'\\' => {
                    self.i += 1;
                    let Some(&e) = self.s.get(self.i) else { return self.err("bad escape") };
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"' as u16),
                        b'\\' => out.push(b'\\' as u16),
                        b'/' => out.push(b'/' as u16),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok());
                            let Some(n) = hex.and_then(|h| u16::from_str_radix(h, 16).ok()) else {
                                return self.err("bad \\u escape");
                            };
                            self.i += 4;
                            out.push(n);
                        }
                        _ => return self.err("bad escape"),
                    }
                }
                c if c < 0x20 => return self.err("control character in string"),
                _ => {
                    let rest = std::str::from_utf8(&self.s[self.i..]).map_err(|_| "invalid UTF-8".to_string())?;
                    let ch = rest.chars().next().unwrap();
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(ch.encode_utf16(&mut buf));
                    self.i += ch.len_utf8();
                }
            }
        }
    }
}

/// Parse JSON text like JavaScript's `JSON.parse`.
pub fn parse_json(text: &str) -> Result<Value, String> {
    let mut p = JsonParser { s: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return p.err("unexpected trailing content");
    }
    Ok(v)
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Value {
    /// Serialise like `haxe.Json.stringify`; `pretty` uses two-space indents.
    pub fn to_json(&self, pretty: bool) -> String {
        let mut out = String::new();
        self.write_json(&mut out, pretty, "");
        out
    }

    fn write_json(&self, out: &mut String, pretty: bool, indent: &str) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => {
                if n.is_finite() {
                    out.push_str(&js_number_to_string(*n));
                } else {
                    out.push_str("null");
                }
            }
            Value::Str(s) => quote(s, out),
            Value::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                let inner = format!("{indent}  ");
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&inner);
                    }
                    v.write_json(out, pretty, &inner);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(indent);
                }
                out.push(']');
            }
            Value::Object(fields) | Value::Hash(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                let inner = format!("{indent}  ");
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if pretty {
                        out.push('\n');
                        out.push_str(&inner);
                    }
                    quote(k, out);
                    out.push(':');
                    if pretty {
                        out.push(' ');
                    }
                    v.write_json(out, pretty, &inner);
                }
                if pretty {
                    out.push('\n');
                    out.push_str(indent);
                }
                out.push('}');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_numbers() {
        for (v, s) in [
            (1.0, "1"),
            (1.5, "1.5"),
            (-0.0, "0"),
            (0.1, "0.1"),
            (1e21, "1e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (-12.5e-9, "-1.25e-8"),
        ] {
            assert_eq!(js_number_to_string(v), s);
        }
    }

    #[test]
    fn json_round_trip() {
        let v = parse_json(r#"{"b":[1,2.5,null,"x\ny"],"2":true,"1":{}}"#).unwrap();
        assert_eq!(v.to_json(false), r#"{"1":{},"2":true,"b":[1,2.5,null,"x\ny"]}"#);
    }
}
