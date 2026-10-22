use crate::{Error, Result, Table};

/// Applies a highlighter-format diff to a table.
pub struct HighlightPatch<'a> {
    pub table: &'a mut Table,
    pub patch: &'a Table,
}

impl<'a> HighlightPatch<'a> {
    pub fn new(table: &'a mut Table, patch: &'a Table) -> Self {
        Self { table, patch }
    }

    pub fn apply(&mut self) -> Result<()> {
        Err(Error::Unimplemented("HighlightPatch::apply"))
    }
}
