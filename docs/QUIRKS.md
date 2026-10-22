# Reference quirks

Phase 1 of this project reproduces the reference implementation (daff 1.4.2,
the Haxe source at <https://github.com/paulfitz/daff>) exactly, including
behaviour that is arguably a bug. This file records those quirks so that:

- nobody "fixes" one by accident and breaks conformance, and
- a later phase can improve on them deliberately.

Every quirk below is reproduced on purpose and covered by
`scripts/fuzz_oracle.py` (differential testing against the reference) or by a
golden test. Each entry says where it lives in the Rust port. Entries marked
**improvement candidate** are the ones worth reconsidering after conformance is
complete; if you change one, gate it behind an explicit option so that the
default stays byte-compatible.

## Data model and CSV

1. **Null is not empty.** A cell is `None` (null) or a string. Cells that were
   never set are null. The CSV reader turns the unquoted text `NULL` into
   null; `_NULL` reads back as the string `NULL`, `__NULL` as `_NULL`, and so
   on. A quoted `"NULL"` stays the string `NULL`.
   (`format/csv.rs`)
2. **Nulls are written as `NULL`.** The CSV writer renders null as the bare
   text `NULL`, and does *not* escape a string cell whose text is `NULL` (the
   diff builder does that for update cells with a leading `_`, the writer
   does not). Round-tripping `NULL` strings through plain CSV therefore turns
   them into nulls. **Improvement candidate.** (`Csv::render_cell`)
3. **Rows longer than the first row are corrupted, not rejected.** The first
   row sets the table width. A cell past that width on a later row is written
   through the flat index, so it lands in the next row's first cells (or
   vanishes past the end on the last row), and the reference traces
   `Ignored overflowing row N with cell 'c' after: …`. Short rows leave
   nulls. **Improvement candidate.** (`Csv::parse_full`, `Table::set`)
4. **Flat-index tables.** Tables store cells at `x + y*width`; `resize` changes
   dimensions without moving data. Writes past the end are kept but hidden
   (`insert_or_delete_*` may write out of range) and reappear if the table
   grows. (`table.rs`)
5. **Empty tables print one line.** The reference row stream always yields a
   first row, so a table with height 0 is written as a single line
   (`\n` for width 0; `NULL,NULL,…` for width > 0). (`Csv::render_table`)
6. **Default line ending is CRLF** unless the first input file has a line
   ending, in which case the first line ending discovered in the *first
   loaded file* (parent, if given) is used for all output.
   (`Csv::parse`, CLI `Io`)
7. **Output delimiter follows the last file named**, not the output name
   alone: each loaded file name updates the delimiter by extension, and an
   output name without a known extension (including `-`) inherits it.
   (CLI `Io::check_format`)
8. **`trim_blank` after load.** Trailing blank rows and (judged on the top
   three rows only) trailing blank columns are dropped from every input.
   (`Table::trim_blank`)

## Alignment and diff

9. **Sorting depends on the sort algorithm.** Several reference comparators
   (`Alignment.toOrder3`, `HighlightPatch.sortMods`) are not consistent
   orderings. The JavaScript build sorts with V8's TimSort, and the order it
   yields for such comparators is what the reference tests encode. The port
   therefore implements V8's algorithm (binary insertion sort for short
   arrays, run merging for long ones) rather than `slice::sort_by`.
   Galloping is omitted, so inconsistent comparators on 64+ element arrays
   may differ. (`unit.rs: stable_sort_by`)
10. **Ignored columns are not honoured when matching rows.** `--ignore` is
    tested against the row alignment's header index, which is still unset
    (-1) at that point, so ignored columns still take part in row matching;
    they are only dropped afterwards when the diff is rendered.
    **Improvement candidate.** (`align.rs`, `CompareTable::align_core2`)
11. **Duplicate column names.** Header keys that repeat are marked `-1` and
    then linked as if they were additions (`align_columns`). Tables with
    duplicate header names align poorly.
12. **Header row is guessed.** The header is chosen among the first five rows
    (best match between the two tables, then most unique cells), so a data
    row can be mistaken for a header. (`align_columns`)
13. **More than five common columns** are indexed using only the five columns
    with the most distinct values; matching is heuristic, not exact.
    (`align_core2`)
14. **Context spreading** treats skipped inserts/deletes specially
    (`-3` marks) and `--all-rows` leaves the row activity array empty, so the
    context pass must be a no-op rather than an error. (`diff.rs`)
15. **Row/column index display** (`--index`) fills the corner with `@:@` and
    reads column numbers from `col_map[i-1]`, which is offset by one from the
    header written by `add_header`; the result is what the reference prints.
    (`TableDiff::add_rc_numbers`)
16. **Meta tables, nested tables and SQL sources** are not implemented in
    the port (no table type here has meta information), so those code paths
    are skipped. They only matter once JSON/SQLite inputs are added.

## Patch

17. **Payload column is always 1.** `payloadCol` is computed from `rcOffset`
    before `rcOffset` is known, so with an `@:@` index column the patch
    payload still starts at column 1 and the index column is treated as data.
    (`HighlightPatch::apply`)
18. **Rows are found by content, with a quirk in the tie-break.** Ambiguous
    matches use the previous patch row's match to pick `prev + 1`, but the
    reference's `break` there only leaves the inner loop, so later indexes
    can overwrite the result. (`HighlightPatch::look_up`)
19. **Columns missing from the patch never match.** Index keys for patch rows
    use the literal text `NOT_FOUND` for source columns that are elided from
    the patch (`...`), so any index that involves such a column cannot match.
    (`HighlightPatch::get_row_string`)
20. **Header maps iterate in insertion order**, which decides which patch
    column wins when several land on the same destination column.
    (`OrderedMap` in `patch.rs`)
21. **Metadata rows are skipped.** Rows whose action begins with `@` only
    trigger the column bookkeeping; no metadata is applied.

## Merge

22. **Merge ignores the caller's flags.** The alignment is built with no
    flags, so `--id`, `--ignore` and the like have no effect on `merge`.
    **Improvement candidate.** (`Merger::apply`)
23. **Conflicts are written into the cell** as `((( parent ))) local ///
    remote`, and the merge reports only a count. There is no way to resolve
    a conflict in the tool. (`Merger::apply`)
24. **Row/column moves are ignored** (the reference's own comment: "row/col
    movement -- ignore for now").

## Command line

25. **`--inplace` never works.** The reference returns status 1 as soon as the
    option is seen (the `return 1` is outside the "output also given" check),
    without writing anything. The port does the same.
    **Improvement candidate.** (`daff-cli`, `run`)
26. **Option parsing is positional-agnostic.** Options are pulled out of the
    argument list wherever they appear; `diff` is implied by file names with
    a dot (two files) or `--`, `copy` by one file.
27. **`--ordered`/`--unordered` and `--context 0`.** `--unordered` also sets
    the context to 0 unless later options change it.
28. **Merge status.** Exit status is 1 when there are conflicts (2 with
    `--fail-if-diff`), after printing `N conflict(s)` on stderr and still
    writing the merged table.

## Deliberate deviations

These are the only places where the port intentionally differs from the
reference output.

1. **Warnings go to stderr.** The reference writes `trace(...)` output (for
   example the overflowing-row message above) to *stdout*, in front of the
   table, which corrupts piped output. The port prints the same text on
   stderr. (`daff-cli`, `Io::load`)
