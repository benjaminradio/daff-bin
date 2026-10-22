# daff (Rust)

A Rust implementation of [daff](https://github.com/paulfitz/daff), the tabular
diff, patch and merge tool, as a single static binary and a library.

Phase 1 (this phase) reproduces the reference implementation (daff 1.4.2)
exactly, quirks included; see [docs/QUIRKS.md](docs/QUIRKS.md). Phase 2 adds
Excel (xlsx) support.

```
cargo build --release          # target/release/daff
daff a.csv b.csv               # diff
daff parent.csv a.csv b.csv    # diff, with a common ancestor
daff patch a.csv diff.csv      # apply a diff
daff merge parent.csv a.csv b.csv
daff render diff.csv > diff.html
daff diff --output-format html a.csv b.csv
daff diff a.json b.json        # JSON tables, nested-cell diffs
daff git csv                   # use daff as git's diff/merge driver for *.csv
```

## Status

| Area | State |
|------|-------|
| CSV/TSV/SSV/PSV diff, patch, merge, trim, copy | done |
| HTML (`render`, `--output-format html`) and ANSI colour output | done |
| JSON (single and multi-table), NDJSON | done |
| `daff git`, `--git` diff driver | done |
| SQLite sources (`.sqlite`) | not yet |
| `--www` (open diff in a browser) | not yet |
| xlsx | phase 2 |

## Layout

- `crates/daff` — the library. The API follows the reference: `Table`,
  `compare_tables`/`compare_tables3` → `CompareTable::align` → `TableDiff`,
  `HighlightPatch`, `Merger`, `DiffRender`, plus `format::{csv,json}`.
- `crates/daff-cli` — the `daff` binary.
- `docs/QUIRKS.md` — reference behaviour reproduced on purpose, and the
  deliberate deviations.
- `scripts/` — differential tests against the reference.

## Testing

`cargo test` runs unit and golden tests. `scripts/conformance.sh` installs the
reference from npm (needs node and python3) and compares this implementation
with it on random tables for every command and output format.
