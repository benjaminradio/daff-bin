use std::fmt;

/// A table of nullable string cells, ported from `coopy.SimpleTable`.
///
/// Storage is a flat vector indexed by `x + y * width`, exactly as in the
/// reference: [`Table::resize`] changes the dimensions without moving data.
/// Cells that were never set are `None` (null), which is distinct from `""`.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Table {
    data: Vec<Option<String>>,
    w: usize,
    h: usize,
}

impl Table {
    pub fn new(w: usize, h: usize) -> Self {
        Table { data: vec![None; w * h], w, h }
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
        self.data.get(x + y * self.w)?.as_deref()
    }

    pub fn set<S: Into<String>>(&mut self, x: usize, y: usize, c: Option<S>) {
        let idx = x + y * self.w;
        if idx >= self.data.len() {
            self.data.resize(idx + 1, None);
        }
        self.data[idx] = c.map(Into::into);
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
                        data[j * self.w + c] = v.clone();
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
                        data[r * wfate + j] = v.clone();
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
        let h_test = self.h.min(3);
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
