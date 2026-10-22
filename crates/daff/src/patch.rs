//! Apply a highlighter-format diff to a table, ported from
//! `coopy.HighlightPatch` (meta tables, SQL sources and metadata rows are not
//! supported; metadata rows are skipped as in the reference).
use crate::align::{compare_tables, IndexPair, Row};
use crate::cell_info::{examine_cell, CellInfo};
use crate::format::csv::Csv;
use crate::unit::stable_sort_by;
use crate::{Result, Table};
use std::collections::HashMap;

/// Map that iterates in insertion order, like the reference's `StringMap`
/// (the order matters when several patch columns land on the same output column).
#[derive(Default)]
struct OrderedMap {
    pos: HashMap<String, usize>,
    entries: Vec<(String, usize)>,
}

impl OrderedMap {
    fn clear(&mut self) {
        self.pos.clear();
        self.entries.clear();
    }
    fn insert(&mut self, k: String, v: usize) {
        match self.pos.get(&k) {
            Some(&i) => self.entries[i].1 = v,
            None => {
                self.pos.insert(k.clone(), self.entries.len());
                self.entries.push((k, v));
            }
        }
    }
    fn get(&self, k: &str) -> Option<&usize> {
        self.pos.get(k).map(|&i| &self.entries[i].1)
    }
    fn values(&self) -> impl Iterator<Item = &usize> {
        self.entries.iter().map(|e| &e.1)
    }
}

#[derive(Debug, Clone)]
struct PatchUnit {
    add: bool,
    rem: bool,
    update: bool,
    code: String,
    source_row: isize,
    source_row_offset: isize,
    source_prev_row: isize,
    source_next_row: isize,
    dest_row: isize,
    patch_row: isize,
}

impl PatchUnit {
    fn new() -> Self {
        PatchUnit {
            add: false,
            rem: false,
            update: false,
            code: String::new(),
            source_row: -1,
            source_row_offset: 0,
            source_prev_row: -1,
            source_next_row: -1,
            dest_row: -1,
            patch_row: -1,
        }
    }
}

fn sort_mods(a: &PatchUnit, b: &PatchUnit) -> i64 {
    let ord = |o: std::cmp::Ordering| o as i64;
    if b.code == "@@" && a.code != "@@" {
        return 1;
    }
    if a.code == "@@" && b.code != "@@" {
        return -1;
    }
    if a.source_row == -1 && !a.add && b.source_row != -1 {
        return 1;
    }
    if a.source_row != -1 && !b.add && b.source_row == -1 {
        return -1;
    }
    let (ka, kb) = (a.source_row + a.source_row_offset, b.source_row + b.source_row_offset);
    if ka != kb {
        return ord(ka.cmp(&kb));
    }
    ord(a.patch_row.cmp(&b.patch_row))
}

/// Applies a highlighter-format diff to a table.
pub struct HighlightPatch<'a> {
    source: &'a mut Table,
    patch: &'a Table,

    header: HashMap<usize, String>,
    header_pre: OrderedMap,
    header_post: OrderedMap,
    header_rename: HashMap<String, String>,
    header_move: Option<HashMap<String, bool>>,
    modifier: HashMap<usize, String>,
    current_row: isize,
    payload_col: usize,
    payload_top: usize,
    mods: Vec<PatchUnit>,
    cmods: Vec<PatchUnit>,
    row_info: CellInfo,
    rc_offset: usize,
    indexes: Option<Vec<IndexPair>>,
    source_in_patch_col: Option<HashMap<usize, usize>>,
    patch_in_source_col: HashMap<usize, usize>,
    patch_in_dest_col: Option<HashMap<usize, isize>>,
    patch_in_source_row: HashMap<isize, isize>,
    last_source_row: isize,
    actions: Vec<String>,
    have_dropped_columns: bool,
    preamble_row: isize,
    finished_columns: bool,
}

/// Lets the index query rows of the patch as if they were rows of the source.
struct PatchRow<'p, 'a>(&'p HighlightPatch<'a>);

impl Row for PatchRow<'_, '_> {
    fn row_string(&self, c: usize) -> Option<String> {
        self.0.get_row_string(c)
    }
    fn is_preamble(&self) -> bool {
        self.0.current_row <= self.0.preamble_row
    }
}

impl<'a> HighlightPatch<'a> {
    pub fn new(source: &'a mut Table, patch: &'a Table) -> Self {
        HighlightPatch {
            source,
            patch,
            header: HashMap::new(),
            header_pre: OrderedMap::default(),
            header_post: OrderedMap::default(),
            header_rename: HashMap::new(),
            header_move: None,
            modifier: HashMap::new(),
            current_row: -1,
            payload_col: 0,
            payload_top: 0,
            mods: Vec::new(),
            cmods: Vec::new(),
            row_info: CellInfo::default(),
            rc_offset: 0,
            indexes: None,
            source_in_patch_col: None,
            patch_in_source_col: HashMap::new(),
            patch_in_dest_col: None,
            patch_in_source_row: HashMap::new(),
            last_source_row: -1,
            actions: Vec::new(),
            have_dropped_columns: false,
            preamble_row: 0,
            finished_columns: false,
        }
    }

    pub fn apply(&mut self) -> Result<bool> {
        self.header.clear();
        self.header_pre.clear();
        self.header_post.clear();
        self.header_rename.clear();
        self.header_move = None;
        self.modifier.clear();
        self.mods.clear();
        self.cmods.clear();
        self.rc_offset = 0;
        self.current_row = -1;
        self.row_info = CellInfo::default();
        self.source_in_patch_col = None;
        self.patch_in_source_col.clear();
        self.patch_in_dest_col = None;
        self.patch_in_source_row.clear();
        self.indexes = None;
        self.last_source_row = -1;
        self.actions.clear();
        self.have_dropped_columns = false;
        self.preamble_row = 0;
        self.finished_columns = false;

        let patch = self.patch;
        if patch.width() < 2 || patch.height() < 1 {
            return Ok(true);
        }
        // Computed before `rc_offset` is known, as in the reference: the
        // payload always starts at column 1.
        self.payload_col = 1 + self.rc_offset;
        self.payload_top = patch.width();
        self.rc_offset = usize::from(patch.get(0, 0).unwrap_or("") == "@:@");
        for r in 0..patch.height() {
            self.actions.push(patch.get(self.rc_offset, r).unwrap_or("").to_string());
        }
        self.preamble_row = self.rc_offset as isize;
        for r in 0..patch.height() {
            self.apply_row(r as isize);
        }
        self.finish_columns();
        self.finish_rows();
        Ok(true)
    }

    fn action_at(&self, r: isize) -> Option<&str> {
        usize::try_from(r).ok().and_then(|r| self.actions.get(r)).map(String::as_str)
    }

    fn need_source_columns(&mut self) {
        if self.source_in_patch_col.is_some() {
            return;
        }
        let mut sip = HashMap::new();
        self.patch_in_source_col.clear();
        for i in 0..self.source.width() {
            let name = self.source.get(i, 0).unwrap_or("");
            if let Some(&at) = self.header_pre.get(name) {
                sip.insert(i, at);
                self.patch_in_source_col.insert(at, i);
            }
        }
        self.source_in_patch_col = Some(sip);
    }

    fn need_dest_columns(&mut self) {
        if self.patch_in_dest_col.is_some() {
            return;
        }
        let mut m = HashMap::new();
        for cmod in &self.cmods {
            if cmod.patch_row != -1 {
                m.insert(cmod.patch_row as usize, cmod.dest_row);
            }
        }
        self.patch_in_dest_col = Some(m);
    }

    fn need_source_index(&mut self) {
        if self.indexes.is_some() {
            return;
        }
        let (_, idx) = compare_tables(self.source, self.source, None).align_storing_indexes();
        self.indexes = Some(idx);
        self.need_source_columns();
    }

    fn apply_row(&mut self, r: isize) {
        self.current_row = r;
        let mut code = self.actions[r as usize].clone();
        let mut done = false;
        if r == 0 && self.rc_offset > 0 {
            done = true;
        } else if code == "@@" {
            self.preamble_row = r;
            self.apply_header();
            self.apply_action("@@");
            done = true;
        } else if code == "!" {
            self.preamble_row = r;
            self.apply_meta();
            done = true;
        } else if code.starts_with('@') {
            // "cannot usefully apply diffs with metadata yet"
            self.preamble_row = r;
            self.need_source_columns();
            done = true;
        }
        if !done {
            self.finish_columns();
            if code == "+++" || code == "---" || code == "+" || code == ":" {
                self.apply_action(&code);
            } else if code.contains("->") {
                code = "->".into();
                self.apply_action(&code);
            } else {
                self.last_source_row = -1;
            }
        }
    }

    fn get_string(&self, c: usize) -> String {
        self.get_datum(c).unwrap_or("").to_string()
    }

    fn get_datum(&self, c: usize) -> Option<&str> {
        self.patch.get(c, self.current_row as usize)
    }

    fn apply_meta(&mut self) {
        for i in self.payload_col..self.payload_top {
            let name = self.get_string(i);
            if name.is_empty() {
                continue;
            }
            self.modifier.insert(i, name);
        }
    }

    fn apply_header(&mut self) {
        for i in self.payload_col..self.payload_top {
            let name = self.get_string(i);
            if name == "..." {
                self.modifier.insert(i, "...".into());
                self.have_dropped_columns = true;
                continue;
            }
            let mut mod_ = self.modifier.get(&i).cloned();
            let mut mv = false;
            if let Some(m) = &mod_ {
                if m.starts_with(':') {
                    mv = true;
                    mod_ = Some(m[1..].to_string());
                }
            }
            self.header.insert(i, name.clone());
            if let Some(m) = &mod_ {
                if m.starts_with('(') {
                    let prev_name: String = m.chars().skip(1).take(m.chars().count().saturating_sub(2)).collect();
                    self.header_pre.insert(prev_name.clone(), i);
                    self.header_post.insert(name.clone(), i);
                    self.header_rename.insert(prev_name, name);
                    continue;
                }
            }
            if mod_.as_deref() != Some("+++") {
                self.header_pre.insert(name.clone(), i);
            }
            if mod_.as_deref() != Some("---") {
                self.header_post.insert(name.clone(), i);
            }
            if mv {
                self.header_move.get_or_insert_with(HashMap::new).insert(name, true);
            }
        }
        if self.source.height() == 0 {
            self.apply_action("+++");
        }
    }

    fn look_up(&mut self, del: isize) -> isize {
        if let Some(&r) = self.patch_in_source_row.get(&(self.current_row + del)) {
            return r;
        }
        let mut result = -1;
        self.current_row += del;
        if self.current_row >= 0 && (self.current_row as usize) < self.patch.height() {
            let indexes = self.indexes.take().unwrap_or_default();
            for idx in &indexes {
                let m = idx.query_by_content(&PatchRow(self));
                if m.spot_a == 0 {
                    continue;
                }
                if m.spot_a == 1 {
                    result = m.item_a[0] as isize;
                    break;
                }
                if self.current_row > 0 {
                    if let Some(&prev) = self.patch_in_source_row.get(&(self.current_row - 1)) {
                        // The reference's `break` here only leaves the inner loop, so
                        // later indexes may still override this result.
                        if let Some(&row) = m.item_a.iter().find(|&&row| row as isize == prev + 1) {
                            result = row as isize;
                        }
                    }
                }
            }
            self.indexes = Some(indexes);
        }
        self.patch_in_source_row.insert(self.current_row, result);
        self.current_row -= del;
        result
    }

    fn apply_action(&mut self, code: &str) {
        let mut m = PatchUnit::new();
        m.code = code.into();
        m.add = code == "+++";
        m.rem = code == "---";
        m.update = code == "->";
        self.need_source_index();
        if self.last_source_row == -1 {
            self.last_source_row = self.look_up(-1);
        }
        m.source_prev_row = self.last_source_row;
        let next_act = self.action_at(self.current_row + 1);
        if next_act != Some("+++") && next_act != Some("...") {
            m.source_next_row = self.look_up(1);
        }
        if m.add {
            if self.action_at(self.current_row - 1) != Some("+++") {
                if self.action_at(self.current_row - 1) == Some("@@") {
                    m.source_prev_row = 0;
                    self.last_source_row = 0;
                } else {
                    m.source_prev_row = self.look_up(-1);
                }
            }
            m.source_row = m.source_prev_row;
            if m.source_row != -1 {
                m.source_row_offset = 1;
            }
        } else {
            self.last_source_row = self.look_up(0);
            m.source_row = self.last_source_row;
        }
        if self.action_at(self.current_row + 1) == Some("") {
            self.last_source_row = m.source_next_row;
        }
        m.patch_row = self.current_row;
        if code == "@@" {
            m.source_row = 0;
        }
        self.mods.push(m);
    }

    fn check_act(&mut self) {
        let act = self.get_string(self.rc_offset);
        if self.row_info.value.as_deref() != Some(act.as_str()) {
            self.row_info = examine_cell(0, 0, Some(&act), "", &act, 0);
        }
    }

    fn get_row_string(&self, c: usize) -> Option<String> {
        let Some(&at) = self.source_in_patch_col.as_ref()?.get(&c) else {
            return Some("NOT_FOUND".into()); // column elided from the patch
        };
        // `getPreString` caches into `row_info`; recompute without mutation.
        let act = self.get_string(self.rc_offset);
        let info = examine_cell(0, 0, Some(&act), "", &act, 0);
        let txt = self.get_string(at);
        if !info.updated {
            return Some(txt);
        }
        let cell = examine_cell(0, 0, Some(&txt), "", &act, 0);
        if !cell.updated {
            return Some(txt);
        }
        cell.lvalue
    }

    // This works out rows/cols to add, delete or move.
    fn process_mods(rmods: &mut Vec<PatchUnit>, fate: &mut Vec<isize>, len: usize) -> isize {
        stable_sort_by(rmods, &|a, b| sort_mods(a, b));
        let mut offset: isize = 0;
        let mut last: isize = -1;
        let mut target: isize = 0;
        if let Some(m) = rmods.first() {
            if m.source_prev_row == -1 {
                last = 0;
            }
        }
        for m in rmods.iter_mut() {
            if last != -1 {
                for i in last..(m.source_row + m.source_row_offset) {
                    fate.push(i + offset);
                    target += 1;
                    last += 1;
                }
            }
            if m.rem {
                fate.push(-1);
                offset -= 1;
            } else if m.add {
                m.dest_row = target;
                target += 1;
                offset += 1;
            } else {
                m.dest_row = target;
            }
            if m.source_row >= 0 {
                last = m.source_row + m.source_row_offset;
                if m.rem {
                    last += 1;
                }
            } else if m.add && m.source_next_row != -1 {
                last = m.source_next_row + m.source_row_offset;
            } else if m.rem || m.add {
                last = -1;
            }
        }
        if last != -1 {
            for i in last..len as isize {
                fate.push(i + offset);
            }
        }
        len as isize + offset
    }

    /// Choose a permutation of source rows/columns so that `mods` can be
    /// rewritten with simple previous/next relationships.
    fn compute_ordering(mods: &[PatchUnit], permutation: &mut Vec<isize>, permutation_rev: &mut Vec<usize>, dim: usize) {
        let mut from_unit: HashMap<isize, isize> = HashMap::new();
        let mut meta_from_unit: HashMap<isize, usize> = HashMap::new();
        let mut ct = 0;
        for m in mods {
            if m.add || m.rem || m.source_row < 0 {
                continue;
            }
            if m.source_prev_row >= 0 {
                from_unit.insert(m.source_row, m.source_prev_row);
                if m.source_prev_row + 1 != m.source_row {
                    ct += 1;
                }
            }
            if m.source_next_row >= 0 {
                from_unit.insert(m.source_next_row, m.source_row);
                if m.source_row + 1 != m.source_next_row {
                    ct += 1;
                }
            }
        }
        if ct == 0 {
            return;
        }
        let mut starts: Vec<usize> = Vec::new();
        for i in 0..dim {
            match from_unit.get(&(i as isize)) {
                Some(&u) => {
                    meta_from_unit.insert(u, i);
                }
                None => starts.push(i),
            }
        }
        let mut logical: Option<isize> = None;
        let mut used: HashMap<usize, bool> = HashMap::new();
        for _ in 0..dim {
            let mut cursor: Option<usize> = logical.and_then(|l| meta_from_unit.get(&l).copied());
            if cursor.is_none() {
                let v = if starts.is_empty() { None } else { Some(starts.remove(0)) };
                cursor = v;
            }
            let mut c = cursor.unwrap_or(0);
            while used.contains_key(&c) {
                c = (c + 1) % dim;
            }
            logical = Some(c as isize);
            permutation_rev.push(c);
            used.insert(c, true);
        }
        *permutation = vec![-1; permutation_rev.len()];
        for i in 0..permutation.len() {
            permutation[permutation_rev[i]] = i as isize;
        }
    }

    fn fill_in_new_columns(&mut self) {
        for cmod in &self.cmods {
            if !cmod.rem && cmod.add {
                for m in &self.mods {
                    if m.patch_row != -1 && m.dest_row != -1 {
                        let d = self.patch.get(cmod.patch_row as usize, m.patch_row as usize).map(String::from);
                        self.source.set(cmod.dest_row as usize, m.dest_row as usize, d);
                    }
                }
                let hdr = self.header.get(&(cmod.patch_row as usize)).cloned();
                self.source.set(cmod.dest_row as usize, 0, hdr);
            }
        }
    }

    fn fate_to_table(fate: &[isize]) -> Vec<Option<usize>> {
        fate.iter().map(|&f| usize::try_from(f).ok()).collect()
    }

    fn finish_rows(&mut self) {
        if self.source.width() == 0 {
            if self.source.height() != 0 {
                self.source.resize(0, 0);
            }
            return;
        }
        let mut fate: Vec<isize> = Vec::new();
        let mut row_permutation: Vec<isize> = Vec::new();
        let mut row_permutation_rev: Vec<usize> = Vec::new();
        Self::compute_ordering(&self.mods, &mut row_permutation, &mut row_permutation_rev, self.source.height());
        if !row_permutation.is_empty() {
            for m in &mut self.mods {
                if m.source_row >= 0 {
                    m.source_row = row_permutation[m.source_row as usize];
                }
            }
            let f = Self::fate_to_table(&row_permutation);
            self.source.insert_or_delete_rows(&f, row_permutation.len());
        }
        let len = Self::process_mods(&mut self.mods, &mut fate, self.source.height());
        self.source.insert_or_delete_rows(&Self::fate_to_table(&fate), len as usize);
        self.need_dest_columns();
        let pidc = self.patch_in_dest_col.clone().unwrap();
        let mods = self.mods.clone();
        for m in &mods {
            if m.rem {
                continue;
            }
            if m.add {
                for &c in self.header_post.values() {
                    if let Some(&offset) = pidc.get(&c) {
                        if offset >= 0 {
                            let d = self.patch.get(c, m.patch_row as usize).map(String::from);
                            self.source.set(offset as usize, m.dest_row as usize, d);
                        }
                    }
                }
            } else if m.update {
                self.current_row = m.patch_row;
                self.check_act();
                if !self.row_info.updated {
                    continue;
                }
                let vrow = self.row_info.value.clone().unwrap_or_default();
                for &c in self.header_pre.values() {
                    let txt = self.patch.get(c, m.patch_row as usize).unwrap_or("").to_string();
                    let cell = examine_cell(0, 0, Some(&txt), "", &vrow, 0);
                    if !cell.updated || cell.conflicted {
                        continue;
                    }
                    let d = Csv::parse_cell(cell.rvalue.as_deref());
                    if let Some(&offset) = pidc.get(&c) {
                        if offset >= 0 {
                            self.source.set(offset as usize, m.dest_row as usize, d);
                        }
                    }
                }
            }
        }
        self.fill_in_new_columns();
        for i in 0..self.source.width() {
            let name = self.source.get(i, 0).unwrap_or("").to_string();
            if let Some(next) = self.header_rename.get(&name).cloned() {
                self.source.set(i, 0, Some(next));
            }
        }
    }

    fn finish_columns(&mut self) {
        if self.finished_columns {
            return;
        }
        self.finished_columns = true;
        self.need_source_columns();
        for i in self.payload_col..self.payload_top {
            let act = self.modifier.get(&i).cloned().unwrap_or_default();
            let at = self.patch_in_source_col.get(&i).map_or(-1, |&v| v as isize);
            if act == "---" {
                let mut m = PatchUnit::new();
                m.code = act;
                m.rem = true;
                m.source_row = at;
                m.patch_row = i as isize;
                self.cmods.push(m);
            } else if act == "+++" {
                let mut m = PatchUnit::new();
                m.code = act;
                m.add = true;
                if let Some(last) = self.cmods.last() {
                    m.source_row = last.source_row;
                }
                if m.source_row != -1 {
                    m.source_row_offset = 1;
                }
                m.patch_row = i as isize;
                self.cmods.push(m);
            } else if act != "..." {
                let mut m = PatchUnit::new();
                m.code = act;
                m.patch_row = i as isize;
                m.source_row = at;
                self.cmods.push(m);
            }
        }
        let (mut at, mut rat) = (-1isize, -1isize);
        let n = self.cmods.len();
        for i in 0..n.saturating_sub(1) {
            let icode = self.cmods[i].code.as_str();
            if icode != "+++" && icode != "---" {
                at = self.cmods[i].source_row;
            }
            self.cmods[i + 1].source_prev_row = at;
            let j = n - 1 - i;
            let jcode = self.cmods[j].code.as_str();
            if jcode != "+++" && jcode != "---" {
                rat = self.cmods[j].source_row;
            }
            self.cmods[j - 1].source_next_row = rat;
        }
        let mut fate: Vec<isize> = Vec::new();
        if self.header_move.is_some() {
            let mut perm: Vec<isize> = Vec::new();
            let mut perm_rev: Vec<usize> = Vec::new();
            Self::compute_ordering(&self.cmods, &mut perm, &mut perm_rev, self.source.width());
            if !perm.is_empty() {
                for m in &mut self.cmods {
                    if m.source_row >= 0 {
                        m.source_row = perm[m.source_row as usize];
                    }
                }
                self.source.insert_or_delete_columns(&Self::fate_to_table(&perm), perm.len());
            }
        }
        let len = Self::process_mods(&mut self.cmods, &mut fate, self.source.width());
        self.source.insert_or_delete_columns(&Self::fate_to_table(&fate), len as usize);
    }
}
