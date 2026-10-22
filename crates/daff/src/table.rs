use crate::value::Cell;
use std::fmt;
use std::rc::Rc;

/// Extra information about where a table came from (`coopy.Meta`), used for
/// tables that hold other tables.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableMeta {
    /// Cells of this table may themselves be tables.
    pub nested: bool,
    pub name: Option<String>,
}

/// A table of nullable string cells, ported from `coopy.SimpleTable`.
///
/// Storage is a flat vector indexed by `x + y * width`, exactly as in the
/// reference: [`Table::resize`] changes the dimensions without moving data.
/// Cells that were never set are `None` (null), which is distinct from `""`.
#[derive(Clone, Default, PartialEq)]
pub struct Table {
    data: Vec<Option<Cell>>,
    w: usize,
    h: usize,
    meta: Option<Rc<TableMeta>>,
}

impl Table {
    pub fn new(w: usize, h: usize) -> Self {
        Table { data: vec![None; w * h], w, h, meta: None }
    }

    pub fn from_rows<I, R, S>(rows: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let rows: Vec<Vec<String>> =
            rows.into_iter().map(|r| r.into_iter().map(Into::into).collect()).collect();
        let w = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut t = Table::new(w, rows.len());
        for (y, r) in rows.into_iter().enumerate() {
            for (x, c) in r.into_iter().enumerate() {
                t.set(x, y, Some(c));
            }
        }
        t
    }

    pub fn meta(&self) -> Option<&TableMeta> {
        self.meta.as_deref()
    }

    pub fn set_meta(&mut self, meta: Option<TableMeta>) {
        self.meta = meta.map(Rc::new);
    }

    pub fn width(&self) -> usize {
        self.w
    }

    pub fn height(&self) -> usize {
        self.h
    }

    /// Cell content; `None` for null or out of range.
    pub fn get(&self, x: usize, y: usize) -> Option<&str> {
        if x >= self.w || y >= self.h {
            return None;
        }
        self.data.get(x + y * self.w)?.as_ref().map(Cell::text)
    }

    pub fn set<S: Into<String>>(&mut self, x: usize, y: usize, c: Option<S>) {
        self.set_cell(x, y, c.map(|s| Cell::Str(s.into())));
    }

    /// The full cell value (which may be a non-string, from JSON input).
    pub fn get_cell(&self, x: usize, y: usize) -> Option<&Cell> {
        if x >= self.w || y >= self.h {
            return None;
        }
        self.data.get(x + y * self.w)?.as_ref()
    }

    pub fn set_cell(&mut self, x: usize, y: usize, c: Option<Cell>) {
        let idx = x + y * self.w;
        if idx >= self.data.len() {
            self.data.resize(idx + 1, None);
        }
        self.data[idx] = c;
    }

    /// Change dimensions without relocating data (flat index semantics).
    pub fn resize(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        if self.data.len() < w * h {
            self.data.resize(w * h, None);
        }
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.data.resize(self.w * self.h, None);
    }

    /// Move rows: `fate[i]` is the new position of row `i`, or `None` to drop it.
    pub fn insert_or_delete_rows(&mut self, fate: &[Option<usize>], hfate: usize) {
        let mut data = vec![None; self.w * hfate];
        for (i, f) in fate.iter().enumerate() {
            if let Some(j) = *f {
                for c in 0..self.w {
                    if let Some(v) = self.data.get(i * self.w + c) {
                        let idx = j * self.w + c;
                        if idx >= data.len() {
                            data.resize(idx + 1, None);
                        }
                        data[idx] = v.clone();
                    }
                }
            }
        }
        self.h = hfate;
        self.data = data;
    }

    /// Move columns: `fate[i]` is the new position of column `i`, or `None` to drop it.
    pub fn insert_or_delete_columns(&mut self, fate: &[Option<usize>], wfate: usize) {
        let mut data = vec![None; wfate * self.h];
        for (i, f) in fate.iter().enumerate() {
            if let Some(j) = *f {
                for r in 0..self.h {
                    if let Some(v) = self.data.get(r * self.w + i) {
                        let idx = r * wfate + j;
                        if idx >= data.len() {
                            data.resize(idx + 1, None);
                        }
                        data[idx] = v.clone();
                    }
                }
            }
        }
        self.w = wfate;
        self.data = data;
    }

    /// Drop trailing blank rows, then trailing columns that are blank in the top rows.
    pub fn trim_blank(&mut self) {
        if self.h == 0 {
            return;
        }
        let blank = |c: Option<&str>| matches!(c, None | Some(""));
        // The reference only looks at the top three rows when deciding whether
        // trailing columns are blank, which drops data below them (B-05).
        let h_test = if crate::compat::reference_compat() { self.h.min(3) } else { self.h };
        loop {
            if self.h == 0 || !(0..self.w).all(|i| blank(self.get(i, self.h - 1))) {
                break;
            }
            self.h -= 1;
        }
        let mut nw = self.w;
        while nw > 0 && (0..h_test).all(|i| blank(self.get(nw - 1, i))) {
            nw -= 1;
        }
        if nw == self.w {
            return;
        }
        let mut data = vec![None; nw * self.h];
        for r in 0..self.h {
            for i in 0..nw {
                data[r * nw + i] = self.data.get(r * self.w + i).cloned().flatten();
            }
        }
        self.w = nw;
        self.data = data;
    }

    pub fn is_similar(&self, other: &Table) -> bool {
        self.w == other.w
            && self.h == other.h
            && (0..self.h).all(|y| (0..self.w).all(|x| self.get(x, y) == other.get(x, y)))
    }
}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.h {
            for x in 0..self.w {
                if x > 0 {
                    f.write_str(",")?;
                }
                f.write_str(self.get(x, y).unwrap_or("null"))?;
            }
            f.write_str("\n")?;
        }
        Ok(())
    }
}
