//! Three-way merge, ported from `coopy.Merger`.
use crate::align::compare_tables3;
use crate::unit::Unit;
use crate::{Flags, Result, Table};
use std::collections::HashMap;

/// A cell where both sides changed to different values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub row: usize,
    pub col: usize,
    pub parent: String,
    pub local: String,
    pub remote: String,
}

/// Three-way merge of `parent`, `local`, `remote`; the result replaces `local`.
pub struct Merger<'a> {
    parent: &'a Table,
    local: &'a mut Table,
    remote: &'a Table,
    flags: Flags,
    conflict_infos: Vec<ConflictInfo>,
}

fn shuffle_dimension(
    dim_units: &[Unit],
    len: usize,
    fate: &mut Vec<Option<usize>>,
    cl: &mut HashMap<isize, usize>,
    cr: &mut HashMap<isize, usize>,
) -> usize {
    let mut at = 0;
    for u in dim_units {
        if u.p < 0 {
            if u.l < 0 {
                if u.r >= 0 {
                    cr.insert(u.r, at);
                    at += 1;
                }
            } else {
                cl.insert(u.l, at);
                at += 1;
            }
        } else if u.l >= 0 && u.r >= 0 {
            cl.insert(u.l, at);
            at += 1;
        }
    }
    for x in 0..len {
        fate.push(cl.get(&(x as isize)).copied());
    }
    at
}

impl<'a> Merger<'a> {
    pub fn new(parent: &'a Table, local: &'a mut Table, remote: &'a Table, flags: Flags) -> Self {
        Merger { parent, local, remote, flags, conflict_infos: Vec::new() }
    }

    pub fn conflict_infos(&self) -> &[ConflictInfo] {
        &self.conflict_infos
    }

    /// Returns the number of conflicts; the merged result is written into `local`.
    pub fn apply(&mut self) -> Result<usize> {
        let mut conflicts = 0;
        self.conflict_infos.clear();
        // The reference merges without the caller's flags (no id columns).
        let align = compare_tables3(Some(self.parent), self.local, self.remote, None).align();
        let units = align.to_order().list().to_vec();
        let column_units = align.meta.as_ref().expect("column alignment").to_order().list().to_vec();
        let _ = (self.flags.allow_insert(), self.flags.allow_delete(), self.flags.allow_update());

        for row in &units {
            if row.l >= 0 && row.r >= 0 && row.p >= 0 {
                for col in &column_units {
                    if col.l >= 0 && col.r >= 0 && col.p >= 0 {
                        let pcell = self.parent.get(col.p as usize, row.p as usize);
                        let rcell = self.remote.get(col.r as usize, row.r as usize);
                        if pcell != rcell {
                            let lcell = self.local.get(col.l as usize, row.l as usize);
                            if pcell == lcell {
                                let v = rcell.map(String::from);
                                self.local.set(col.l as usize, row.l as usize, v);
                            } else if rcell != lcell {
                                let (p, l, r) = (
                                    pcell.unwrap_or("").to_string(),
                                    lcell.unwrap_or("").to_string(),
                                    rcell.unwrap_or("").to_string(),
                                );
                                let cell = format!("((( {p} ))) {l} /// {r}");
                                self.local.set(col.l as usize, row.l as usize, Some(cell));
                                conflicts += 1;
                                self.conflict_infos.push(ConflictInfo {
                                    row: row.l as usize,
                                    col: col.l as usize,
                                    parent: p,
                                    local: l,
                                    remote: r,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Rearrange columns and rows as appropriate.
        let (mut col_local, mut col_remote) = (HashMap::new(), HashMap::new());
        let mut fate = Vec::new();
        let wfate = shuffle_dimension(&column_units, self.local.width(), &mut fate, &mut col_local, &mut col_remote);
        self.local.insert_or_delete_columns(&fate, wfate);

        let (mut row_local, mut row_remote) = (HashMap::new(), HashMap::new());
        let mut fate = Vec::new();
        let hfate = shuffle_dimension(&units, self.local.height(), &mut fate, &mut row_local, &mut row_remote);
        self.local.insert_or_delete_rows(&fate, hfate);

        // Paste in any new columns.
        for (&x, &x2) in &col_remote {
            for unit in &units {
                if unit.l >= 0 && unit.r >= 0 {
                    if let Some(&y) = row_local.get(&unit.l) {
                        let v = self.remote.get(x as usize, unit.r as usize).map(String::from);
                        self.local.set(x2, y, v);
                    }
                } else if unit.p < 0 && unit.r >= 0 {
                    if let Some(&y) = row_remote.get(&unit.r) {
                        let v = self.remote.get(x as usize, unit.r as usize).map(String::from);
                        self.local.set(x2, y, v);
                    }
                }
            }
        }
        // Paste in any new rows.
        for (&y, &y2) in &row_remote {
            for unit in &column_units {
                if unit.l >= 0 && unit.r >= 0 {
                    if let Some(&x) = col_local.get(&unit.l) {
                        let v = self.remote.get(unit.r as usize, y as usize).map(String::from);
                        self.local.set(x, y2, v);
                    }
                }
            }
        }
        Ok(conflicts)
    }
}
