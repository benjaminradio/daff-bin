//! Rust port of [daff](https://github.com/paulfitz/daff): tabular diff, patch, merge.
//!
//! The public API mirrors the reference library's shape:
//!
//! | daff (JS)                        | this crate                      |
//! |----------------------------------|---------------------------------|
//! | `new daff.Table(rows)`           | [`Table::from_rows`]            |
//! | `daff.compareTables(a, b)`       | [`compare_tables`]              |
//! | `align()`                        | [`Compare::align`]              |
//! | `new daff.TableDiff(al, flags)`  | [`TableDiff::new`]              |
//! | `new daff.HighlightPatch(t, d)`  | [`HighlightPatch::new`]         |
//! | `new daff.Merger(p, a, b, f)`    | [`Merger::new`]                 |

pub mod align;
pub mod diff;
pub mod error;
pub mod flags;
pub mod format;
pub mod merge;
pub mod patch;
pub mod table;

pub use align::{compare_tables, Alignment, Compare};
pub use diff::TableDiff;
pub use error::{Error, Result};
pub use flags::Flags;
pub use merge::Merger;
pub use patch::HighlightPatch;
pub use table::Table;
