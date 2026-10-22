use super::{TableReader, TableWriter};
use crate::{Result, Table};
use std::io::{Read, Write};

#[derive(Debug, Clone, Copy)]
pub struct Csv {
    pub delimiter: u8,
}

impl Default for Csv {
    fn default() -> Self {
        Self { delimiter: b',' }
    }
}

impl TableReader for Csv {
    fn read(&self, input: &mut dyn Read) -> Result<Table> {
        let mut rdr = ::csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(self.delimiter)
            .from_reader(input);
        let mut t = Table::new();
        for rec in rdr.records() {
            t.push_row(rec?.iter().map(|c| Some(c.to_string())).collect());
        }
        Ok(t)
    }
}

impl TableWriter for Csv {
    fn write(&self, table: &Table, out: &mut dyn Write) -> Result<()> {
        let mut w = ::csv::WriterBuilder::new()
            .delimiter(self.delimiter)
            .flexible(true)
            .from_writer(out);
        for row in table.rows() {
            w.write_record(row.iter().map(|c| c.as_deref().unwrap_or("")))?;
        }
        w.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = "a,b\n1,\"x,y\"\n";
        let t = Csv::default().read(&mut src.as_bytes()).unwrap();
        assert_eq!(t.cell(1, 1), Some("x,y"));
        let mut out = Vec::new();
        Csv::default().write(&t, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), src);
    }
}
