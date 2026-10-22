# Gaps in the tabular diff specification, and what to do about them

Phase 2 made the implementation match the spec (v0.8) and its own intent.
This document is the start of phase 3. It asks a different question: where is
the *spec itself* silent, ambiguous or too weak for diffs of tabular data, so
that a correct implementation still cannot always do the right thing? For each
gap it records the evidence, whether it can be solved, the options, and a
decision — including "leave it". Nothing here has been implemented yet unless
a decision says so.

## How to read this

- **Evidence** is a command that shows the problem (run against the current
  binary; `/tmp` files are the obvious tiny tables).
- **Solvable?** *Yes* (a rule or flag fixes it), *Partly* (fixable for the
  common case, a hard limit remains), *No* (inherent in what a table diff is).
- **Status** of the decision:
  - **Decided**: the answer is clear and cheap; it is, or will be, done.
  - **Proposed**: my recommendation; it changes the format or default
    behaviour, so it needs your OK.
  - **Leave**: a documented limitation; the cost of fixing exceeds the benefit.
  - **Defer**: depends on something later (typed formats, multi-sheet), with
    the phase that must settle it.

## Principles used to judge options

1. **Stay readable by the original daff.** A diff we write must still be a
   valid reference diff. Extensions may only use channels the reference
   ignores. Checked: the reference `patch` skips rows whose action cell looks
   like `@key@` ("metadata rows") and ignores unknown extra columns, so those
   are the extension channel. Anything else is a fork of the format.
2. **A default diff must be applicable by itself.** `patch(a, diff(a, b))`
   equals `b` (property-tested). Options that drop information
   (`--context 0`, `--act`, `--ignore`, `-i`, `-w`, `-d`) make *partial* diffs;
   they should say so and `patch` should cope or complain.
3. **Never silently wrong.** When the information is insufficient, report a
   problem (as `patch` now does) instead of guessing quietly.
4. **Text first.** A diff is a table that survives CSV. Meaning is defined on
   cell text; types and properties are optional extras.
5. **Heuristics belong to generating a diff, not to applying it.** Matching
   rows or columns with heuristics is unavoidable when comparing; applying a
   diff should rely on what the diff states.

## Summary

| ID | Area | Gap | Solvable? | Decision | Status | Phase |
|----|------|-----|-----------|----------|--------|-------|
| G-01 | Rows | No key given: matching is heuristic and not recorded | Partly | Record the key in a meta row; warn on weak matches | Proposed | 3 |
| G-02 | Rows | Identical duplicate rows cannot be told apart | Partly | Auto-add index columns for ambiguous diffs; patch uses them | Proposed | 3 |
| G-03 | Rows | Blank keys cannot be matched by content | Partly | Same as G-02; warn at diff time | Proposed | 3 |
| G-04 | Rows | Editing a key looks like delete + insert (or an update, by luck) | No | Document; optional fuzzy matching later | Leave | 3 |
| G-05 | Rows | Inserts and moves without context have no position | Yes | Anchor rule or index columns; patch reports `Unplaceable` | Proposed | 3 |
| G-06 | Rows | Ordered vs unordered mode is not recorded; moves are not minimal-unique | Partly | Record the mode in a meta row | Proposed | 3 |
| G-07 | Columns | Header row is guessed; headerless tables; several header rows | Partly | `--no-header`, `--header-row N` | Proposed | 3 |
| G-08 | Columns | Duplicate, blank or tag-like column names (`...`) | Yes | Escape tag-like names; warn on duplicates | Proposed | 3 |
| G-09 | Columns | Rename detection is heuristic | Partly | Keep; add `--rename old:new` | Proposed | 3 |
| G-10 | Values | Types are invisible (`1` vs `"1"`, dates, formulas) | Partly | Defer to the typed formats; design a `--typed` mode | Defer | 4 |
| G-11 | Values | Text equivalence: Unicode forms, line endings, numbers | Yes | Opt-in normalisation flags; default stays exact | Proposed | 3 |
| G-12 | Values | NULL vs blank vs absent | Yes | Table of semantics; as implemented | Decided | 2 |
| G-13 | Values | Long text cells are replaced whole | Partly | Leave; maybe `--text-diff` later | Leave | 3+ |
| G-14 | Format | In-band markers vs data | Yes | Escape rule for the one real collision (G-08) | Decided | 3 |
| G-15 | Format | Extra rows/columns before `@@` | Yes | Locate `@@`; skip the rest | Decided | 2 |
| G-16 | Format | Metadata rows are unspecified | Yes | Specify `@key@` rows as the extension channel | Proposed | 3 |
| G-17 | Format | Multi-table diffs, added/removed tables | Partly | Specify sections; table-level tags | Proposed | 3 / 4 |
| G-18 | Format | Diff does not record how it was made | Yes | `@options@` meta row | Proposed | 3 |
| G-19 | Semantics | Applying to a different table; reversing a diff | Yes | Preconditions done (B-24); add `daff invert` | Decided / Proposed | 2 / 3 |
| G-20 | Semantics | Composing or rebasing diffs | No | Leave; apply in sequence | Leave | – |
| G-21 | Semantics | Conflicts: representation, location, resolution | Partly | Report file + keep in-cell markers | Proposed | 3 |
| G-22 | Semantics | Merge ignores moves, renames are weak | Partly | Document; ours-order wins | Leave | 3 |
| G-23 | Scale | A sort makes every row a move | No | Document; use unordered | Leave | – |
| G-24 | Input | Encodings, delimiters, dialects | Yes | `--encoding`, `--delimiter`, sniffing | Proposed | 3 |

Questions that need your decision are collected at the end.

---

## Rows

### G-01 Matching rows without a key
**Spec**: says nothing about how rows of the two tables correspond. The
reference tries combinations of up to five columns until rows are unique
(`--id` overrides).
**Evidence**: `daff diff a.csv b.csv` and `daff diff --id id a.csv b.csv` give
different diffs when the key itself changes (see G-04); the diff does not say
which columns were used.
**Problem**: the diff is only meaningful relative to a matching nobody
recorded. `patch` re-derives its own matching by content, which can differ.
When the heuristic matches on a weak key (a column with few distinct values)
the diff can be wrong without saying so.
**Options**:
1. Leave as is.
2. Record the identity columns in a meta row, e.g. `@key@,id,,` (columns that
   identify rows in this diff). `patch` and other consumers use it; the
   reference ignores it.
3. At diff time, warn when the best key found is weak (some rows matched only
   by a non-unique key).
4. Require `--id` for large or important tables (policy, not code).
**Decision** (Proposed): 2 and 3. Cheap, backwards compatible, and it makes
the key part of the diff.

### G-02 Identical duplicate rows
**Spec**: silent; "a row common to both tables appears at most once" is
unsatisfiable for duplicates.
**Evidence**: `printf 'id,v\n15,red\n13,mauve\n15,red\n15,red\n' > a.csv`,
`printf 'id,v\n15,red\n13,mauve\n15,red\n16,green\n15,red\n' > b.csv`;
`daff diff --index a.csv b.csv` knows the new row is `-:4` but `patch` puts
`16,green` second. Without `--index` the diff cannot say which of the three
identical rows the new one follows.
**Problem**: two rows with equal content have no identity apart from their
position. Position information is in the diff only when context rows (which are
themselves duplicates) or index columns say so.
**Options**:
1. Leave, and warn when diffing a table with duplicate rows.
2. Use the index columns (`@:@`, row numbers `before:after`) that `--index`
   already produces: `patch` uses them as position hints when present.
3. Emit index columns automatically when the diff involves duplicates or
   blank keys (opt out with `--no-index`).
4. Use gap sizes: make `...` rows carry the number of omitted rows (a
   format change; see G-16 for a backwards-compatible way).
**Decision** (Proposed): 2 + 3 + 1: automatic index columns exactly when the
diff would be ambiguous, `patch` honouring them, and a warning otherwise.
*Hard limit*: a diff without index columns of a table with duplicates can
still be misapplied; that is documented, and `patch` reports it when it can
(see G-05).

### G-03 Blank or all-equal keys
**Spec**: "blank cells are ambiguous" (so blank keys match anything).
**Evidence**: single-column tables with blank rows fail the round-trip
property (known limit from phase 2).
**Options**: as G-02 (index columns identify them). **Decision** (Proposed):
covered by G-02's rule; the warning names the ambiguity ("N rows have a blank
key").

### G-04 Editing the key
**Spec**: silent. A changed key is, by definition, a different row.
**Evidence**: `id,v / 1,a / 2,b` to `id,v / 1,a / 20,b`: with `--id id` the
diff is `+++ 20` and `--- 2`; without it the heuristic matches on `v` and shows
`2->20`.
**Solvable?** No: whether "2 became 20" or "2 was deleted and 20 added" is a
judgement about the data.
**Options**: 1. Leave. 2. An optional similarity matcher (`--fuzzy`) that pairs
unmatched deleted and inserted rows by how many cells agree.
**Decision** (Leave) for the default; the fuzzy matcher is a possible phase 3
feature, off by default because it can invent relationships.

### G-05 Position of inserts and moves without context
**Spec**: "we can add context rows around insertions so we know where to put
them"; does not say what happens when there are none.
**Evidence**: `daff diff --context 0 c5.csv c5x.csv` (an insert after row 2)
gives just `+++,X,new`; `patch` appends it at the end, with no complaint.
**Options**:
1. Leave.
2. `patch` reports an `Unplaceable` problem when an insert or move has no
   anchor and no index information, and places it at the end.
3. `diff --context 0` in ordered mode keeps one anchor row next to each insert
   and move (so "0" means "no *extra* context").
4. `diff --context 0` in ordered mode adds index columns (G-02).
**Decision** (Proposed): 2 always; 3 or 4 for you to choose — 4 is the less
surprising for "give me the minimal diff" users because the visible rows stay
the same.

### G-06 Ordered vs unordered, and moves
**Spec**: row order matters "if meaningful"; `:` marks moved rows; does not
say how a diff states which mode it was made in, nor which rows count as moved.
**Evidence**: `daff diff --unordered c5.csv c5x.csv` then `patch` appends the
insert at the end (order information was discarded on purpose). Which rows are
tagged `:` is a minimisation (fewest rows moved) that is not unique.
**Options**: 1. Leave. 2. Record the mode in a meta row (`@order@,ordered`).
**Decision** (Proposed): 2. A consumer then knows whether an insert's
position is meaningful. The choice of which rows are "moved" stays unspecified
(any consistent minimal choice is valid; documented).

## Columns

### G-07 Which row is the header
**Spec**: the diff has an `@@` row; says nothing about how the compared tables'
header is found. The reference guesses among the first five rows.
**Evidence**: `daff diff` of two headerless CSVs treats the first data row as
the header (`@@,1,a`). Duplicate names used to confuse the guess (B-18, fixed
for the common case).
**Options**: 1. Leave. 2. `--no-header` (synthetic names `A`, `B`, ... as in
spreadsheets, matching the spec's column-letter convention in `--index`).
3. `--header-row N` for tables with title rows.
**Decision** (Proposed): 2 and 3. They are input options only; the diff format
does not change. Multi-row headers: Leave (use `--header-row`).

### G-08 Duplicate, blank and tag-like column names
**Spec**: silent; the header row of a diff holds column names as data.
**Evidence**: a column named `...`: `daff diff` shows the right change but
`patch` skips the column (it reads `...` as "columns omitted") and silently
leaves the cell unchanged — the one real collision between a data value and a
marker in the header row. Duplicate names work only through special-casing.
**Options**: 1. Leave and warn. 2. Escape header cells that are `...`, using
the same scheme as `NULL` (`...` becomes `_...`, `_...` becomes `__...`); the
reference reads `_...` as a plain name, which is harmless.
3. Make duplicate names unique with a suffix when diffing.
**Decision** (Proposed): 2 for `...`; for duplicates keep order-based matching
(done) and warn. Blank names: matched by position (current), documented.

### G-09 Detecting renames
**Spec**: `(old)` in the schema row marks a rename; not how one is detected.
The reference links columns by name, and when names do not match, by equal
cell values in the best-matching row.
**Evidence**: rename `v` to `val` is found when a data row is unchanged; it
shows up as delete + insert when every row changed.
**Solvable?** Partly: no rule is right for all data.
**Options**: 1. Leave. 2. `--rename old:new` (and in `patch`, nothing needed).
**Decision** (Proposed): 2. Heuristic stays for the default.

## Values

### G-10 Types
**Spec**: "we lose information about the type of the cell value"; only NULL vs
empty is handled.
**Evidence**: two JSON files differing only in `1` vs `"1"` produce an empty
diff; `1.0` vs `1`, `01` vs `1`, `TRUE` vs `true` are text differences in CSV
unless `-d` is used (numbers only).
**Problem**: harmless for CSV, important for typed sources (JSON now; xlsx,
SQLite later), where `1` and `"1"` are different values.
**Options**: 1. Leave: text equality everywhere. 2. A `--typed` mode where
equal text with different types is a change, shown with a type note; the type
travels in a meta row (`@type@`) or as a cell suffix. 3. Typed comparison by
default for typed sources only.
**Decision** (Defer to phase 4): the design belongs with xlsx, whose cell
types (number, text, date, boolean, formula, error) force the question. Until
then JSON type changes stay invisible; documented here and in QUIRKS.

### G-11 Text equivalence
**Spec**: silent. Equality is on exact text.
**Evidence**: `é` as one code point vs `e` + combining accent shows as
`é->é` (identical on screen); a cell with CR+LF vs LF inside quotes shows as a
change (files that went through git `autocrlf`); `-i`/`-w` only cover case and
leading/trailing space; `1.0` vs `1` needs `-d`.
**Options**: 1. Leave. 2. Opt-in flags: `--unicode-normalize` (NFC),
`--normalize-eol` (CR+LF and CR to LF inside cells), possibly
`--ignore-number-format` (compare numeric text as numbers).
**Decision** (Proposed): 2, all off by default so a diff stays exact. When a
changed cell looks identical, the diff could show it with escapes; Leave.

### G-12 NULL, blank, absent
**Spec**: defines the NULL encoding; says blank cells in a diff are ambiguous.
**Decision** (Decided; implemented in phase 2). The meaning of each case:

| Situation | CSV | JSON | In a diff cell |
|-----------|-----|------|----------------|
| null value | `NULL` | `null` or missing key | `NULL` |
| empty string | empty | `""` | blank |
| the string `NULL` | `_NULL` | `"NULL"` | `_NULL` |
| column not in this table | – | – | blank |
| blank cell in a diff used as a patch | read as `""` (matches null or `""`) | | |

Not solved: a CSV with the literal word `NULL` meaning "text NULL" cannot be
told from null; use `_NULL`.

### G-13 Long text
**Spec**: a modified cell is `old->new` in full.
**Evidence**: a one-word change in a multi-paragraph cell is a full replace.
**Solvable?** Partly (an intra-cell text diff exists but needs a format).
**Decision** (Leave). A `--text-diff` option could render a word-level diff in
the cell for humans, not for patching; not planned.

## Format

### G-14 Markers versus data
Action column and header names are positionally distinct from data, so the only
real collision is the header cell `...` (G-08). Cell values equal to `NULL`
are handled by the NULL encoding; values containing `->` by the per-row
padding. **Decision** (Decided): nothing more.

### G-15 Extra rows and columns before `@@`
**Spec**: readers "should be robust to a few extra rows or columns appearing
before the `@@`".
**Evidence** (before phase 2's fix): the reference destroyed the table with a
leading extra column and did nothing with a title row. Now the action column
is located by finding `@@`; everything above and left of it is ignored; a diff
with no `@@` is reported. **Decision** (Decided; B-29, B-30).

### G-16 Metadata rows as the extension channel
**Spec**: not mentioned. The reference code recognises action cells of the
form `@name@` and `@name@->` and skips them with a warning.
**Evidence**: `@@,id,v` / `@mode@,ordered,` / ... is applied normally by the
reference and by us.
**Options**: 1. No extensions. 2. Specify `@key@` rows: first cell `@key@`,
remaining cells hold values by column (for per-column facts such as types) or
in the first data cell (for diff-wide facts such as the key or the order mode).
**Decision** (Proposed): 2, written up as a short `docs/FORMAT.md`. Every
Proposed item above that needs to travel in the diff (G-01, G-06, G-10, G-18,
G-21) uses this one mechanism.

### G-17 Multi-table diffs
**Spec**: one table only.
**Evidence**: the multi-table JSON form writes sections (`name`, a line of `=`,
the diff). The reference cannot diff a table that exists on one side only (it
crashes); we report an error. There is no CSV-safe way to say "table added".
**Solvable?** Partly.
**Options**: 1. Leave: only same-named tables compared, others an error.
2. Define table-level actions: the base `name`/`table` diff already is a
diff; make `+++`/`---` rows there mean table added/removed, with the table's
rows shown as all inserted/deleted.
3. As 2, plus a single-file CSV form with a `@table@` meta row before each
section.
**Decision** (Proposed): 2 now for JSON; the CSV form (3) is settled with
xlsx in phase 4 because sheets are the main user. Added/removed tables become
a diff row, not an error.

### G-18 The diff does not record how it was made
**Spec**: silent.
**Evidence**: a diff made with `--act update`, `--ignore b`, `-i` or
`--context 0` looks like any other; `patch` cannot tell it is partial.
**Options**: 1. Leave. 2. An `@options@` meta row listing the options that
change meaning (`act`, `ignore`, `case`, `whitespace`, `epsilon`, `context`).
**Decision** (Proposed): 2. `patch` uses it to word its problems ("this diff
ignores column b") and `invert` (G-19) refuses diffs it cannot invert.

## Semantics

### G-19 Applying to another table; reversing
**Spec**: silent on applying a diff to a table other than the original.
**Decision** (Decided; B-24, phase 2): preconditions are checked, a diff
applied twice is a no-op, problems are reported (`docs/SPEC-ERRATA.md` #11).
**Reversing** a diff (swap `+++`/`---`, `a->b` to `b->a`, inverse schema) is
possible because every deleted row and old value is in the diff; moves can be
inverted only with index columns, and partial diffs (G-18) cannot be inverted.
**Decision** (Proposed): add `daff invert diff.csv`, and `patch --reverse` as
sugar; refuse when information is missing.

### G-20 Composition and rebasing
`diff(a,b)` then `diff(b,c)` is not `diff(a,c)`, and rebasing a diff onto
another version is a three-way merge problem. **Decision** (Leave): apply diffs
in sequence; use `merge` to combine two versions of a table.

### G-21 Conflicts
**Spec**: silent on three-way diffs and merges; the code has `!->` cells in
diffs and `((( p ))) l /// r` in merged cells.
**Evidence**: a merge where one side edits a row the other deleted prints
`1 conflict` on stderr and keeps the row, but nothing in the output says which
row; the cell markers cannot be parsed back reliably (cell text may contain
the separators).
**Options**:
1. Leave.
2. Write a conflict report: `merge --conflicts FILE` (JSON: kind, row key,
   column, parent/local/remote values); the output table is unchanged.
3. Additionally mark kept-but-conflicting rows in the merged table through a
   meta row or a sidecar, so a viewer can highlight them.
4. Provide `resolve` (choose local/remote/parent per conflict).
**Decision** (Proposed): 2 and keep in-cell markers; 3 and 4 are Leave for now
(they need a UI to be worth anything).

### G-22 Merge limits
The merge ignores row and column moves (the reference says so in a comment),
detects renames only when the data identifies them, and keeps the local order.
**Decision** (Leave): documented; local order wins.

## Scale and input

### G-23 Sorting
Sorting a table makes every row a move (`:` on all but one row), which is
correct and unreadable. **Decision** (Leave): use `--unordered`; a future
`@sorted@ column` meta row is possible but unplanned.

### G-24 Encodings, delimiters, dialects
**Spec**: out of scope, but a tool needs it. Today: UTF-8 only (refused
otherwise, B-07), delimiter chosen by file extension (`.csv` comma, `.tsv` tab,
`.ssv` semicolon), no sniffing, no BOM-less UTF-16.
**Evidence**: a semicolon-separated `.csv` (common from European Excel) is read
as one column; a UTF-16 file (Excel "Unicode text") is refused.
**Options**: 1. Leave. 2. `--encoding NAME` (utf-8, utf-16, latin-1,
windows-1252) and `--delimiter C` flags; optional sniffing for `.csv`.
**Decision** (Proposed): 2 without sniffing by default (explicit is safer);
output in the same encoding as input.

---

## Suggested order for phase 3

1. **Format channel and spec text** (G-16, then G-01, G-06, G-18): a short
   `docs/FORMAT.md` with the meta rows, plus the generator and `patch` support.
   Low risk, enables the rest.
2. **Ambiguity handling** (G-02, G-03, G-05): auto-index, index hints in
   `patch`, `Unplaceable` reports, diff-time warnings.
3. **Input flexibility** (G-07, G-24, G-11): `--no-header`, `--header-row`,
   `--encoding`, `--delimiter`, normalisation flags.
4. **Operations** (G-19 invert, G-21 conflict report, G-09 `--rename`,
   G-08 escaping).
5. **Table-level diffs** (G-17 for JSON), leaving the CSV/sheet form for phase 4
   together with G-10.

## Decisions needed from you

1. **Extension channel** (G-16): are `@key@` meta rows the right way to extend
   the format, given the reference ignores them? (Alternative: no extensions.)
2. **Auto-index** (G-02, G-05): may `diff` add `--index` columns on its own when
   it detects ambiguity (default on, opt out), or should that stay opt-in with
   only a warning?
3. **`--context 0`** (G-05): keep one anchor row (option 3) or add index
   columns (option 4)?
4. **Typed comparison** (G-10): settle now for JSON, or only with xlsx in
   phase 4?
5. **Fuzzy matching** (G-04, G-09): worth building as opt-in flags, or leave
   matching entirely to `--id` and the existing heuristics?
6. **Conflict output** (G-21): is a JSON conflict report enough, or do you want
   in-table marking too?
7. **Scope of input flags** (G-24): add encoding and delimiter options in phase
   3, or leave them to phase 4 (where Excel brings its own encodings)?
