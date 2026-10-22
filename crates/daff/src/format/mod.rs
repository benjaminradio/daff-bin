//! Pluggable table I/O. Phase 2 (xlsx) adds a reader/writer here.
use crate::{Result, Table};
use std::io::{Read, Write};

pub mod csv;
pub mod json;

pub trait TableReader {
    fn read(&self, input: &mut dyn Read) -> Result<Table>;
}

pub trait TableWriter {
    fn write(&self, table: &Table, out: &mut dyn Write) -> Result<()>;
}
