//! `daff` command line tool. Argument handling and format selection follow the
//! reference implementation (`coopy.Coopy.run`): options may appear anywhere,
//! `diff`/`copy` are implied by file names, and formats are picked from file
//! extensions.
use daff::format::csv::Csv;
use daff::format::json::{parse_ndjson, render_json, render_ndjson, table_from_json, tables_from_json};
use daff::value::parse_json;
use daff::{
    compare_tables3, DiffRender, Flags, HighlightPatch, Merger, Table, TableDiff, Tables, TerminalDiffRender,
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
    /// The first CSV file read started with a byte order mark.
    bom_preference: bool,
    /// Write that byte order mark in front of CSV output (data commands only).
    write_bom: bool,
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
            bom_preference: false,
            write_bom: false,
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

    fn load_table(&mut self, name: &str, flags: &Flags) -> Res<Table> {
        let ext = self.check_format(Some(name));
        if ext == "sqlite" {
            return Err(self.unsupported("format 'sqlite'"));
        }
        let bytes = std::fs::read(name).map_err(|e| format!("{name}: {e}"))?;
        let text = if daff::reference_compat() {
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            String::from_utf8(bytes).map_err(|e| {
                format!("{name}: not valid UTF-8 (first bad byte at offset {}); convert the file to UTF-8 first", e.utf8_error().valid_up_to())
            })?
        };
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
                        return tables_from_json(&json, flags).map_err(|e| e.to_string());
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
        self.bom_preference |= parsed.bom;
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
                let mut txt = csv.render_table(t);
                if self.write_bom && self.bom_preference {
                    txt.insert(0, '\u{feff}');
                }
                Ok(Some(txt))
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

    fn save_tables(&mut self, flags: &Flags, name: &str, os: &Tables, use_color: bool, is_diff: bool) -> Res<()> {
        if self.output_format != "copy" {
            let f = self.output_format.clone();
            self.set_format(&f);
        }
        self.check_format(Some(name));
        let render = use_color
            .then(|| TerminalDiffRender::new(Some(flags), self.delim_preference.as_deref(), is_diff));
        let order = os.order();
        let one = os.one().ok_or("no table to write")?;
        if order.len() == 1 {
            return self.save_table(flags, name, one, render.as_ref());
        }
        if matches!(self.format_preference.as_deref(), Some("html") | Some("www")) {
            let mut renderer = DiffRender::new();
            renderer.use_pretty_arrows(flags.use_glyphs);
            renderer.quote_html(flags.quote_html);
            renderer.render_tables(os);
            return self.apply_renderer(name, renderer);
        }
        let mut txt = String::new();
        let mut need_blank = false;
        if os.has_ins_del() {
            txt += &self.encode_table(flags, name, one, render.as_ref())?.unwrap_or_else(|| "null".into());
            need_blank = true;
        }
        for n in order.iter().skip(1) {
            if let Some(t) = os.get(n) {
                if need_blank {
                    txt.push('\n');
                }
                need_blank = true;
                txt += n;
                txt.push('\n');
                txt += &"=".repeat(n.encode_utf16().count());
                txt.push('\n');
                txt += &self.encode_table(flags, name, t, render.as_ref())?.unwrap_or_else(|| "null".into());
            }
        }
        self.save_text(name, &txt)
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
     --reference-compat: reproduce the bugs of the original daff 1.4.2 (see docs/BUGS.md)

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
    git: bool,
    order_set: bool,
    args: Vec<String>,
}

fn parse_opts(st: &mut State, argv: Vec<String>) -> Res<Opts> {
    let mut o = Opts {
        flags: Flags::default(),
        output: None,
        fail_if_diff: false,
        inplace: false,
        git: false,
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
                if !daff::reference_compat() && !["update", "insert", "delete", "column"].contains(&act.as_str()) {
                    return Err(format!("--act expects update, insert, delete or column, not '{act}'"));
                }
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
            "--git" => o.git = true,
            "--reference-compat" => daff::set_reference_compat(true),
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

/// `Coopy.command`: echo, then run a command with inherited output.
fn command(cmd: &str, args: &[&str]) -> i32 {
    let mut line = format!("$ {cmd}");
    for a in args {
        line.push(' ');
        if a.contains(' ') {
            line.push_str(&format!("\"{a}\""));
        } else {
            line.push_str(a);
        }
    }
    println!("{line}");
    let _ = std::io::stdout().flush();
    match std::process::Command::new(cmd).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(_) => 127,
    }
}

/// `daff git csv tsv ...`: configure git to use daff for diffs and merges.
fn install_git_driver(formats: &[String]) -> Res<u8> {
    print!("Setting up git to use daff on");
    for f in formats {
        print!(" *.{f}");
    }
    println!(" files");
    if command("git", &["--version"]) != 0 {
        eprintln!("! Cannot run git, aborting");
        return Ok(1);
    }
    println!("- Can run git");
    let mut daff_cmd = String::new();
    for d in ["daff", "daff.rb", "daff.py"] {
        if command(d, &["version"]) == 0 {
            daff_cmd = d.to_string();
            println!("- Can run {d} as \"{d}\"");
            break;
        }
    }
    if daff_cmd.is_empty() {
        eprintln!("! Cannot find daff, is it in your path?");
        return Ok(1);
    }
    for f in formats {
        let have_diff = command("git", &["config", "--global", "--get", &format!("diff.daff-{f}.command")]) == 0;
        command("git", &["config", "--global", &format!("diff.daff-{f}.command"), &format!("{daff_cmd} diff --git")]);
        if have_diff {
            println!("- Cleared existing daff diff driver for {f}");
        }
        println!("- Added diff driver for {f}");
        let have_merge = command("git", &["config", "--global", "--get", &format!("merge.daff-{f}.driver")]) == 0;
        if !have_merge {
            command("git", &["config", "--global", &format!("merge.daff-{f}.name"), &format!("daff tabular {f} merge")]);
        }
        command(
            "git",
            &["config", "--global", &format!("merge.daff-{f}.driver"), &format!("{daff_cmd} merge --output %A %O %A %B")],
        );
        if have_merge {
            println!("- Cleared existing daff merge driver for {f}");
        }
        println!("- Added merge driver for {f}");
    }
    if !std::path::Path::new(".git/config").exists() {
        eprintln!("! This next part needs to happen in a git repository.");
        eprintln!("! Please run again from the root of a git repository.");
        return Ok(1);
    }
    let attr = ".gitattributes";
    let mut txt = String::new();
    if !std::path::Path::new(attr).exists() {
        println!("- No .gitattributes file");
    } else {
        println!("- You have a .gitattributes file");
        txt = std::fs::read_to_string(attr).map_err(|e| e.to_string())?;
    }
    let mut need_update = false;
    let mut post = String::new();
    for f in formats {
        if txt.contains(&format!("*.{f}")) {
            eprintln!("- Your .gitattributes file already mentions *.{f}");
        } else {
            post.push_str(&format!("*.{f} diff=daff-{f}\n"));
            post.push_str(&format!("*.{f} merge=daff-{f}\n"));
            println!("- Placing the following lines in .gitattributes:");
            print!("{post}");
            if !txt.is_empty() && !need_update {
                txt.push('\n');
            }
            txt.push_str(&post);
            need_update = true;
        }
    }
    if need_update {
        std::fs::write(attr, txt).map_err(|e| e.to_string())?;
    }
    println!("- Done!");
    Ok(0)
}

fn run(argv: Vec<String>) -> Res<u8> {
    if std::env::var_os("DAFF_REFERENCE_COMPAT").is_some_and(|v| v != "0") {
        daff::set_reference_compat(true);
    }
    let mut st = State::new();
    let Opts { mut flags, output, fail_if_diff, inplace, git, order_set, mut args } = parse_opts(&mut st, argv)?;
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
        let formats = args.split_off(offset);
        return install_git_driver(&formats);
    }
    let mut rest = args.split_off(offset);
    if git {
        // Called by git as an external diff driver: 7 or 9 parameters.
        let ct = rest.len();
        if ct != 7 && ct != 9 {
            eprintln!("Expected 7 or 9 parameters from git, but got {ct}");
            return Ok(1);
        }
        let old_display_path = rest[0].clone();
        let mut new_display_path = rest[0].clone();
        let (old_file, new_file) = (rest[1].clone(), rest[4].clone());
        if ct == 9 {
            print!("{}", rest[8]);
            new_display_path = rest[7].clone();
        }
        print!("--- a/{old_display_path}\n");
        print!("+++ b/{new_display_path}\n");
        rest = vec![old_file, new_file];
    }
    let mut offset = 0;
    let parent = if rest.len() >= 3 {
        offset = 1;
        Some(st.load_table(&rest[0], &flags)?)
    } else {
        None
    };
    let name = rest.get(offset).ok_or("expected a file name")?.clone();
    let mut a = st.load_table(&name, &flags)?;
    let mut b = None;
    let mut output = output;
    if rest.len() > offset + 1 {
        if cmd != "copy" {
            b = Some(st.load_table(&rest[offset + 1], &flags)?);
        } else {
            output = Some(rest[offset + 1].clone());
        }
    }
    if !daff::reference_compat() && matches!(cmd.as_str(), "diff" | "merge") {
        // Column names given to --id / --ignore must exist (B-21).
        let tables: Vec<&Table> = parent.iter().chain(std::iter::once(&a)).chain(b.iter()).collect();
        let nested = tables.iter().any(|t| t.meta().is_some_and(|m| m.nested));
        let known = |n: &str| tables.iter().any(|t| (0..t.width()).any(|x| t.get(x, 0) == Some(n)));
        if !nested {
            for (opt, names) in [("--id", &flags.ids), ("--ignore", &flags.columns_to_ignore)] {
                for n in names.iter().flatten() {
                    if !known(n) {
                        return Err(format!("{opt} column '{n}' is not in the header of any input table"));
                    }
                }
            }
        }
    }
    let mut output = output;
    if inplace {
        if daff::reference_compat() {
            // The reference rejects --inplace unconditionally: it reports
            // status 1 without writing anything (B-23).
            if output.is_some() {
                eprintln!("Please do not use --inplace when specifying an output.");
            }
            return Ok(1);
        }
        if output.is_some() {
            return Err("Please do not use --inplace when specifying an output.".into());
        }
        if !matches!(cmd.as_str(), "patch" | "merge") {
            return Err("--inplace only applies to patch and merge".into());
        }
        output = Some(name.clone());
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
            let os = td.hilite_with_nesting().map_err(|e| e.to_string())?;
            let color = st.use_color(&flags, &output);
            st.save_tables(&flags, &output, &os, color, true)?;
            if fail_if_diff && td.summary().different {
                return Ok(1);
            }
        }
        "trim" => {
            st.write_bom = true;
            st.save_table(&flags, &output, &a, None)?
        }
        "copy" => {
            if a.meta().is_some_and(|m| m.nested) {
                return Err("cannot copy a multi-table JSON file".into());
            }
            st.write_bom = true;
            let mut os = Tables::new();
            *os.add("untitled") = a;
            let color = st.use_color(&flags, &output);
            st.save_tables(&flags, &output, &os, color, false)?
        }
        "render" => st.render_table(&flags, &output, &a)?,
        "patch" => {
            let patch = b.ok_or("patch needs a table and a patch")?;
            HighlightPatch::new(&mut a, &patch).apply().map_err(|e| e.to_string())?;
            st.write_bom = true;
            st.save_table(&flags, &output, &a, None)?;
        }
        "merge" => {
            let remote = b.ok_or("merge needs parent, local and remote tables")?;
            let parent = parent.ok_or("merge needs parent, local and remote tables")?;
            let conflicts = Merger::new(&parent, &mut a, &remote, flags.clone()).apply().map_err(|e| e.to_string())?;
            if conflicts > 0 {
                eprintln!("{conflicts} conflict{}", if conflicts > 1 { "s" } else { "" });
            }
            st.write_bom = true;
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
