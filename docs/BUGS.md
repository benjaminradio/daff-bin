# Bug catalogue (phase 2)

Phase 1 matched daff 1.4.2 exactly (tag `v0.1.0`). Phase 2 compares the
implementation with two other things and fixes the differences:

- **the spec**: the tabular diff specification v0.8
  (<http://paulfitz.github.io/daff-doc/spec.html>, source `spec.md` in
  <https://github.com/paulfitz/daff-doc>), and
- **intended behaviour**: what the documentation, the option help and the
  structure of the code show the tool is meant to do. The main yardstick is
  round trips: `patch(a, diff(a, b))` must equal `b`, `merge(a, a, b)` must
  equal `b`, and reading then writing a file must not change its meaning.

Fixes are on by default. `--reference-compat` (or
`DAFF_REFERENCE_COMPAT=1`) restores the original behaviour of every fixed
bug; `scripts/conformance.sh` runs with it, so the differential tests against
the reference keep working. Categories: **DATA** (data loss or corruption),
**SPEC** (differs from the written spec), **INTENT** (differs from documented
or evident intent), **ROBUST** (crash), **DOC** (the documentation is
wrong; fix the text, not the code).

The properties are checked by `scripts/prop_roundtrip.py` and
`scripts/fuzz_robust.py`; every entry has a repro that fails before its fix.

| ID | Cat | Summary | Status |
|----|-----|---------|--------|
| B-01 | DATA | NDJSON with ragged rows is scrambled | open |
| B-02 | DATA | `patch` corrupts tables with duplicate rows | open |
| B-03 | DATA | `patch` misplaces moved rows when other rows are deleted or omitted | open |
| B-04 | DATA | CSV rows longer than the first row lose cells | open |
| B-05 | DATA | `trim_blank` drops a column that has data below the top rows | open |
| B-06 | DATA/SPEC | `NULL`-like strings are not encoded (diff cells, CSV output) | open |
| B-07 | DATA | Invalid UTF-8 is silently replaced by U+FFFD | open |
| B-08 | DATA | `merge` silently drops edits to a row/column deleted on the other side | open |
| B-09 | DATA | `merge` duplicates a row added identically on both sides | open |
| B-10 | ROBUST | `patch` aborts (capacity overflow) when it deletes more than exists | open |
| B-11 | SPEC | Cells of columns absent from a table are `NULL`, spec says blank | open |
| B-12 | SPEC | Blank values of an added column are patched as null, not blank | open |
| B-13 | SPEC | Schema tags are combined (`:(old)`, `:+++`), spec says not to | open |
| B-14 | SPEC | One long separator (`-->`) is used on every row, spec says per row | open |
| B-15 | DOC | `__hilite_diff__` action column of unordered diffs does not exist | open |
| B-16 | INTENT | Cells with the text `null` or `undefined` are blank when matching rows | open |
| B-17 | INTENT | `--ignore-epsilon` treats `12abc` and `12xyz` as equal numbers | open |
| B-18 | INTENT | Duplicate header names make a data row look like the header | open |
| B-19 | INTENT | Single-table JSON ignores `columns` as header; own JSON output unreadable | open |
| B-20 | INTENT | A UTF-8 byte order mark ends up in the first column name | open |
| B-21 | INTENT | Unknown `--id`, `--act` and `--ignore` values are silently accepted | open |
| B-22 | INTENT | `merge` ignores `--id`/`--ignore` | open |
| B-23 | INTENT | `--inplace` never works | open |
| B-24 | INTENT | `patch` does not notice that a row is already present or has changed | open |
| B-25 | INTENT | Terminal columns are aligned by UTF-16 length, not display width | open |
| B-26 | INTENT | Table names are not escaped in HTML headings | open |

## Details

### B-01 NDJSON ragged rows are scrambled
`{"a":1,"b":2}` / `{"a":3}` / `{"a":5,"c":6}` read as
`a,c,b / 1,3,2 / 5,6,NULL / NULL,NULL,NULL`. Rows are added last to first and
the table width grows while cells are already stored (flat-index storage), so
earlier cells move. Columns are also numbered in the order the *last* lines
mention them. Expected: union of keys in first-seen order, every value in its
own column.

### B-02 patch with duplicate rows
`id,v / 1,a / 1,a / 2,b` patched with the diff to `id,v / 1,a / 2,b` yields
three rows of `NULL,NULL`. Rows are located by content; with duplicates the
lookup is ambiguous and the later bookkeeping goes wrong.

### B-03 patch misplaces moved rows
`1..5` to `1,5,2,3` (4 deleted, 5 moved): the diff is correct but patching
gives `1,3,5,2`.

### B-04 / B-05 CSV reading
A data row longer than the first row loses the extra cells (reported on
stderr, written through the flat index so they can overwrite the next row).
`trim_blank` removes trailing columns that are blank in the *first three
rows* only: `a,b, / 1,2, / 3,4, / 5,6,7` loses the `7`. A short row (or a
blank line) is padded with null, so it is written back as `,NULL`.

### B-06 NULL encoding
The spec encodes null as `NULL`, the string `NULL` as `_NULL`, `_NULL` as
`__NULL` and so on, so a diff survives CSV. Only modified cells are encoded.
A string `_NULL` in an inserted row is written raw and read back as `NULL`;
`trim`/`copy` of a file containing `__NULL` writes `_NULL`. Expected: every
written string that matches `_*NULL` gets one more `_`, null is written
`NULL`.

### B-07 invalid UTF-8
`caf\xe9` is read as `caf�` and written back that way. Expected: refuse
the file with a clear message instead of changing data silently.

### B-08 / B-09 merge
Local edits a row, remote deletes it (or the reverse; same for columns): the
edit disappears and the merge reports no conflict. Both sides adding the
same new row gives the row twice. Expected: edit-versus-delete is a
conflict (the edited version is kept, the conflict counted) and identical
additions are merged into one row.

### B-10 patch abort
A patch that removes rows from an empty table computes a negative height
that becomes a huge allocation. Found by `scripts/fuzz_robust.py`.

### B-11 / B-12 blanks (spec)
Spec: inserted rows "leave deleted columns blank". The diff writes `NULL`
(`+++,9,q,NULL`). Spec: when a diff is applied a blank cell means "insert the
blank string". Cells of an added column that are blank in the diff are left
null (`id,new / 1,` becomes `1,NULL`).

### B-13 / B-14 tags (spec)
Spec: "tags are not combined" and a moved column is marked `:`; the
implementation writes `:(old)`, `:+++`, `:---`. Spec: the `-` padding of
`->` is chosen per row to avoid collision *on that row*; the implementation
picks one separator for the whole diff.

### B-15 spec text
An unordered diff has the action column first and headed `@@` like any other;
there is no `__hilite_diff__` column. The spec text is out of date.

### B-16 'null' / 'undefined' keys
Row matching drops key cells whose text is `null` or `undefined` (a leftover
of JavaScript string conversion), so `null,x` / `null,y` can never be matched
and shows as delete plus insert.

### B-17 epsilon
`-d 0.01` compares with JavaScript `parseFloat`, which reads a numeric
prefix. The documentation says "cells that look like floating point numbers".

### B-18 duplicate headers
`a,a,b / 1,2,3` is diffed with the data row as the header, because the header
is the row with the most unique cells among the first five.

### B-19 JSON
`{"t": {"columns": [...], "rows": [...]}}` is read without a header row (the
multi-table reader does add it), so the first data row becomes the header.
`daff copy x.csv out.json` writes `{"sheet": [[...]]}` which `daff` cannot
read ("JSON failed").

### B-20 BOM
A leading U+FEFF becomes part of the first column name, so a BOM file and
the same file without one differ in their first column.

### B-21 options
`--id nope`, `--act bogus` and `--ignore nope` are accepted and do nothing
useful (or show nothing).

### B-22 / B-23 CLI
`merge` builds its alignment without the flags, so `--id` is ignored.
`--inplace` returns status 1 without doing anything.

### B-24 patch preconditions
Applying a diff to a table that already has the change (or has a different
value in the changed cell) silently applies it again.

### B-25 / B-26 cosmetics
Terminal padding counts UTF-16 units (wide characters misalign). Table names
in multi-table HTML are inserted into `<h3>` without escaping.
