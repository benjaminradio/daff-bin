//! Rust port of [daff](https://github.com/paulfitz/daff): tabular diff, patch, merge.
//!
//! The public API mirrors the reference library's shape:
//!
//! | daff (JS)                        | this crate                      |
//! |----------------------------------|---------------------------------|
//! | `new daff.SimpleTable(w, h)`     | [`Table::new`]                  |
//! | `daff.compareTables(a, b)`       | [`compare_tables`]              |
//! | `daff.compareTables3(p, a, b)`   | [`compare_tables3`]             |
//! | `.align()`                       | [`CompareTable::align`]         |
//! | `new daff.TableDiff(al, flags)`  | [`TableDiff::new`]              |
//! | `.hilite(output)`                | [`TableDiff::hilite`]           |
//! | `new daff.HighlightPatch(t, d)`  | [`HighlightPatch::new`]         |
//! | `new daff.Merger(p, a, b, f)`    | [`Merger::new`]                 |

pub mod align;
pub mod cell_info;
pub mod compat;
pub mod diff;
pub mod error;
pub mod flags;
pub mod format;
pub mod merge;
pub mod mover;
pub mod patch;
pub mod render;
pub mod table;
pub mod tables;
pub mod unit;
pub mod value;

pub use align::{compare_tables, compare_tables3, Alignment, CompareTable, IndexPair, Row};
pub use cell_info::{examine_cell, CellInfo};
pub use compat::{reference_compat, set_reference_compat};
pub use diff::{DiffSummary, TableDiff};
pub use error::{Error, Result};
pub use flags::Flags;
pub use merge::{ConflictInfo, ConflictKind, Merger};
pub use patch::{HighlightPatch, PatchIssue, PatchIssueKind};
pub use render::{DiffRender, TerminalDiffRender};
pub use table::{Table, TableMeta};
pub use tables::Tables;
pub use unit::{Ordering, Unit};
pub use value::{Cell, Value};
