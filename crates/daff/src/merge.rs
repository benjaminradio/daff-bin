use crate::{Flags, Result, Table};

/// Three-way merge of `parent`, `local`, `remote`.
pub struct Merger<'a> {
    pub parent: &'a Table,
    pub local: &'a Table,
    pub remote: &'a Table,
    pub flags: Flags,
}

impl<'a> Merger<'a> {
    pub fn new(parent: &'a Table, local: &'a Table, remote: &'a Table, flags: Flags) -> Self {
        Self { parent, local, remote, flags }
    }

    /// Returns the number of conflicts; the merged result is written into `local`.
    pub fn apply(&self) -> Result<usize> {
        Err(crate::Error::Unimplemented("Merger::apply"))
    }
}
