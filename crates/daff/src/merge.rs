//! Three-way merge, ported from `coopy.Merger`.
use crate::align::compare_tables3;
use crate::unit::Unit;
use crate::{Flags, Result, Table};
use std::collections::HashMap;

/// What kind of disagreement a conflict records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both sides changed a cell to different values.
    Cell,
    /// One side deleted a row that the other side changed (the changed row is kept).
    RowDeleted,
    /// One side deleted a column that the other side changed (the changed column is kept).
    ColumnDeleted,
}

/// A place where the two sides disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictInfo {
    pub kind: ConflictKind,
    /// For deletions, the row (or column) of the side that kept it.
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

    fn is_ignored_column(&self, c: &Unit) -> bool {
        let names = [
            (self.parent, c.p),
            (&*self.local, c.l),
            (self.remote, c.r),
        ];
        names.iter().any(|(t, x)| *x >= 0 && self.flags.is_ignored(t.get(*x as usize, 0).unwrap_or("")))
    }

    /// Decide what to do with rows and columns that one side deleted and the
    /// other changed, and with rows both sides added (B-08, B-09). Returns
    /// the number of conflicts found.
    fn resolve_structure(&mut self, units: &mut Vec<Unit>, column_units: &mut Vec<Unit>) -> usize {
        let mut conflicts = 0;
        let (parent, local, remote) = (self.parent, &*self.local, self.remote);
        let cols: Vec<Unit> = column_units.to_vec();
        let rows: Vec<Unit> = units.to_vec();

        // Rows deleted on one side but changed on the other.
        let mut infos = Vec::new();
        for u in units.iter_mut() {
            if u.p < 0 {
                continue;
            }
            let changed_vs_parent = |t: &Table, row: isize, side_l: bool| -> bool {
                cols.iter().any(|c| {
                    let x = if side_l { c.l } else { c.r };
                    c.p >= 0 && x >= 0 && parent.get(c.p as usize, u.p as usize) != t.get(x as usize, row as usize)
                })
            };
            if u.l >= 0 && u.r < 0 && changed_vs_parent(local, u.l, true) {
                infos.push(ConflictInfo {
                    kind: ConflictKind::RowDeleted,
                    row: u.l as usize,
                    col: 0,
                    parent: String::new(),
                    local: String::new(),
                    remote: String::new(),
                });
                u.p = -1; // keep the local row as if it were new
                conflicts += 1;
            } else if u.l < 0 && u.r >= 0 && changed_vs_parent(remote, u.r, false) {
                infos.push(ConflictInfo {
                    kind: ConflictKind::RowDeleted,
                    row: u.r as usize,
                    col: 0,
                    parent: String::new(),
                    local: String::new(),
                    remote: String::new(),
                });
                u.p = -1; // bring the remote row back as if it were new
                conflicts += 1;
            }
        }

        // The same row added on both sides becomes one row.
        let common: Vec<&Unit> = cols.iter().filter(|c| c.l >= 0 && c.r >= 0).collect();
        let mut local_added: HashMap<Vec<Option<&str>>, Vec<usize>> = HashMap::new();
        for (i, u) in rows.iter().enumerate() {
            if u.p < 0 && u.l >= 0 && u.r < 0 {
                let key = common.iter().map(|c| local.get(c.l as usize, u.l as usize)).collect();
                local_added.entry(key).or_default().push(i);
            }
        }
        for (i, u) in rows.iter().enumerate() {
            if u.p < 0 && u.l < 0 && u.r >= 0 {
                let key: Vec<Option<&str>> = common.iter().map(|c| remote.get(c.r as usize, u.r as usize)).collect();
                if let Some(list) = local_added.get_mut(&key) {
                    if !list.is_empty() {
                        let li = list.remove(0);
                        units[li].r = u.r;
                        units[i] = Unit::new(-1, -1, -1);
                    }
                }
            }
        }

        // Columns deleted on one side but changed on the other.
        let row_pairs: Vec<Unit> = rows.iter().filter(|r| r.p >= 0).copied().collect();
        for c in column_units.iter_mut() {
            if c.p < 0 {
                continue;
            }
            let changed = |t: &Table, x: isize, side_l: bool| -> bool {
                row_pairs.iter().any(|r| {
                    let y = if side_l { r.l } else { r.r };
                    y >= 0 && parent.get(c.p as usize, r.p as usize) != t.get(x as usize, y as usize)
                })
            };
            if c.l >= 0 && c.r < 0 && changed(local, c.l, true) {
                infos.push(ConflictInfo {
                    kind: ConflictKind::ColumnDeleted,
                    row: 0,
                    col: c.l as usize,
                    parent: String::new(),
                    local: String::new(),
                    remote: String::new(),
                });
                c.p = -1;
                conflicts += 1;
            } else if c.l < 0 && c.r >= 0 && changed(remote, c.r, false) {
                infos.push(ConflictInfo {
                    kind: ConflictKind::ColumnDeleted,
                    row: 0,
                    col: c.r as usize,
                    parent: String::new(),
                    local: String::new(),
                    remote: String::new(),
                });
                c.p = -1;
                conflicts += 1;
            }
        }
        self.conflict_infos.extend(infos);
        // A remote row/column that was deleted locally and is now kept sorts
        // by the local index, which it does not have; put it after its
        // predecessor in the remote table instead.
        for list in [&mut *units, &mut *column_units] {
            let kept: Vec<Unit> = list.iter().filter(|u| u.l < 0 && u.p < 0 && u.r > 0).copied().collect();
            let mut kept = kept;
            kept.sort_by_key(|u| u.r);
            list.retain(|u| !(u.l < 0 && u.p < 0 && u.r > 0));
            for k in kept {
                let at = list.iter().position(|u| u.r == k.r - 1).map_or(list.len(), |i| i + 1);
                list.insert(at, k);
            }
        }
        conflicts
    }

    /// Returns the number of conflicts; the merged result is written into `local`.
    pub fn apply(&mut self) -> Result<usize> {
        let mut conflicts = 0;
        self.conflict_infos.clear();
        let compat = crate::compat::reference_compat();
        // The reference merges without the caller's flags (no id columns; B-22).
        let align_flags = if compat { None } else { Some(&self.flags) };
        let align = compare_tables3(Some(self.parent), self.local, self.remote, align_flags).align();
        let mut units: Vec<Unit> = align.to_order().list().to_vec();
        let mut column_units: Vec<Unit> = align.meta.as_ref().expect("column alignment").to_order().list().to_vec();
        if !compat {
            conflicts += self.resolve_structure(&mut units, &mut column_units);
        }
        let ignored: Vec<bool> = column_units
            .iter()
            .map(|c| !compat && self.is_ignored_column(c))
            .collect();
        let _ = (self.flags.allow_insert(), self.flags.allow_delete(), self.flags.allow_update());

        for row in &units {
            if row.l >= 0 && row.r >= 0 && row.p >= 0 {
                for (ci, col) in column_units.iter().enumerate() {
                    if ignored[ci] {
                        continue;
                    }
                    if col.l >= 0 && col.r >= 0 && col.p >= 0 {
                        let pcell = self.parent.get(col.p as usize, row.p as usize);
                        let rcell = self.remote.get(col.r as usize, row.r as usize);
                        if pcell != rcell {
                            let lcell = self.local.get(col.l as usize, row.l as usize);
                            if pcell == lcell {
                                let v = self.remote.get_cell(col.r as usize, row.r as usize).cloned();
                                self.local.set_cell(col.l as usize, row.l as usize, v);
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
                                    kind: ConflictKind::Cell,
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
                        let v = self.remote.get_cell(x as usize, unit.r as usize).cloned();
                        self.local.set_cell(x2, y, v);
                    }
                } else if unit.p < 0 && unit.r >= 0 {
                    if let Some(&y) = row_remote.get(&unit.r) {
                        let v = self.remote.get_cell(x as usize, unit.r as usize).cloned();
                        self.local.set_cell(x2, y, v);
                    }
                }
            }
        }
        // Paste in any new rows.
        for (&y, &y2) in &row_remote {
            for unit in &column_units {
                if unit.l >= 0 && unit.r >= 0 {
                    if let Some(&x) = col_local.get(&unit.l) {
                        let v = self.remote.get_cell(unit.r as usize, y as usize).cloned();
                        self.local.set_cell(x, y2, v);
                    }
                }
            }
        }
        Ok(conflicts)
    }
}
