use crate::{Flags, Table};

/// Row/column correspondence between two tables.
#[derive(Debug, Clone, Default)]
pub struct Alignment {
    /// Pairs of (row in local, row in remote); `None` on one side means add/remove.
    pub rows: Vec<(Option<usize>, Option<usize>)>,
    pub cols: Vec<(Option<usize>, Option<usize>)>,
}

pub struct Compare<'a> {
    pub local: &'a Table,
    pub remote: &'a Table,
    pub flags: Flags,
}

pub fn compare_tables<'a>(local: &'a Table, remote: &'a Table) -> Compare<'a> {
    Compare { local, remote, flags: Flags::default() }
}

impl Compare<'_> {
    pub fn with_flags(mut self, flags: Flags) -> Self {
        self.flags = flags;
        self
    }

    pub fn align(&self) -> Alignment {
        todo!("port daff Coopy/CompareTable alignment")
    }
}
