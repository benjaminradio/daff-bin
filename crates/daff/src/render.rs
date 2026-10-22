//! HTML and terminal rendering of highlighter-format diffs, ported from
//! `coopy.DiffRender` and `coopy.TerminalDiffRender`.
use crate::cell_info::{examine_cell, render_cell};
use crate::format::csv::Csv;
use crate::{Flags, Table};

/// Renders a diff table as HTML.
pub struct DiffRender {
    out: Vec<String>,
    td_open: &'static str,
    td_close: &'static str,
    pretty_arrows: bool,
    quote_html: bool,
    section: Option<String>,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

impl Default for DiffRender {
    fn default() -> Self {
        DiffRender {
            out: Vec::new(),
            td_open: "<td",
            td_close: "</td>",
            pretty_arrows: true,
            quote_html: true,
            section: None,
        }
    }
}

impl DiffRender {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn use_pretty_arrows(&mut self, flag: bool) {
        self.pretty_arrows = flag;
    }

    pub fn quote_html(&mut self, flag: bool) {
        self.quote_html = flag;
    }

    fn insert(&mut self, s: &str) {
        self.out.push(s.to_string());
    }

    fn set_section(&mut self, s: Option<&str>) {
        if self.section.as_deref() == s {
            return;
        }
        if let Some(old) = self.section.take() {
            self.insert(&format!("</t{old}>\n"));
        }
        if let Some(new) = s {
            self.insert(&format!("<t{new}>\n"));
            self.section = Some(new.to_string());
        }
    }

    fn begin_row(&mut self, mode: &str) {
        self.td_open = "<td";
        self.td_close = "</td>";
        if mode == "header" {
            self.td_open = "<th";
            self.td_close = "</th>";
        }
        if mode.is_empty() {
            self.insert("<tr>");
        } else {
            self.insert(&format!("<tr class=\"{mode}\">"));
        }
    }

    fn insert_cell(&mut self, txt: Option<&str>, mode: &str) {
        let decorate = if mode.is_empty() { String::new() } else { format!(" class=\"{mode}\"") };
        let open = format!("{}{decorate}>", self.td_open);
        self.insert(&open);
        self.insert(txt.unwrap_or("null"));
        let close = self.td_close;
        self.insert(close);
    }

    /// Render a table; call [`html`](Self::html) for the result.
    pub fn render(&mut self, tab: &Table) -> &mut Self {
        if tab.width() == 0 || tab.height() == 0 {
            return self;
        }
        self.insert("<table>\n");
        self.section = None;
        let mut change_row: Option<usize> = None;
        let corner = tab.get(0, 0).unwrap_or("");
        let off = usize::from(corner == "@:@");
        if off > 0 && (tab.width() <= 1 || tab.height() <= 1) {
            return self;
        }
        for row in 0..tab.height() {
            let txt = tab.get(off, row).unwrap_or("").to_string();
            let row_mode = examine_cell(off, row, Some(&txt), "", &txt, off).category;
            if row_mode == "spec" {
                change_row = Some(row);
            }
            if matches!(row_mode.as_str(), "header" | "spec" | "index" | "meta") {
                self.set_section(Some("head"));
            } else {
                self.set_section(Some("body"));
            }
            self.begin_row(&row_mode);
            for c in 0..tab.width() {
                let vcol = change_row.map_or("", |r| tab.get(c, r).unwrap_or(""));
                let cell = examine_cell(c, row, tab.get(c, row), vcol, &txt, off);
                let val = if self.pretty_arrows { cell.pretty_value } else { cell.value };
                let val = if self.quote_html { Some(html_escape(val.as_deref().unwrap_or(""))) } else { val };
                self.insert_cell(val.as_deref(), &cell.category_given_tr);
            }
            self.insert("</tr>\n");
        }
        self.set_section(None);
        self.insert("</table>\n");
        self
    }

    pub fn html(&self) -> String {
        self.out.concat()
    }

    pub fn sample_css() -> &'static str {
        SAMPLE_CSS
    }

    /// Wrap the output in a complete HTML page.
    pub fn complete_html(&mut self) {
        self.out.insert(0, "<!DOCTYPE html>\n<html>\n<head>\n<meta charset='utf-8'>\n<style TYPE='text/css'>\n".into());
        self.out.insert(1, SAMPLE_CSS.into());
        self.out.insert(2, "</style>\n</head>\n<body>\n<div class='highlighter'>\n".into());
        self.out.push("</div>\n</body>\n</html>\n".into());
    }
}

const SAMPLE_CSS: &str = ".highlighter .add { 
  background-color: #7fff7f;
}

.highlighter .remove { 
  background-color: #ff7f7f;
}

.highlighter td.modify { 
  background-color: #7f7fff;
}

.highlighter td.conflict { 
  background-color: #f00;
}

.highlighter .spec { 
  background-color: #aaa;
}

.highlighter .move { 
  background-color: #ffa;
}

.highlighter .null { 
  color: #888;
}

.highlighter table { 
  border-collapse:collapse;
}

.highlighter td, .highlighter th {
  border: 1px solid #2D4068;
  padding: 3px 7px 2px;
}

.highlighter th, .highlighter .header, .highlighter .meta {
  background-color: #aaf;
  font-weight: bold;
  padding-bottom: 4px;
  padding-top: 5px;
  text-align:left;
}

.highlighter tr.header th {
  border-bottom: 2px solid black;
}

.highlighter tr.index td, .highlighter .index, .highlighter tr.header th.index {
  background-color: white;
  border: none;
}

.highlighter .gap {
  color: #888;
}

.highlighter td {
  empty-cells: show;
  white-space: pre-wrap;
}
";

/// Renders a table for a terminal, with ANSI colours for diffs.
pub struct TerminalDiffRender {
    align_columns: bool,
    wide_columns: bool,
    use_glyphs: bool,
    csv: Csv,
    diff: bool,
}

fn code(name: &str) -> &'static str {
    match name {
        "header" | "meta" => "\x1b[0;1m",
        "minor" => "\x1b[33m",
        "done" => "\x1b[0m",
        "spec" => "\x1b[35;1m",
        "add" => "\x1b[32;1m",
        "conflict" => "\x1b[33;1m",
        "modify" => "\x1b[34;1m",
        "remove" => "\x1b[31;1m",
        _ => "",
    }
}

fn has_code(name: &str) -> bool {
    !code(name).is_empty()
}

/// Length in UTF-16 code units, as JavaScript counts string length.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

impl TerminalDiffRender {
    /// Padding comes from `flags.padding_strategy`.
    pub fn new(flags: Option<&Flags>, delimiter: Option<&str>, diff: bool) -> Self {
        let mut r = TerminalDiffRender {
            align_columns: true,
            wide_columns: false,
            use_glyphs: true,
            csv: Csv::with_delimiter(delimiter.unwrap_or(",")),
            diff,
        };
        if let Some(f) = flags {
            if f.padding_strategy.as_deref() == Some("dense") {
                r.align_columns = false;
            }
            if f.padding_strategy.as_deref() == Some("sparse") {
                r.wide_columns = true;
            }
            r.use_glyphs = f.use_glyphs;
        }
        r
    }

    pub fn render(&self, t: &Table) -> String {
        let (w, h) = (t.width(), t.height());
        let sizes = if self.align_columns { self.pick_sizes(t) } else { None };
        let mut out = String::new();
        for y in 0..h {
            let (mut target, mut at) = (0usize, 0usize);
            for x in 0..w {
                if let Some(sizes) = &sizes {
                    for _ in at..target {
                        out.push(' ');
                        at += 1;
                    }
                    let _ = sizes;
                }
                if x > 0 {
                    out.push_str(code("minor"));
                    out.push_str(&self.csv.delimiter);
                    out.push_str(code("done"));
                }
                out.push_str(&self.get_text(t, x, y, true));
                if let Some(sizes) = &sizes {
                    at += js_len(&self.get_text(t, x, y, false));
                    target += sizes[x];
                }
            }
            out.push_str("\r\n");
        }
        out
    }

    fn get_text(&self, t: &Table, x: usize, y: usize, color: bool) -> String {
        let cell = render_cell(t, x, y);
        let mut val: Option<String> = t.get(x, y).map(String::from);
        if color && self.diff {
            let mut c: Option<&str> = None;
            if has_code(&cell.category) {
                c = Some(&cell.category);
            }
            if has_code(&cell.category_given_tr) {
                c = Some(&cell.category_given_tr);
            }
            if let Some(c) = c {
                let sep = if self.use_glyphs { &cell.pretty_separator } else { &cell.separator };
                let js = |s: &Option<String>| s.clone().unwrap_or_else(|| "null".into());
                if cell.rvalue.is_some() {
                    let mut v = format!(
                        "{}{}{}{}{}{}{}",
                        code("remove"),
                        js(&cell.lvalue),
                        code("modify"),
                        sep,
                        code("add"),
                        js(&cell.rvalue),
                        code("done")
                    );
                    if cell.pvalue.is_some() {
                        v = format!("{}{}{}{}{}", code("conflict"), js(&cell.pvalue), code("modify"), sep, v);
                    }
                    val = Some(v);
                } else {
                    let v = if self.use_glyphs { &cell.pretty_value } else { &cell.value };
                    val = Some(format!("{}{}{}", code(c), js(v), code("done")));
                }
            }
        } else if color && !self.diff {
            if y == 0 {
                val = Some(format!("{}{}{}", code("header"), val.as_deref().unwrap_or("null"), code("done")));
            }
        } else {
            val = if self.use_glyphs { cell.pretty_value } else { cell.value };
        }
        self.csv.render_cell(val.as_deref())
    }

    fn pick_sizes(&self, t: &Table) -> Option<Vec<usize>> {
        let (w, h) = (t.width(), t.height());
        let mut sizes = Vec::new();
        let mut row: Option<usize> = None;
        let mut total = w.saturating_sub(1);
        for x in 0..w {
            let (mut m, mut m2) = (0f64, 0f64);
            let (mut mmax, mut mmostmax) = (0usize, 0usize);
            let mut mmin: Option<usize> = None;
            let lens: Vec<(String, usize)> = (0..h)
                .map(|y| {
                    let txt = self.get_text(t, x, y, false);
                    let len = js_len(&txt);
                    (txt, len)
                })
                .collect();
            for (y, (txt, len)) in lens.iter().enumerate() {
                if txt == "@@" && row.is_none() && self.diff {
                    row = Some(y);
                }
                if row.is_none() && !self.diff {
                    row = Some(y);
                }
                if row == Some(y) {
                    mmin = Some(*len);
                }
                m += *len as f64;
                m2 += (*len * *len) as f64;
                mmax = mmax.max(*len);
            }
            let mean = m / h as f64;
            let stddev = (m2 / h as f64 - mean * mean).sqrt();
            let mut most = (mean + stddev * 2.0 + 0.5).trunc();
            most = if most.is_nan() { 0.0 } else { most };
            let most_f = most as usize;
            for (_, len) in &lens {
                if *len <= most_f {
                    mmostmax = mmostmax.max(*len);
                }
            }
            let mut most = mmostmax;
            if let Some(mn) = mmin {
                if most < mn {
                    most = mn;
                }
            }
            if self.wide_columns {
                most = mmax;
            }
            sizes.push(most);
            total += most;
        }
        if total > 130 && !self.wide_columns {
            return None;
        }
        Some(sizes)
    }
}
