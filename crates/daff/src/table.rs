/// A dense, rectangular-ish grid of cells. Cells are nullable strings,
/// matching daff's treatment of `null` vs empty string.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    rows: Vec<Vec<Option<String>>>,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_rows<I, R, S>(rows: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            rows: rows
                .into_iter()
                .map(|r| r.into_iter().map(|c| Some(c.into())).collect())
                .collect(),
        }
    }

    pub fn height(&self) -> usize {
        self.rows.len()
    }

    pub fn width(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<&str> {
        self.rows.get(y)?.get(x)?.as_deref()
    }

    pub fn rows(&self) -> &[Vec<Option<String>>] {
        &self.rows
    }

    pub fn push_row(&mut self, row: Vec<Option<String>>) {
        self.rows.push(row);
    }
}
