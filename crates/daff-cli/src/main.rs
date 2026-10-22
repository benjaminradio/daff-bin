//! `daff` command line tool. Argument handling follows the reference
//! implementation: options may appear anywhere, and `diff`/`copy` are implied
//! by file names.
use daff::format::csv::Csv;
use daff::{compare_tables3, Flags, HighlightPatch, Merger, Table, TableDiff};
use std::io::Write;
use std::process::ExitCode;

#[derive(Default)]
struct Io {
    delim: String,
    eol: Option<String>,
    explicit_format: bool,
}

fn delim_for_ext(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_lowercase();
    match ext.as_str() {
        "csv" => Some(","),
        "tsv" => Some("\t"),
        "ssv" => Some(";"),
        "psv" => Some("\u{1F4A9}"),
        _ => None,
    }
}

fn delim_for_format(name: &str) -> Option<&'static str> {
    delim_for_ext(&format!(".{name}"))
}

impl Io {
    fn check_format(&mut self, name: &str) -> Result<(), String> {
        if self.explicit_format {
            return Ok(());
        }
        if let Some(d) = delim_for_ext(name) {
            self.delim = d.into();
        } else if let Some((_, ext)) = name.rsplit_once('.') {
            if matches!(
                ext.to_lowercase().as_str(),
                "json" | "ndjson" | "sqlite" | "sqlite3" | "html" | "htm" | "www"
            ) {
                return Err(format!("format '{ext}' is not supported yet"));
            }
        }
        Ok(())
    }

    fn load(&mut self, name: &str) -> Result<Table, String> {
        self.check_format(name)?;
        let bytes = std::fs::read(name).map_err(|e| format!("{name}: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        let parsed = Csv::with_delimiter(&self.delim).parse_full(&text);
        // The reference traces these to stdout, which corrupts piped output;
        // we write them to stderr instead (the one deliberate deviation).
        for w in &parsed.warnings {
            eprintln!("{w}");
        }
        let mut t = parsed.table;
        if self.eol.is_none() {
            self.eol = parsed.eol;
        }
        t.trim_blank();
        Ok(t)
    }

    fn save(&mut self, output: &str, t: &Table) -> Result<(), String> {
        self.check_format(output)?;
        let csv = Csv { delimiter: self.delim.clone(), eol: self.eol.clone() };
        let txt = csv.render_table(t);
        if output == "-" {
            std::io::stdout().write_all(txt.as_bytes()).map_err(|e| e.to_string())
        } else {
            std::fs::write(output, txt).map_err(|e| format!("{output}: {e}"))
        }
    }
}

const USAGE: &str = "\
daff can produce and apply tabular diffs.
Call as:
  daff a.csv b.csv
  daff [--output OUTPUT.csv] a.csv b.csv
  daff parent.csv a.csv b.csv
  daff trim [--output OUTPUT.csv] source.csv
  daff copy in.csv out.tsv
  daff in.csv
  daff version

  daff diff [--output OUTPUT.csv] [--context NUM] [--all] [--act ACT] a.csv b.csv
     --act ACT:     show only a certain kind of change (update, insert, delete, column)
     --all:         do not prune unchanged rows or columns
     --all-rows:    do not prune unchanged rows
     --all-columns: do not prune unchanged columns
     --context NUM: show NUM rows of context (0=none)
     --context-columns NUM: show NUM columns of context (0=none)
     --fail-if-diff: return status is 0 if equal, 1 if different, 2 if problem
     --id:          specify column name to use as primary key (repeat for multi-column key)
     --ignore:      specify column name to ignore completely (can repeat)
     --index:       include row/columns numbers from original tables
     --input-format [csv|tsv|ssv|psv]: set format to expect for input
     --eol [crlf|lf|cr|auto]: separator between rows of csv output.
     --output-format [csv|tsv|ssv|psv|copy]: set format for output
     --ordered:     assume row order is meaningful (default for CSV)
     --unordered:   assume row order is meaningless
     -w / --ignore-whitespace: ignore changes in leading/trailing whitespace
     -i / --ignore-case: ignore differences in case
     -d EPS / --ignore-epsilon EPS: ignore small floating point differences
";

struct Opts {
    flags: Flags,
    output: Option<String>,
    fail_if_diff: bool,
    inplace: bool,
    order_set: bool,
    args: Vec<String>,
}

fn parse_opts(io: &mut Io, argv: Vec<String>) -> Result<Opts, String> {
    let mut o = Opts {
        flags: Flags::default(),
        output: None,
        fail_if_diff: false,
        inplace: false,
        order_set: false,
        args: Vec::new(),
    };
    let mut it = argv.into_iter();
    let value = |it: &mut std::vec::IntoIter<String>, tag: &str| {
        it.next().ok_or_else(|| format!("{tag} needs a value"))
    };
    while let Some(tag) = it.next() {
        match tag.as_str() {
            "--output" => o.output = Some(value(&mut it, &tag)?),
            "--all" => {
                o.flags.show_unchanged = true;
                o.flags.show_unchanged_columns = true;
            }
            "--all-rows" => o.flags.show_unchanged = true,
            "--all-columns" => o.flags.show_unchanged_columns = true,
            "--act" => {
                let act = value(&mut it, &tag)?;
                o.flags.filter(&act, true);
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
            "--unordered" => {
                o.flags.ordered = false;
                o.flags.unchanged_context = 0;
                o.order_set = true;
            }
            "--ordered" => {
                o.flags.ordered = true;
                o.order_set = true;
            }
            "--color" | "--no-color" | "--plain" | "--unquote" | "--fragment" => {}
            "--input-format" | "--output-format" => {
                let f = value(&mut it, &tag)?;
                if f != "copy" {
                    let d = delim_for_format(&f).ok_or_else(|| format!("format '{f}' is not supported yet"))?;
                    io.delim = d.into();
                    io.explicit_format = true;
                }
            }
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
            "-e" | "--eol" => {
                io.eol = match value(&mut it, &tag)?.as_str() {
                    "crlf" => Some("\r\n".into()),
                    "lf" => Some("\n".into()),
                    "cr" => Some("\r".into()),
                    "auto" => None,
                    other => return Err(format!("Expected line ending of either 'crlf' or 'lf' but got {other}")),
                };
            }
            "--fail-if-diff" => o.fail_if_diff = true,
            "--inplace" => o.inplace = true,
            "-h" | "--help" | "help" => o.args.insert(0, "help".into()),
            _ => o.args.push(tag),
        }
    }
    Ok(o)
}

fn run(argv: Vec<String>) -> Result<u8, String> {
    let mut io = Io { delim: ",".into(), ..Default::default() };
    let Opts { mut flags, output, fail_if_diff, inplace, order_set, mut args } = parse_opts(&mut io, argv)?;
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
    let rest = args.split_off(offset);
    let mut offset = 0;
    let parent = if rest.len() >= 3 {
        offset = 1;
        Some(io.load(&rest[0])?)
    } else {
        None
    };
    let name = rest.get(offset).ok_or("expected a file name")?.clone();
    let mut a = io.load(&name)?;
    let mut b = None;
    let mut output = output;
    if rest.len() > offset + 1 {
        if cmd != "copy" {
            b = Some(io.load(&rest[offset + 1])?);
        } else {
            output = Some(rest[offset + 1].clone());
        }
    }
    if inplace {
        // The reference rejects --inplace unconditionally: it reports status 1
        // without reading or writing anything. See docs/QUIRKS.md.
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
                flags.ordered = true;
            }
            let align = compare_tables3(parent.as_ref(), &a, &b, Some(&flags)).align();
            let mut td = TableDiff::new(&align, flags);
            let out = td.highlight();
            io.save(&output, &out)?;
            if fail_if_diff && td.summary().different {
                return Ok(1);
            }
        }
        "trim" | "copy" => io.save(&output, &a)?,
        "patch" => {
            let patch = b.ok_or("patch needs a table and a patch")?;
            HighlightPatch::new(&mut a, &patch).apply().map_err(|e| e.to_string())?;
            io.save(&output, &a)?;
        }
        "merge" => {
            let remote = b.ok_or("merge needs parent, local and remote tables")?;
            let parent = parent.ok_or("merge needs parent, local and remote tables")?;
            let conflicts = Merger::new(&parent, &mut a, &remote, flags).apply().map_err(|e| e.to_string())?;
            if conflicts > 0 {
                eprintln!("{conflicts} conflict{}", if conflicts > 1 { "s" } else { "" });
            }
            io.save(&output, &a)?;
            if conflicts > 0 {
                return Ok(if fail_if_diff { 2 } else { 1 });
            }
        }
        "render" | "git" => return Err(format!("{cmd}: not implemented yet")),
        _ => unreachable!(),
    }
    Ok(0)
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("daff: {e}");
            ExitCode::from(2)
        }
    }
}
