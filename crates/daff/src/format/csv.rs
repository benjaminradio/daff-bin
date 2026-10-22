//! CSV/TSV/etc., ported from `coopy.Csv` (including its quirks: the literal
//! text `NULL` is a null cell, and nulls render as `NULL`).
use super::{TableReader, TableWriter};
use crate::{Result, Table};
use std::io::{Read, Write};

#[derive(Debug, Clone)]
pub struct Csv {
    pub delimiter: String,
    /// Line ending used when writing; `None` means CRLF.
    pub eol: Option<String>,
}

impl Default for Csv {
    fn default() -> Self {
        Self { delimiter: ",".into(), eol: None }
    }
}

/// Result of [`Csv::parse_full`].
pub struct Parsed {
    pub table: Table,
    /// First line ending found in the text.
    pub eol: Option<String>,
    pub warnings: Vec<String>,
}

struct Parser<'a> {
    txt: &'a [char],
    delim: Vec<char>,
    cursor: usize,
    row_ended: bool,
    discovered_eol: Option<String>,
}

impl Parser<'_> {
    fn parse_cell_part(&mut self) -> Option<String> {
        let txt = self.txt;
        self.row_ended = false;
        let mut first_non_underscore = txt.len();
        let mut last_processed = 0;
        let mut quoting = false;
        let mut quote: Option<char> = None;
        let mut result = String::new();
        let start = self.cursor;
        let mut i = self.cursor;
        while i < txt.len() {
            let ch = txt[i];
            last_processed = i;
            if ch != '_' && i < first_non_underscore {
                first_non_underscore = i;
            }
            if !quoting {
                if ch == self.delim[0] {
                    if self.delim.len() == 1 {
                        break;
                    }
                    if i + self.delim.len() <= txt.len() && txt[i..i + self.delim.len()] == self.delim[..] {
                        last_processed += self.delim.len() - 1;
                        break;
                    }
                }
                if ch == '\r' || ch == '\n' {
                    if let Some(&ch2) = txt.get(i + 1) {
                        if ch2 != ch && (ch2 == '\r' || ch2 == '\n') {
                            if self.discovered_eol.is_none() {
                                self.discovered_eol = Some(format!("{ch}{ch2}"));
                            }
                            last_processed += 1;
                        }
                    }
                    if self.discovered_eol.is_none() {
                        self.discovered_eol = Some(ch.to_string());
                    }
                    self.row_ended = true;
                    break;
                }
                if ch == '"' {
                    if i == self.cursor {
                        quoting = true;
                        quote = Some(ch);
                        if i != start {
                            result.push(ch);
                        }
                        i += 1;
                        continue;
                    } else if Some(ch) == quote {
                        quoting = true;
                    }
                }
                result.push(ch);
                i += 1;
                continue;
            }
            if Some(ch) == quote {
                quoting = false;
                i += 1;
                continue;
            }
            result.push(ch);
            i += 1;
        }
        self.cursor = last_processed;
        if quote.is_none() {
            if result == "NULL" {
                return None;
            }
            if first_non_underscore > start {
                let del = first_non_underscore - start;
                if result.chars().skip(del).collect::<String>() == "NULL" {
                    return Some(result.chars().skip(1).collect());
                }
            }
        }
        Some(result)
    }
}

impl Csv {
    /// Parse a single cell without CSV structure (`Csv.parseCell`): only the
    /// `NULL` / `_NULL` escaping is interpreted.
    pub fn parse_cell(txt: Option<&str>) -> Option<String> {
        let txt = txt?;
        if txt == "NULL" {
            return None;
        }
        let first_non_underscore = txt.chars().position(|c| c != '_').unwrap_or(txt.chars().count());
        if first_non_underscore > 0 && txt.chars().skip(first_non_underscore).collect::<String>() == "NULL" {
            return Some(txt.chars().skip(1).collect());
        }
        Some(txt.to_string())
    }

    pub fn with_delimiter(delim: &str) -> Self {
        Self { delimiter: delim.into(), eol: None }
    }

    /// Parse text into a table, also returning the first line ending seen.
    pub fn parse(&self, text: &str) -> (Table, Option<String>) {
        let p = self.parse_full(text);
        (p.table, p.eol)
    }

    /// As [`parse`](Self::parse), also returning the warnings the reference
    /// would have traced.
    pub fn parse_full(&self, text: &str) -> Parsed {
        let mut warnings = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let delim: Vec<char> = self.delimiter.chars().collect();
        let mut p = Parser { txt: &chars, delim, cursor: 0, row_ended: false, discovered_eol: None };
        let mut tab = Table::new(0, 0);
        let (mut w, mut h, mut at, mut yat) = (0usize, 0usize, 0usize, 0usize);
        while p.cursor < chars.len() {
            let cell = p.parse_cell_part();
            if yat >= h {
                h = yat + 1;
                tab.resize(w, h);
            }
            if at >= w && yat == 0 {
                w = at + 1;
                tab.resize(w, h);
            }
            if at >= w && yat > 0 && cell.as_deref().is_some_and(|c| !c.is_empty()) {
                let context: Vec<&str> = (0..w).map(|i| tab.get(i, yat).unwrap_or("null")).collect();
                warnings.push(format!(
                    "Ignored overflowing row {yat} with cell '{}' after: {}",
                    cell.as_deref().unwrap_or(""),
                    context.join(",")
                ));
            }
            // Overflowing cells on later rows are written through the flat
            // index, as in the reference implementation.
            tab.set(at, h - 1, cell);
            at += 1;
            if p.row_ended {
                at = 0;
                yat += 1;
            }
            p.cursor += 1;
        }
        Parsed { table: tab, eol: p.discovered_eol, warnings }
    }

    pub fn render_cell(&self, cell: Option<&str>) -> String {
        let Some(s) = cell else { return "NULL".into() };
        let first_last_space = s.starts_with(' ') || s.ends_with(' ');
        let need_quote = first_last_space
            || s.contains(['"', '\r', '\n', '\t'])
            || s.contains(self.delimiter.as_str());
        let body = s.replace('"', "\"\"");
        if need_quote {
            format!("\"{body}\"")
        } else {
            body
        }
    }

    pub fn render_table(&self, t: &Table) -> String {
        let eol = self.eol.as_deref().unwrap_or("\r\n");
        let mut out = String::new();
        // The reference's row stream always yields a first row, so an empty
        // table is written as one line (of `NULL` cells if it has columns).
        for y in 0..t.height().max(1) {
            for x in 0..t.width() {
                if x > 0 {
                    out.push_str(&self.delimiter);
                }
                out.push_str(&self.render_cell(t.get(x, y)));
            }
            out.push_str(eol);
        }
        out
    }
}

impl TableReader for Csv {
    fn read(&self, input: &mut dyn Read) -> Result<Table> {
        let mut buf = Vec::new();
        input.read_to_end(&mut buf)?;
        Ok(self.parse(&String::from_utf8_lossy(&buf)).0)
    }
}

impl TableWriter for Csv {
    fn write(&self, table: &Table, out: &mut dyn Write) -> Result<()> {
        out.write_all(self.render_table(table).as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = "a,b\r\n1,\"x,y\"\r\n";
        let (t, eol) = Csv::default().parse(src);
        assert_eq!(t.get(1, 1), Some("x,y"));
        assert_eq!(eol.as_deref(), Some("\r\n"));
        assert_eq!(Csv::default().render_table(&t), src);
    }

    #[test]
    fn null_cells() {
        let (t, _) = Csv::default().parse("a,b\nNULL,_NULL\n");
        assert_eq!(t.get(0, 1), None);
        assert_eq!(t.get(1, 1), Some("NULL"));
    }
}
