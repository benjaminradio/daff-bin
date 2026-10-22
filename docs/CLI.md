# Command line: what exists, what is missing

A review of the `daff` command line from an end user's point of view: what is
there today, which diff features are missing, and which small things are worth
adding. Nothing here is implemented yet. Items are ranked, and linked to the
gap analysis (`docs/GAPS.md`) where they depend on a format decision.

The comparison with other tools (csv-diff, csvdiff, qsv/xsv-style `diff`, `dolt
diff`, `git diff --word-diff`, Miller) is from memory of what those tools offer,
not checked against their current versions.

## What exists today

| Command | What it does |
|---------|--------------|
| `daff a.csv b.csv` / `daff diff` | tabular diff (3-way with a parent first); `--id`, `--ignore`, `--act`, `--all`, `--context`, `--index`, `--unordered`, `-i`, `-w`, `-d`, `--output`, `--output-format`, `--color`, `--fail-if-diff` |
| `daff patch a.csv diff.csv` | apply a diff (`--inplace`, `--force`) |
| `daff merge parent.csv a.csv b.csv` | 3-way merge (`--inplace`, conflicts counted) |
| `daff render diff.csv` | diff table to HTML (`--fragment`, `--css`, `--plain`) |
| `daff trim`, `daff copy` | normalise a table, convert between csv/tsv/ssv/psv/json/ndjson |
| `daff git csv` | install daff as git's diff and merge driver |
| `daff version` | print the version |

Formats: CSV family, JSON, NDJSON, multi-table JSON. No SQLite or xlsx yet.

## Missing: the larger ones

These change what a user can *do*, not how a flag is spelled.

1. **A machine-readable list of changes.** The only structured output is the
   diff as a sheet (`{"sheet": [[...]]}`), which a script has to interpret
   (action column, schema row, `a->b` strings). Users of csv-diff and
   `dolt diff` expect a list like
   `{"added": [...], "removed": [...], "changed": [{"key": ..., "column": ...,
   "from": ..., "to": ...}]}`, or one JSON object per change (JSON Lines) for
   streaming. This is the most common reason people reach for another tool.
   *Fix*: `--format changes` (JSON) and `--format changes-jsonl`; built from the
   same alignment, so it is cheap. Depends on key detection being reportable
   (G-01).
2. **A summary.** There is no "3 rows added, 1 removed, 5 cells changed, column
   `x` added". The library already computes it (`DiffSummary`); the CLI does not
   show it. *Fix*: `--stat` (human, `git diff --stat` style), `--summary` (one
   line), `--stat --format json`. Also the natural basis for scripting
   (`--exit-code`, below).
3. **Comparing more than two files.** Real use is rarely `a.csv b.csv`:
   - *directories*: `daff diff dir1 dir2` (pair files by name, report added and
     removed files; reuses the multi-table machinery and its output sections);
   - *git revisions*: `daff diff HEAD~3:data.csv data.csv` and
     `daff diff --staged`; today only the git driver (`daff git`) does this, and
     only for a whole repository setup;
   - *history*: `daff log data.csv` (a diff per commit that touched the file,
     summarised) and `daff blame data.csv` (which commit last changed each row
     or cell). This is the "git for tables" feature people ask for; it is the
     biggest item and not needed to be useful.
4. **Explaining the matching.** When a diff looks wrong the user cannot see why:
   which header row was chosen, which columns formed the key, which rows were
   matched weakly, which columns were treated as renamed. *Fix*:
   `daff explain a.csv b.csv` (or `diff --explain`) printing those decisions.
   It is also the user-facing side of G-01 and G-02 (warnings about weak keys
   and duplicate rows belong here).
5. **Workflow around patching.** `patch` applies or complains; users also need:
   - `patch --dry-run` (or `--check`): report problems without writing;
   - `daff invert diff.csv` and `patch --reverse` (G-19): undo a change;
   - for merges: `--ours` / `--theirs` strategies for conflicts, a conflict
     report (`--conflicts FILE`, G-21) and, optionally, `daff resolve`.
6. **Database-shaped output.** For tables that really are database tables,
   `diff --format sql --table t --id id` printing `INSERT`/`UPDATE`/`DELETE`
   statements (as `dolt diff --sql` does) is a direct, checkable way to apply a
   change elsewhere. Fits phase 5 (SQLite) but the output side needs no database.

## Missing: the smaller ones worth adding

Cheap, each independent, mostly polish that people notice immediately.

| Item | Why |
|------|-----|
| `-` for standard input (`cat a.csv \| daff diff - b.csv`) | currently "no such file"; expected everywhere |
| `--version` / `-V`; `daff help diff` per-command help | `--version` is an error today; help is one long text |
| `--exit-code` (`diff(1)`-style: 0 same, 1 different, 2 error) and `-q/--quiet` | `--fail-if-diff` exists but inverts the usual scripting convention and has no quiet mode; today a different pair exits 0 |
| `--color=auto\|always\|never` and `NO_COLOR` | `--color` / `--no-color` exist but `NO_COLOR` is ignored; `auto` should also look for a TTY on stdout only |
| Markdown output (`--output-format md`) | paste a diff into a pull request or chat; `render` only does HTML |
| `--key` as a friendlier name for `--id`; columns by number (`--id 1,3`); case-insensitive column names (`--ignore-header-case`) | key columns are the most-used option |
| `--only COLS` / `--columns COLS` (compare just these columns) | the complement of `--ignore`, e.g. "did anything in these three columns change?" |
| `--no-header`, `--header-row N`, `--delimiter C`, `--encoding E` | G-07, G-24; input flags that stop people hand-editing files first |
| `--www` | opens the HTML diff in a browser; the reference has it, we do not |
| Pager for long coloured output (`--pager` / `$PAGER`, off when piped) | `git diff` behaviour |
| Shell completions (`daff completions bash\|zsh\|fish`) and a man page | discoverability |
| Per-column tolerance (`-d amount=0.01,price=1%`) | `-d` is one number for every column; percentage tolerance is common |
| `--limit N` / `--max-changes N` (stop after N changes) | previews on big tables |
| `--sample` of changed rows only (no context) = `--context 0` with `--all-columns` | already expressible, but an alias helps |
| `-o`/`--output` to the same file as the input | currently overwrites after reading; fine but not documented; `--inplace` covers patch/merge |
| `daff check diff.csv` | validate that a file is a well-formed diff (has `@@`, valid tags), useful before patching |
| A config file (`.daff.toml` in the repository: key columns per file pattern, default options) | the git driver cannot pass `--id` per file; with a config file `git diff` knows that `orders.csv` is keyed by `order_id` |

## Not worth doing (leave to other tools)

Sorting, filtering, joining, aggregating and general CSV editing (Miller, qsv,
csvkit and DuckDB do this well); SQL queries as a way to select rows; a GUI.
`daff` should stay "compare, patch, merge, explain". Word-level diffs inside
cells (`--text-diff`, G-13) are possible but low priority.

## Suggested grouping for phase 3

1. *Quick wins* (one pass, no format decisions): stdin `-`, `--version`, per-command
   help, `--exit-code`/`-q`, `--color=when` and `NO_COLOR`, `--key`, `--stat` /
   `--summary`, Markdown output, shell completions, `patch --dry-run`, `check`.
2. *Structured output*: `--format changes` and `changes-jsonl`; `--explain`.
3. *Inputs*: `--no-header`, `--header-row`, `--delimiter`, `--encoding`, `--only`.
4. *Multi-file*: directory diff and `REV:path` arguments; `.daff.toml`.
5. *After the format decisions in `docs/GAPS.md`*: `invert`/`--reverse`,
   conflict report and strategies, `--www`, SQL output.
6. *Later or optional*: `log`, `blame`, per-column tolerance, pager.

## Decisions needed

- **Exit codes**: keep the reference's `--fail-if-diff` only, or add the
  `diff(1)` convention as `--exit-code` (and keep the default at 0)? A change
  of the default would break scripts written for daff.
- **Machine-readable format**: is the change list (item 1) a priority, and
  should its shape follow csv-diff (added / removed / changed) or be one record
  per cell change?
- **History commands** (`log`, `blame`): wanted, or out of scope for a diff tool?
- **Config file**: a `.daff.toml` in the repository, or rely on git attributes
  and command lines?
