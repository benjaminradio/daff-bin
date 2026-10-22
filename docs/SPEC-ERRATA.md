# Notes on the tabular diff specification (v0.8)

Where the specification (`spec.md` in paulfitz/daff-doc, v0.8, 27 May 2014)
is out of date, silent or ambiguous, this is what the implementation does.
Each item is checked by a test.

1. **The action column is always first and always called `@@` in the header
   row.** The spec says an unordered diff names its action column
   `__hilite_diff__`; no version of daff does that. Unordered diffs differ
   only in omitting `:` tags (and, for JSON, in using nested cells).
2. **Moved and renamed/inserted columns combine tags.** A schema cell is
   `:` for a moved column, `(old)` for a renamed one, `+++` for an inserted
   one, `---` for a deleted one. A column that is both moved and renamed is
   `:(old)`; moved and inserted is `:+++`. The spec's rule that tags are not
   combined applies to the weak tags of context rows, where a moved row that
   also gained a cell is just `:`.
3. **The padding of `->` is per row.** A modified-cell row uses the shortest
   `-…->` that does not occur in any cell of that row's source rows; other
   rows keep `->`. Parsers must split on the row's own action tag.
4. **NULL encoding applies to every cell of the diff**, not only modified
   cells: null is `NULL`, the string `NULL` is `_NULL`, `_NULL` is `__NULL`.
   The same encoding is used when a table is written as CSV.
5. **Columns missing from one table are blank in rows of the other** (a
   deleted column in an inserted row, an inserted column in a deleted row).
6. **Blank cells of an inserted column** become the empty string when a diff
   is applied, whether the row is shown as `+` or only as context.
7. **Row/column numbers (`--index`).** The corner cell is `@:@`, the first
   column holds row numbers as `before:after` (`-` for absent) and the first
   row column letters (`A:A`, `B:C`, …). They are informational; a patch
   ignores them.
8. **Conflicts** in a three-way diff use `!` in the separator, as in
   `a!->b!->c` (parent, local, remote), with the action tag `!->`. In a
   merge, conflicting cells are written `((( parent ))) local /// remote`;
   a row or column deleted on one side and changed on the other is kept and
   counted as a conflict.
9. **Nested diffs (JSON).** With nested cells, a modified cell is
   `{"before": x, "after": y}` and a conflict is
   `{"before", "ours", "theirs"}`. The spec says there is no specification
   for this; this is the de facto format.
10. **Order of rows in a diff.** Rows appear in the order of the remote table;
    deleted rows are interleaved by their position in the local table. Because
    that interleaving is only a hint once rows move, a patch does not use the
    position of a deleted row to place other rows.
11. **Applying a diff checks what it expects.** The spec does not say what a
    patch does when the table differs from the one the diff was made from.
    The rule used: a modified cell is changed only if it still has the old
    value (it is left alone if it already has the new value, and reported
    otherwise); a deleted row is removed only if it still looks like the row
    shown; an inserted row that is already there is not added twice. Blank and
    NULL are treated as equal for these checks, as the spec says blank cells
    are ambiguous.
