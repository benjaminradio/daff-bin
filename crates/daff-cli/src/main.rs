//! `daff` command line tool. Argument handling and format selection follow the
//! reference implementation (`coopy.Coopy.run`): options may appear anywhere,
//! `diff`/`copy` are implied by file names, and formats are picked from file
//! extensions.
use daff::format::csv::Csv;
use daff::format::json::{parse_ndjson, render_json, render_ndjson, table_from_json};
use daff::value::parse_json;
use daff::{
    compare_tables3, DiffRender, Flags, HighlightPatch, Merger, Table, TableDiff, TerminalDiffRender,
};
use std::io::{IsTerminal, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Errors exit with status 1 (an uncaught exception in the reference), or 2
/// when `--fail-if-diff` is in effect.
static FAIL_IF_DIFF: AtomicBool = AtomicBool::new(false);

const PSV_DELIM: &str = "\u{1F4A9}";

/// Format state, mirroring the private fields of `coopy.Coopy`.
struct State {
    format_preference: Option<String>,
    delim_preference: Option<String>,
    csv_eol_preference: Option<String>,
    extern_preference: bool,
    output_format: String,
    nested_output: bool,
    order_preference: bool,
    css_output: Option<String>,
    fragment: bool,
}

type Res<T> = Result<T, String>;

impl State {
    fn new() -> Self {
        State {
            format_preference: None,
            delim_preference: None,
            csv_eol_preference: None,
            extern_preference: false,
            output_format: "copy".into(),
            nested_output: false,
            order_preference: false,
            css_output: None,
            fragment: false,
        }
    }

    /// `checkFormat`: infer format and delimiter from a file name.
    fn check_format(&mut self, name: Option<&str>) -> String {
        if self.extern_preference {
            return self.format_preference.clone().unwrap_or_default();
        }
        let mut ext = String::new();
        if let Some(pt) = name.and_then(|n| n.rfind('.').map(|i| (n, i))) {
            let e = pt.0[pt.1 + 1..].to_lowercase();
            let csv = |d: &str, s: &mut State| {
                s.format_preference = Some("csv".into());
                s.delim_preference = Some(d.into());
            };
            ext = e.clone();
            match e.as_str() {
                "json" => self.format_preference = Some("json".into()),
                "ndjson" => self.format_preference = Some("ndjson".into()),
                "csv" => csv(",", self),
                "tsv" => csv("\t", self),
                "ssv" => csv(";", self),
                "psv" => csv(PSV_DELIM, self),
                "sqlite3" | "sqlite" => self.format_preference = Some("sqlite".into()),
                "html" | "htm" => self.format_preference = Some("html".into()),
                "www" => self.format_preference = Some("www".into()),
                _ => ext.clear(),
            }
        }
        let fmt = self.format_preference.as_deref();
        self.nested_output = fmt == Some("json") || fmt == Some("ndjson");
        self.order_preference = !self.nested_output;
        ext
    }

    fn set_format(&mut self, name: &str) {
        self.extern_preference = false;
        self.check_format(Some(&format!(".{name}")));
        self.extern_preference = true;
    }

    fn unsupported(&self, what: &str) -> String {
        format!("{what} is not supported yet")
    }

    fn load_table(&mut self, name: &str) -> Res<Table> {
        let ext = self.check_format(Some(name));
        if ext == "sqlite" {
            return Err(self.unsupported("format 'sqlite'"));
        }
        let bytes = std::fs::read(name).map_err(|e| format!("{name}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        if ext == "ndjson" {
            return parse_ndjson(&text).map_err(|e| e.to_string());
        }
        if ext == "json" || ext.is_empty() {
            let parsed = parse_json(&text).map(|json| {
                self.format_preference = Some("json".into());
                json
            });
            match parsed {
                Ok(json) => {
                    if matches!(&json, daff::Value::Object(f) if f.iter().any(|(k, v)| k == "tables" && !matches!(v, daff::Value::Null))) {
                        return Err(self.unsupported("JSON files with multiple tables"));
                    }
                    match table_from_json(&json).map_err(|e| e.to_string())? {
                        Some(t) => return Ok(t),
                        None if ext == "json" => return Err("JSON failed".into()),
                        None => {}
                    }
                }
                Err(e) if ext == "json" => return Err(e),
                Err(_) => {}
            }
        }
        self.format_preference = Some("csv".into());
        let csv = Csv::with_delimiter(self.delim_preference.as_deref().unwrap_or(","));
        let parsed = csv.parse_full(&text);
        // The reference traces these to stdout, which corrupts piped output;
        // we write them to stderr instead (see docs/QUIRKS.md).
        for w in &parsed.warnings {
            eprintln!("{w}");
        }
        if self.csv_eol_preference.is_none() {
            self.csv_eol_preference = parsed.eol;
        }
        let mut t = parsed.table;
        t.trim_blank();
        Ok(t)
    }

    fn save_text(&self, name: &str, txt: &str) -> Res<()> {
        if name == "-" {
            std::io::stdout().write_all(txt.as_bytes()).map_err(|e| e.to_string())
        } else {
            std::fs::write(name, txt).map_err(|e| format!("{name}: {e}"))
        }
    }

    fn apply_renderer(&self, name: &str, mut renderer: DiffRender) -> Res<()> {
        if !self.fragment {
            renderer.complete_html();
        }
        if self.format_preference.as_deref() == Some("www") {
            return Err(self.unsupported("--www"));
        }
        self.save_text(name, &renderer.html())?;
        if let Some(css) = &self.css_output {
            self.save_text(css, DiffRender::sample_css())?;
        }
        Ok(())
    }

    fn render_table(&self, flags: &Flags, name: &str, t: &Table) -> Res<()> {
        let mut renderer = DiffRender::new();
        renderer.use_pretty_arrows(flags.use_glyphs);
        renderer.quote_html(flags.quote_html);
        renderer.render(t);
        self.apply_renderer(name, renderer)
    }

    /// `encodeTable`; `None` means the output was already written.
    fn encode_table(
        &mut self,
        flags: &Flags,
        name: &str,
        t: &Table,
        render: Option<&TerminalDiffRender>,
    ) -> Res<Option<String>> {
        if self.output_format != "copy" {
            let f = self.output_format.clone();
            self.set_format(&f);
        }
        self.check_format(Some(name));
        if self.format_preference.as_deref() == Some("sqlite") && !self.extern_preference {
            self.format_preference = Some("csv".into());
        }
        if let Some(r) = render {
            return Ok(Some(r.render(t)));
        }
        match self.format_preference.as_deref() {
            Some("csv") => {
                let csv = Csv {
                    delimiter: self.delim_preference.clone().unwrap_or_else(|| ",".into()),
                    eol: self.csv_eol_preference.clone(),
                };
                Ok(Some(csv.render_table(t)))
            }
            Some("html") | Some("www") => {
                self.render_table(flags, name, t)?;
                Ok(None)
            }
            Some("sqlite") => Err("Cannot yet output to sqlite, aborting".into()),
            Some("ndjson") => Ok(Some(render_ndjson(t))),
            _ => Ok(Some(render_json(t))),
        }
    }

    fn save_table(&mut self, flags: &Flags, name: &str, t: &Table, render: Option<&TerminalDiffRender>) -> Res<()> {
        match self.encode_table(flags, name, t, render)? {
            Some(txt) => self.save_text(name, &txt),
            None => Ok(()),
        }
    }

    fn use_color(&self, flags: &Flags, output: &str) -> bool {
        let mut use_color = flags.terminal_format.as_deref() == Some("ansi");
        if flags.terminal_format.is_none()
            && output == "-"
            && matches!(self.output_format.as_str(), "copy" | "csv" | "psv")
        {
            use_color = std::io::stdout().is_terminal();
        }
        use_color
    }

    fn save_tables(&mut self, flags: &Flags, name: &str, t: &Table, use_color: bool, is_diff: bool) -> Res<()> {
        if self.output_format != "copy" {
            let f = self.output_format.clone();
            self.set_format(&f);
        }
        self.check_format(Some(name));
        let render = use_color
            .then(|| TerminalDiffRender::new(Some(flags), self.delim_preference.as_deref(), is_diff));
        self.save_table(flags, name, t, render.as_ref())
    }
}

const USAGE: &str = "\
daff can produce and apply tabular diffs.
Call as:
  daff a.csv b.csv
  daff [--color] [--no-color] [--output OUTPUT.csv] a.csv b.csv
  daff [--output OUTPUT.html] a.csv b.csv
  daff parent.csv a.csv b.csv
  daff patch [--inplace] a.csv patch.csv
  daff merge [--inplace] parent.csv a.csv b.csv
  daff trim [--output OUTPUT.csv] source.csv
  daff render [--output OUTPUT.html] diff.csv
  daff copy in.csv out.tsv
  daff in.csv
  daff version

The --inplace option to patch and merge will result in modification of a.csv.

If you need more control, here is the full list of flags:
  daff diff [--output OUTPUT.csv] [--context NUM] [--all] [--act ACT] a.csv b.csv
     --act ACT:     show only a certain kind of change (update, insert, delete, column)
     --all:         do not prune unchanged rows or columns
     --all-rows:    do not prune unchanged rows
     --all-columns: do not prune unchanged columns
     --color:       highlight changes with terminal colors (default in terminals)
     --context NUM: show NUM rows of context (0=none)
     --context-columns NUM: show NUM columns of context (0=none)
     --fail-if-diff: return status is 0 if equal, 1 if different, 2 if problem
     --id:          specify column name to use as primary key (repeat for multi-column key)
     --ignore:      specify column name to ignore completely (can repeat)
     --index:       include row/columns numbers from original tables
     --input-format [csv|tsv|ssv|psv]: set format to expect for input
     --eol [crlf|lf|cr|auto]: separator between rows of csv output.
     --no-color:    make sure terminal colors are not used
     --ordered:     assume row order is meaningful (default for CSV)
     --output-format [csv|tsv|ssv|psv|copy|html]: set format for output
     --padding [dense|sparse|smart]: set padding method for aligning columns
     --unordered:   assume row order is meaningless
     -w / --ignore-whitespace: ignore changes in leading/trailing whitespace
     -i / --ignore-case: ignore differences in case
     -d EPS / --ignore-epsilon EPS: ignore small floating point differences

  daff render [--output OUTPUT.html] [--css CSS.css] [--fragment] [--plain] diff.csv
     --css CSS.css: generate a suitable css file to go with the html
     --fragment:    generate just a html fragment rather than a page
     --plain:       do not use fancy utf8 characters to make arrows prettier
     --unquote:     do not quote html characters in html diffs
";

struct Opts {
    flags: Flags,
    output: Option<String>,
    fail_if_diff: bool,
    inplace: bool,
    order_set: bool,
    args: Vec<String>,
}

fn parse_opts(st: &mut State, argv: Vec<String>) -> Res<Opts> {
    let mut o = Opts {
        flags: Flags::default(),
        output: None,
        fail_if_diff: false,
        inplace: false,
        order_set: false,
        args: Vec::new(),
    };
    let mut it = argv.into_iter();
    let value = |it: &mut std::vec::IntoIter<String>, tag: &str| it.next().ok_or_else(|| format!("{tag} needs a value"));
    while let Some(tag) = it.next() {
        match tag.as_str() {
            "--output" => o.output = Some(value(&mut it, &tag)?),
            "--css" => {
                st.fragment = true;
                st.css_output = Some(value(&mut it, &tag)?);
            }
            "--fragment" => st.fragment = true,
            "--plain" => o.flags.use_glyphs = false,
            "--unquote" => o.flags.quote_html = false,
            "--all" => {
                o.flags.show_unchanged = true;
                o.flags.show_unchanged_columns = true;
            }
            "--all-rows" => o.flags.show_unchanged = true,
            "--all-columns" => o.flags.show_unchanged_columns = true,
            "--act" => {
                let act = value(&mut it, &tag)?;
                // `acts[x] = true` on a fresh map, as the reference does
                o.flags.acts.get_or_insert_with(Default::default).insert(act, true);
            }
            "--context" => {
                if let Ok(n) = value(&mut it, &tag)?.parse() {
                    o.flags.unchanged_context = n;
                }
            }
            "--context-columns" => {
                if let Ok(n) = value(&mut it, &tag)?.parse() {
                    o.flags.unchanged_column_context = n;
                }
            }
            "--inplace" => o.inplace = true,
            "--unordered" => {
                o.flags.ordered = false;
                o.flags.unchanged_context = 0;
                o.order_set = true;
            }
            "--ordered" => {
                o.flags.ordered = true;
                o.order_set = true;
            }
            "--color" => o.flags.terminal_format = Some("ansi".into()),
            "--no-color" => o.flags.terminal_format = Some("plain".into()),
            "--input-format" => {
                let f = value(&mut it, &tag)?;
                st.set_format(&f);
            }
            "--output-format" => st.output_format = value(&mut it, &tag)?,
            "--id" => {
                let id = value(&mut it, &tag)?;
                o.flags.add_primary_key(&id);
            }
            "--ignore" => {
                let c = value(&mut it, &tag)?;
                o.flags.ignore_column(&c);
            }
            "--index" => {
                o.flags.always_show_order = true;
                o.flags.never_show_order = false;
            }
            "--www" => st.output_format = "www".into(),
            "--table" => {
                let t = value(&mut it, &tag)?;
                o.flags.add_table(&t);
            }
            "-w" | "--ignore-whitespace" => o.flags.ignore_whitespace = true,
            "-i" | "--ignore-case" => o.flags.ignore_case = true,
            "-d" | "--ignore-epsilon" => {
                let eps: f64 = value(&mut it, &tag)?
                    .parse()
                    .map_err(|_| "Epsilon for numeric comparison must be numeric".to_string())?;
                o.flags.ignore_epsilon = eps;
            }
            "--padding" => o.flags.padding_strategy = Some(value(&mut it, &tag)?),
            "-e" | "--eol" => {
                st.csv_eol_preference = match value(&mut it, &tag)?.as_str() {
                    "crlf" => Some("\r\n".into()),
                    "lf" => Some("\n".into()),
                    "cr" => Some("\r".into()),
                    "auto" => None,
                    other => return Err(format!("Expected line ending of either 'crlf' or 'lf' but got {other}")),
                };
            }
            "--fail-if-diff" => {
                o.fail_if_diff = true;
                FAIL_IF_DIFF.store(true, Ordering::Relaxed);
            }
            "-h" | "--help" | "help" => o.args.insert(0, "help".into()),
            _ => o.args.push(tag),
        }
    }
    Ok(o)
}

fn run(argv: Vec<String>) -> Res<u8> {
    let mut st = State::new();
    let Opts { mut flags, output, fail_if_diff, inplace, order_set, mut args } = parse_opts(&mut st, argv)?;
    if args.first().map(String::as_str) == Some("help") || args.is_empty() {
        eprint!("{USAGE}");
        return Ok(1);
    }
    let mut cmd = args[0].clone();
    let mut offset = 1;
    if !["diff", "patch", "merge", "trim", "render", "git", "version", "copy"].contains(&cmd.as_str()) {
        if cmd.starts_with("--") {
            cmd = "diff".into();
            offset = 0;
        } else if cmd.contains('.') {
            if args.len() == 2 {
                cmd = "diff".into();
                offset = 0;
            } else if args.len() == 1 {
                cmd = "copy".into();
                offset = 0;
            }
        }
    }
    if cmd == "version" {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    if cmd == "git" {
        return Err("git: not implemented yet".into());
    }
    let rest = args.split_off(offset);
    let mut offset = 0;
    let parent = if rest.len() >= 3 {
        offset = 1;
        Some(st.load_table(&rest[0])?)
    } else {
        None
    };
    let name = rest.get(offset).ok_or("expected a file name")?.clone();
    let mut a = st.load_table(&name)?;
    let mut b = None;
    let mut output = output;
    if rest.len() > offset + 1 {
        if cmd != "copy" {
            b = Some(st.load_table(&rest[offset + 1])?);
        } else {
            output = Some(rest[offset + 1].clone());
        }
    }
    if inplace {
        // The reference rejects --inplace unconditionally: it reports status 1
        // without writing anything. See docs/QUIRKS.md.
        if output.is_some() {
            eprintln!("Please do not use --inplace when specifying an output.");
        }
        return Ok(1);
    }
    let output = output.unwrap_or_else(|| "-".into());
    match cmd.as_str() {
        "diff" => {
            let b = b.ok_or("diff needs two tables")?;
            if !order_set {
                flags.ordered = st.order_preference;
                if !flags.ordered {
                    flags.unchanged_context = 0;
                }
            }
            flags.allow_nested_cells = st.nested_output;
            let align = compare_tables3(parent.as_ref(), &a, &b, Some(&flags)).align();
            let mut td = TableDiff::new(&align, flags.clone());
            let out = td.highlight();
            let color = st.use_color(&flags, &output);
            st.save_tables(&flags, &output, &out, color, true)?;
            if fail_if_diff && td.summary().different {
                return Ok(1);
            }
        }
        "trim" => st.save_table(&flags, &output, &a, None)?,
        "copy" => {
            let color = st.use_color(&flags, &output);
            st.save_tables(&flags, &output, &a, color, false)?
        }
        "render" => st.render_table(&flags, &output, &a)?,
        "patch" => {
            let patch = b.ok_or("patch needs a table and a patch")?;
            HighlightPatch::new(&mut a, &patch).apply().map_err(|e| e.to_string())?;
            st.save_table(&flags, &output, &a, None)?;
        }
        "merge" => {
            let remote = b.ok_or("merge needs parent, local and remote tables")?;
            let parent = parent.ok_or("merge needs parent, local and remote tables")?;
            let conflicts = Merger::new(&parent, &mut a, &remote, flags.clone()).apply().map_err(|e| e.to_string())?;
            if conflicts > 0 {
                eprintln!("{conflicts} conflict{}", if conflicts > 1 { "s" } else { "" });
            }
            st.save_table(&flags, &output, &a, None)?;
            if conflicts > 0 {
                return Ok(if fail_if_diff { 2 } else { 1 });
            }
        }
        _ => unreachable!(),
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("daff: {e}");
            ExitCode::from(if FAIL_IF_DIFF.load(Ordering::Relaxed) { 2 } else { 1 })
        }
    }
}
