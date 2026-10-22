//! A set of named output tables (`coopy.Tables`): the base diff table plus one
//! table per nested table that was compared.
use crate::Table;

#[derive(Debug, Clone, Default)]
pub struct Tables {
    tables: Vec<(String, Option<Table>)>,
    has_ins_del: bool,
}

impl Tables {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a table; returns it for filling in.
    pub fn add(&mut self, name: &str) -> &mut Table {
        self.tables.push((name.to_string(), Some(Table::new(0, 0))));
        self.tables.last_mut().unwrap().1.as_mut().unwrap()
    }

    pub fn order(&self) -> Vec<&str> {
        self.tables.iter().map(|(n, _)| n.as_str()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|(n, _)| n == name).and_then(|(_, t)| t.as_ref())
    }

    /// The first table (the base diff).
    pub fn one(&self) -> Option<&Table> {
        self.tables.first().and_then(|(_, t)| t.as_ref())
    }

    pub fn set_ins_del(&mut self, v: bool) {
        self.has_ins_del = v;
    }

    /// True if the base alignment added or removed rows (tables).
    pub fn has_ins_del(&self) -> bool {
        self.has_ins_del
    }
}
