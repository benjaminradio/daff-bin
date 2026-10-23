# Aligning the daff command line with diff, patch, diff3 and git diff

Proposal for phase 3. Breaking changes are acceptable, so the question is how
far to go. The rule: **if an option exists in `diff` (GNU diffutils 3.10),
`patch` (2.7), `diff3` or `git diff`, and it makes sense for tables, `daff`
should use the same name with the same meaning.** Where a standard flag has no
tabular meaning it is simply absent. Where `daff` needs something the standard
tools lack, it gets a long option that does not collide with a standard one.

This replaces the option-level parts of `docs/CLI.md` (which remains the review
of missing features). Nothing here is implemented yet. The option tables were
checked against `diff --help`, `patch --help`, `diff3 --help` and
`git diff -h` on this machine.

## Principles

1. **Operands and exit status behave like the tools people know.**
   `daff FILE1 FILE2`, `-` for standard input, directories, `--` to end options;
   exit status 0 = no differences, 1 = differences, 2 = trouble (for `patch`:
   0 applied, 1 some parts rejected, 2 trouble; for `merge`: 0 clean, 1
   conflicts, 2 trouble).
2. **Same flag, same meaning.** A short flag is never reused for something else.
   That forces renames of a few existing options (listed below).
3. **The tabular diff stays daff's own format.** The standard tools have a
   default output (`normal`) that is not self-describing; ours (the highlighter
   table) is, and is what `patch` reads. So the default output is unchanged and
   the standard formats are choices: `-u`, `-c`, `-y`, `--normal`.
4. **Row-oriented text formats are first-class.** Unified output (one table row
   per line, `-`/`+` for removed/added rows, a `@@ -2,3 +2,4 @@` hunk header,
   the header row as context) is readable by every pager, editor, code review
   tool and `git`, and costs little to produce.
5. **Helpful about removed options.** An old option that is gone fails with a
   one-line message naming its replacement, never silently does something else.

## Commands and operands

| Command | Today | Proposed |
|---------|-------|----------|
| `daff A B` | tabular diff | same; `daff diff A B` is the explicit form |
| `daff diff A B C` (3 files) | 3-way diff, order parent/local/remote | removed; use `daff diff3 MYFILE OLDFILE YOURFILE` (diff3's order) |
| `daff patch ORIG PATCHFILE` | prints result to stdout | as `patch(1)`: edits ORIG in place; `-o FILE` (`-o -` for stdout); stdin when PATCHFILE omitted (`-i`) |
| `daff merge PARENT LOCAL REMOTE` | prints to stdout, parent first | `daff merge MYFILE OLDFILE YOURFILE` like `diff3 -m`: result to stdout, `-o FILE`; exit 1 on conflicts |
| `daff diff3 MYFILE OLDFILE YOURFILE` | – | the 3-way diff of today (what to change in MYFILE to take YOURFILE's changes since OLDFILE) |
| `daff render`, `trim`, `copy`, `git`, `version` | as is | `render` and `trim`/`copy` unchanged (not diff concepts); `daff version` also as `-v/--version` |
| directories | error | `daff -r DIR1 DIR2`, see below |

`git diff` forms are added as operand syntax rather than new commands:
`daff REV:PATH FILE`, `daff --cached [REV] [PATH]`, `daff --no-index A B`
(runs `git show` for the revision side; needs git on the path).

## Option mapping

### diff(1) options

"Yes" = implement with the same meaning. "Renamed" = daff's current option moves.

| diff option | For tables | Decision |
|-------------|------------|----------|
| `--normal` | row-oriented classic hunks (`2c2`, `< row`, `> row`) | Yes, low priority |
| `-q, --brief` | "Files A and B differ" | Yes |
| `-s, --report-identical-files` | "Files A and B are identical" | Yes |
| `-c, -C NUM, --context[=NUM]` | copied-context format | Yes, low priority. Renamed: today's `--context N` means "rows of context" and becomes `-U N` |
| `-u, -U NUM, --unified[=NUM]` | unified rows format; `NUM` = context rows | Yes (high). For the default tabular output `-U NUM` also sets the row context (today's `--context`); `-U` without a number is 3 as in diff, absent it stays 1 for the tabular format (see questions) |
| `-e, --ed`, `-n, --rcs` | scripts for line editors | No |
| `-y, --side-by-side`, `-W, --width`, `--left-column`, `--suppress-common-lines` | old table left, new table right, changed cells highlighted | Yes (medium) |
| `-p, --show-c-function`, `-F RE` | nearest enclosing function line | No. (Possible reinterpretation: show the nearest row whose key matches RE; not planned) |
| `--label LABEL` | names in headers of unified/side-by-side/git output | Yes |
| `-t, -T, --tabsize, --suppress-blank-empty` | tab handling in output | `-t`/`-T`/`--tabsize` only for side-by-side/unified; low priority |
| `-l, --paginate` | pager | Yes (also `--no-pager`) |
| `-r, --recursive` | compare directories | Yes (medium) |
| `-N, --new-file`, `--unidirectional-new-file` | absent file = empty table | Yes |
| `-x PAT`, `-X FILE`, `-S FILE` | exclude / start with | Yes |
| `--from-file=F1`, `--to-file=F2` | compare one file to many | Yes |
| `--no-dereference`, `--ignore-file-name-case`, `--no-ignore-file-name-case` | directory traversal | Yes, trivial |
| `-i, --ignore-case` | case-insensitive cell comparison | Yes (exists) |
| `-E, --ignore-tab-expansion` | tabs vs spaces | No |
| `-Z, --ignore-trailing-space` | trailing whitespace in cells | Yes |
| `-b, --ignore-space-change` | runs of whitespace equal | Yes |
| `-w, --ignore-all-space` | all whitespace ignored | Yes. Renamed: today's `-w` (trim only) becomes `-Z` plus leading trim; `-w` now removes all whitespace |
| `-B, --ignore-blank-lines` | ignore blank rows | Yes |
| `-I RE, --ignore-matching-lines` | ignore a change when old and new cell both match RE (timestamps, ids) | Yes, as `-I RE` on cells |
| `-a, --text`, `--binary` | binary files | No (non-tables are skipped with a notice) |
| `--strip-trailing-cr` | CR at end of row | Yes (rows) and `--strip-cell-cr` for CR inside cells (G-11) |
| `-d, --minimal`, `--speed-large-files`, `--horizon-lines` | diff algorithm effort | No. Renamed: `-d EPS` (epsilon) becomes `--ignore-epsilon EPS` |
| `-D, --ifdef`, `--GTYPE-group-format`, `--line-format` | templated output | No (a `--format=TEMPLATE` for cell-level output may come later) |
| `--color[=WHEN]`, `--palette=PAL` | colour | Yes: `--color=auto\|always\|never`, `NO_COLOR` honoured, auto = stdout is a terminal. `--no-color` kept as git has it |
| `-v, --version`, `--help` | | Yes; `--help` per command |

### git diff options

| git option | Decision |
|------------|----------|
| `--stat`, `--numstat`, `--shortstat`, `--summary` | Yes (high): rows added/deleted/changed, columns added/removed, per table in multi-table and directory diffs |
| `--name-only`, `--name-status` | Yes for directories and multi-table diffs (changed files or tables) |
| `--exit-code`, `--quiet` | Yes, but this becomes the default exit status (like `diff`); `--exit-code` accepted as a no-op alias |
| `-R` | Yes: swap the two inputs |
| `--diff-filter=[ADMC…]` | Yes. Replaces `--act`: `A` inserted rows, `D` deleted, `M` modified, `C` column changes (git uses the letters for files; same idea) |
| `--word-diff[=color\|plain\|porcelain]` | Yes, low priority: word-level diff inside a changed text cell (G-13) |
| `-z` | Yes for names output |
| `--output=<file>` | Yes (`-o` as in `patch`) |
| `-M`, `-C`, `-B` (rename and copy detection) | Directory diffs: maybe file renames later; columns already have rename detection |
| `-S`, `-G`, `--pickaxe-all` (search changes) | Maybe later: `-G RE` shows only rows whose changed cell matches RE |
| `--cached`, `--no-index`, `<rev>:<path>` | Yes as operands |
| `--patience`, `--minimal`, `--histogram`, `--submodule`, `--ws-error-highlight` | No |

### patch(1) options

| patch option | Decision |
|--------------|----------|
| `[ORIGFILE [PATCHFILE]]`, `-i PATCHFILE`, stdin | Yes |
| `-o FILE`, `--output` | Yes. **Default becomes editing ORIGFILE in place** (patch's behaviour) |
| `-R, --reverse` | Yes (G-19): needs the diff inversion |
| `--dry-run` | Yes (what `docs/CLI.md` called `--check`) |
| `-N, --forward` | Yes: skip already-applied/reversed parts silently. Without it, an already-applied part is not an error either (phase 2 behaviour); `-N` mostly silences the report |
| `-f, --force`, `-t, --batch` | `--force` exists. `-t` = never ask; `daff` never asks, accepted as no-op |
| `-r FILE, --reject-file` | Yes (high value): rejected diff rows (the `PatchIssue`s) are written as a diff file that can be reviewed and re-applied |
| `--merge` | Yes: put conflict markers in the table instead of rejecting |
| `-b, --backup`, `-z SUFFIX`, `-B PREFIX`, `-V STYLE` | Yes, simplified (`-b`, `-z`, `--backup-if-mismatch`) |
| `-s, --quiet`, `--verbose` | Yes |
| `-d DIR` | Yes |
| `-p NUM`, `-F LINES`, `-l, --ignore-whitespace`, `-c/-e/-n/-u` | `-l` yes (match cells ignoring whitespace); `-u`/`-c` yes if those input formats exist; `-p` and `-F` no |
| `-E, --remove-empty-files` | Yes (delete output that is an empty table) |

### diff3 and `git merge-file` options

| option | Decision |
|--------|----------|
| `-m, --merge` | `daff merge` is `diff3 -m`; `daff diff3` without it shows the changes |
| `-A, --show-all` | Yes: bracket every conflict in the merged table (today's `((( p ))) l /// r` cells; `-A` is the default for `merge`) |
| `-L LABEL` (up to three) | Yes; used in conflict markers and headers |
| `-e`, `-E`, `-3`, `-x`, `-X`, `-i` | No (ed scripts) |
| `--ours`, `--theirs`, `--union` (from `git merge-file`) | Yes: conflict strategies (`docs/CLI.md` item 5) |
| `-p, --stdout` (git merge-file) | the default for us |
| `--diff-program`, `-T` | No |

## Existing daff options: what happens to them

| Today | After |
|-------|-------|
| `--output F` | kept as `-o F` / `--output=F` |
| `--context N`, `--context-columns N` | `-U N` / `--unified=N` and `--context-columns N` (long only) |
| `--all`, `--all-rows`, `--all-columns` | kept; `--all` is the same as a very large `-U` |
| `-w` | now `-Z` (trailing) and leading-trim kept under `--ignore-edge-space`; `-w` is "ignore all whitespace" |
| `-d EPS` | `--ignore-epsilon EPS` only |
| `-e, --eol` | `--eol` only |
| `--fail-if-diff` | removed (default exit status); a hint is printed |
| `--inplace` | removed for `patch` (default); `merge -o MYFILE` |
| `--act X` | `--diff-filter=…` |
| `--id`, `--ignore`, `--index`, `--ordered`, `--unordered`, `--table`, `--padding` | kept (no standard equivalent); `--key` added as alias of `--id`, `--ignore` also as `--ignore-column` |
| `--input-format`, `--output-format` | `--from-format` and `--format`; `-u`, `-c`, `-y` are shorthands for `--format=unified` etc. |
| `--color`, `--no-color` | `--color[=WHEN]`, `--no-color` kept |
| `--git` | kept (git calls `daff diff --git`); the installer is rewritten for the new syntax |
| `--reference-compat` | kept for data behaviour; `DAFF_LEGACY_CLI=1` could accept the old option names for a transition period (decision below) |

## New output formats this brings in

`--format=` (or its shorthands) selects: `daff` (default tabular highlighter),
`unified` (`-u`), `context` (`-c`), `normal`, `side-by-side` (`-y`), `html`,
`json` (the sheet), and, from `docs/CLI.md`, `changes` (JSON list of changes),
`jsonl`, `md`, `sql`. Statistics (`--stat`, `--numstat`, `--shortstat`) and name
lists (`--name-only`, `--name-status`) are alternatives to a format, as in git.

Unified output for a table (sketch):

```
--- a/orders.csv
+++ b/orders.csv
@@ id,customer,total @@
 1,ann,10
-2,bob,15
+2,bob,20
 3,cy,7
+4,di,3
```

Updates become a `-`/`+` pair of rows (the unified format has no cell level), with
`--word-diff` or colour marking the changed cells; inserted and deleted columns
are shown by changing the header line (`--- header` / `+++ header` pair).

## What does not map

Everything that exists because `diff` works on lines of text: ed/RCS/ifdef
output, function-name hints, tab expansion, binary detection, diff-algorithm
tuning. And things `daff` has that diff does not: keys (`--id`), column
operations, `--index`, multi-table input, formats other than CSV. Those keep
long names and are documented as daff extensions.

## Implementation notes

- Replace the hand-written argument loop with `clap` (derive). It gives GNU-style
  bundled short flags (`-qs`, `-U3`), `--long=value` and `--long value`,
  `--` handling, unambiguous long-option prefixes (`--col` for `--color`),
  per-command `--help`, and shell completion and man page generation
  (`clap_complete`, `clap_mangen`) for free. The binary grows by a few hundred KB.
- Output formats become small renderers over the existing alignment, so
  unified/side-by-side/stat/names need no new matching logic.
- Directory diff reuses the multi-table machinery (`Tables`); a file pair is a
  table pair, absent files follow `-N`.
- The conformance harness keeps working through `--reference-compat` plus a
  legacy-CLI switch (so the oracle scripts need not change), then the harness
  gains the standard-flag tests.
- Test the new surface against GNU `diff`/`patch` behaviour where it carries
  over: exit statuses, `-q`/`-s` messages, `-N`, `-r` output headers, `--label`.

## Suggested order

1. Parser on clap; exit statuses; `-` for stdin; `-q`, `-s`, `-v/--version`,
   `--help`, `-o`, `--color[=WHEN]`/`NO_COLOR`, `-i -b -w -Z -B`, `-U`, `--label`,
   removed-option hints. (Mostly renames; no new output.)
2. `patch` and `merge` as `patch(1)` and `diff3 -m`: in-place default, `-i/-o`,
   `--dry-run`, `-b`, `-r`, `-R` (with `invert`), `--merge`, `--ours/--theirs`.
3. New outputs: `-u`, `--stat/--numstat/--shortstat`, `--name-only/--name-status`,
   `--format=changes/jsonl/md`.
4. Directories and git operands: `-r -N -x -X`, `REV:PATH`, `--cached`, `--no-index`.
5. `-y` side-by-side, `-I`, `--word-diff`, `-l` pager, `--diff-filter`,
   `-c`/`--normal`, `sql`, `--www`.

## Questions for you

1. **Default output**: keep the tabular highlighter (my recommendation: it is
   self-describing and `patch` reads it), or make unified rows the default as
   `git diff` users would expect?
2. **Context default**: `-U` without a number is 3 as in diff; with no `-U` the
   tabular format keeps its default of 1. Or make 3 the default everywhere?
3. **`patch` in place by default**: matches `patch(1)`, but is destructive.
   Accept (with `--dry-run`, `-b`, and `-o -` to print), or keep stdout the
   default and require `--inplace`/`-i`? (GNU `-i` is the input file, so `-i` is
   taken.)
4. **`merge` operand order**: switch to diff3's `MYFILE OLDFILE YOURFILE`
   (breaks the order that the current git driver setup uses, which `daff git`
   would rewrite) or keep parent first?
5. **`-w` meaning**: adopt GNU's (ignore all whitespace) and move the old
   trim-ends behaviour to `-Z`/`--ignore-edge-space`?
6. **Transition**: hard errors with a hint for removed options, or a
   `DAFF_LEGACY_CLI=1` switch that keeps accepting them for a release?
7. **Directory diff scope**: compare only files with known table extensions
   (csv, tsv, json, ndjson, later xlsx) and note the rest as "Only in…"/skipped?
8. **Dependencies**: OK to add `clap` (and later `clap_complete`/`clap_mangen`)?
