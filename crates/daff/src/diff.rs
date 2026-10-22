use crate::{Alignment, Flags, Result, Table};

/// Produces a highlighter-format diff table from an [`Alignment`].
pub struct TableDiff<'a> {
    pub alignment: &'a Alignment,
    pub flags: Flags,
}

impl<'a> TableDiff<'a> {
    pub fn new(alignment: &'a Alignment, flags: Flags) -> Self {
        Self { alignment, flags }
    }

    pub fn highlight(&self) -> Result<Table> {
        Err(crate::Error::Unimplemented("TableDiff::highlight"))
    }
}
