# daff (Rust)

A Rust implementation of [daff](https://github.com/paulfitz/daff), the tabular
diff, patch and merge tool, as a single static binary and a library.

Phase 1 reproduced the reference implementation (daff 1.4.2) exactly, quirks
included (tag `v0.1.0`; see [docs/QUIRKS.md](docs/QUIRKS.md)). Phase 2 fixes
the bugs found by comparing it with the spec and with its own intent
([docs/BUGS.md](docs/BUGS.md), [docs/SPEC-ERRATA.md](docs/SPEC-ERRATA.md)); the
fixes are on by default and `--reference-compat` restores the 1.4.2 behaviour.
Phase 3 will expand and optimize daff, phase 4 adds Excel (xlsx) and phase 5
SQLite.

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
| SQLite sources (`.sqlite`) | phase 5 |
| `--www` (open diff in a browser) | not yet |
| xlsx | phase 4 |

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
with it (in `--reference-compat` mode) on random tables for every command and
output format. `scripts/prop_roundtrip.py` checks the round-trip properties
(`patch(a, diff(a, b)) == b`, `merge(a, a, b) == b`) and
`scripts/fuzz_robust.py` looks for crashes.
