//! Apply a highlighter-format diff to a table, ported from
//! `coopy.HighlightPatch` (meta tables, SQL sources and metadata rows are not
//! supported; metadata rows are skipped as in the reference).
use crate::align::{compare_tables, IndexPair, Row};
use crate::cell_info::{examine_cell, CellInfo};
use crate::format::csv::Csv;
use crate::unit::stable_sort_by;
use crate::{Result, Table};
use std::collections::HashMap;
use std::rc::Rc;

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

/// Something in the diff that does not fit the table it is applied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchIssue {
    /// Row of the diff (0-based).
    pub patch_row: usize,
    pub kind: PatchIssueKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchIssueKind {
    /// The cell does not have the value the diff expects (and not the new one).
    CellChanged,
    /// A row to delete differs from the row the diff shows.
    RowChanged,
    /// A row to update does not exist.
    RowNotFound,
    /// The diff itself records a conflict.
    Conflict,
    /// There is no `@@` header row, so nothing could be applied.
    NoHeader,
    /// A changed cell is in a column the table does not have (or the diff elides).
    ColumnNotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Context,
    Moved,
    Plus,
    Update,
    Add,
    Delete,
    Gap,
}

/// One row of the patch, with the source row it was matched to.
#[derive(Debug, Clone)]
struct Item {
    patch_row: isize,
    kind: Kind,
    source_row: isize,
    dest_row: isize,
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
    seen_header: bool,
    header_patch_row: usize,
    skip_first_row: bool,
    // Row bookkeeping of the fixed algorithm (see `finish_rows_by_items`).
    items: Vec<Item>,
    orig: Option<Rc<Table>>,
    force: bool,
    issues: Vec<PatchIssue>,
    used_rows: std::collections::HashSet<usize>,
    last_matched: isize,
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
            seen_header: false,
            header_patch_row: 0,
            skip_first_row: false,
            items: Vec::new(),
            orig: None,
            force: false,
            issues: Vec::new(),
            used_rows: Default::default(),
            last_matched: -1,
        }
    }

    /// Apply the diff even where it does not match the table: values are
    /// overwritten and rows deleted whatever they contain, as the reference does.
    pub fn force(&mut self, force: bool) -> &mut Self {
        self.force = force;
        self
    }

    /// Problems found by the last [`apply`](Self::apply). The parts of the diff
    /// that fit were applied; the rest was left alone.
    pub fn issues(&self) -> &[PatchIssue] {
        &self.issues
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
        self.seen_header = false;
        self.skip_first_row = false;
        self.items.clear();
        self.issues.clear();
        self.orig = None;
        self.used_rows.clear();
        self.last_matched = -1;

        let patch = self.patch;
        if patch.width() < 2 || patch.height() < 1 {
            return Ok(true);
        }
        // Computed before `rc_offset` is known, as in the reference: the
        // payload always starts at column 1.
        self.payload_col = 1 + self.rc_offset;
        self.payload_top = patch.width();
        self.rc_offset = usize::from(patch.get(0, 0).unwrap_or("") == "@:@");
        self.skip_first_row = self.rc_offset > 0;
        if !crate::compat::reference_compat() {
            // The action column is the one holding `@@`, wherever it is: the
            // spec asks readers to cope with extra rows and columns before it.
            // With an index column the payload starts one column later; the
            // reference treats the action column as data (quirk 17).
            self.skip_first_row = patch.get(0, 0) == Some("@:@");
            'sniff: for r in 0..patch.height().min(10) {
                for c in 0..patch.width().min(5) {
                    if patch.get(c, r) == Some("@@") {
                        self.rc_offset = c;
                        break 'sniff;
                    }
                }
            }
            self.payload_col = 1 + self.rc_offset;
        }
        for r in 0..patch.height() {
            self.actions.push(patch.get(self.rc_offset, r).unwrap_or("").to_string());
        }
        self.preamble_row = self.rc_offset as isize;
        for r in 0..patch.height() {
            self.apply_row(r as isize);
        }
        if !self.seen_header && !crate::compat::reference_compat() {
            self.issues.push(PatchIssue {
                patch_row: 0,
                kind: PatchIssueKind::NoHeader,
                message: "the diff has no @@ header row; nothing was applied".into(),
            });
            return Ok(true);
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
        self.orig = Some(Rc::new(self.source.clone()));
        let (_, idx) = compare_tables(self.source, self.source, None).align_storing_indexes();
        self.indexes = Some(idx);
        self.need_source_columns();
    }

    fn apply_row(&mut self, r: isize) {
        self.current_row = r;
        let mut code = self.actions[r as usize].clone();
        let mut done = false;
        if r == 0 && self.skip_first_row {
            done = true;
        } else if code == "@@" {
            self.seen_header = true;
            self.header_patch_row = r as usize;
            self.preamble_row = r;
            self.apply_header();
            if crate::compat::reference_compat() {
                self.apply_action("@@");
            } else {
                // index the source before the columns are changed
                self.need_source_index();
            }
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
        } else if !self.seen_header && !crate::compat::reference_compat() {
            // rows before the header are not part of the diff
            done = true;
        }
        if !done && !crate::compat::reference_compat() {
            self.finish_columns();
            let kind = match code.as_str() {
                "+++" => Some(Kind::Add),
                "---" => Some(Kind::Delete),
                "+" => Some(Kind::Plus),
                ":" => Some(Kind::Moved),
                "..." => Some(Kind::Gap),
                "" => Some(Kind::Context),
                c if c.contains("->") => Some(Kind::Update),
                _ => None,
            };
            if let Some(kind) = kind {
                self.push_item(kind);
            }
        } else if !done {
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
            if !self.force && !crate::compat::reference_compat() && self.source.height() > 0 {
                // Column changes the table already has are not made twice.
                let have = |n: &str| (0..self.source.width()).any(|x| self.source.get(x, 0) == Some(n));
                let bare = mod_.as_deref().map(|m| m.trim_start_matches(':'));
                match bare {
                    Some("+++") if have(&name) => {
                        mod_ = None;
                        self.modifier.remove(&i);
                    }
                    Some("---") if !have(&name) => {
                        self.modifier.insert(i, "...".into());
                        self.header.insert(i, name.clone());
                        continue;
                    }
                    Some(m) if m.starts_with('(') => {
                        let old: String = m.chars().skip(1).take(m.chars().count().saturating_sub(2)).collect();
                        if !have(&old) && have(&name) {
                            mod_ = None;
                            self.modifier.remove(&i);
                        }
                    }
                    _ => {}
                }
            }
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
                    self.header_rename.insert(prev_name, name.clone());
                    if mv && !crate::compat::reference_compat() {
                        // a moved *and* renamed column is still a moved column (B-27)
                        self.header_move.get_or_insert_with(HashMap::new).insert(name, true);
                    }
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
            if crate::compat::reference_compat() {
                self.apply_action("+++");
            } else {
                // an empty table: the header row itself is inserted
                self.push_item(Kind::Add);
            }
        }
    }

    fn push_item(&mut self, kind: Kind) {
        let source_row = match kind {
            Kind::Context | Kind::Moved | Kind::Plus | Kind::Update | Kind::Delete => self.match_row(kind),
            Kind::Add | Kind::Gap => -1,
        };
        self.items.push(Item { patch_row: self.current_row, kind, source_row, dest_row: -1 });
    }

    /// Find the source row a patch row describes, by content. Rows that were
    /// already matched are not offered again, so duplicate rows are matched
    /// one by one in order (B-02).
    fn match_row(&mut self, kind: Kind) -> isize {
        self.need_source_index();
        let indexes = self.indexes.take().unwrap_or_default();
        let mut result: isize = -1;
        let mut ambiguous: Vec<usize> = Vec::new();
        for idx in &indexes {
            let m = idx.query_by_content(&PatchRow(self));
            if m.spot_a == 0 {
                continue;
            }
            let unused: Vec<usize> = m.item_a.iter().copied().filter(|r| !self.used_rows.contains(r)).collect();
            if m.spot_a == 1 {
                if let Some(&r) = unused.first() {
                    result = r as isize;
                    break;
                }
            } else if !unused.is_empty() {
                ambiguous = unused;
            }
        }
        if result < 0 && !ambiguous.is_empty() {
            let after = ambiguous.iter().copied().find(|&r| r as isize > self.last_matched);
            result = after.unwrap_or(ambiguous[0]) as isize;
        }
        self.indexes = Some(indexes);
        if result >= 0 {
            self.used_rows.insert(result as usize);
            if kind != Kind::Moved {
                self.last_matched = result;
            }
        }
        result
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
                        let d = self.patch.get_cell(cmod.patch_row as usize, m.patch_row as usize).cloned();
                        self.source.set_cell(cmod.dest_row as usize, m.dest_row as usize, d);
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

    /// Apply the row changes. Rows of the source are kept in their order
    /// except where the patch moves them; rows the patch does not mention
    /// (omitted as `...`) stay with their neighbours. Rewritten from the
    /// reference algorithm, which mishandled duplicate rows and moves (B-02,
    /// B-03, B-12).
    fn finish_rows_by_items(&mut self) {
        if self.source.width() == 0 {
            if self.source.height() != 0 {
                self.source.resize(0, 0);
            }
            return;
        }
        let n = self.source.height();
        let mut items = self.items.clone();
        let skipped = if self.force { Default::default() } else { self.check_preconditions(&mut items) };
        // Rows that anchor the omitted rows: matched, not moved, not deleted (the
        // position of a deleted row in the diff is unreliable once rows move).
        let anchors: Vec<(usize, usize)> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.source_row >= 0 && !matches!(it.kind, Kind::Moved | Kind::Delete))
            .map(|(i, it)| (i, it.source_row as usize))
            .collect();
        let mut by_source = anchors.clone();
        by_source.sort_by_key(|&(_, s)| s);
        let mut anchor_items: Vec<usize> = anchors.iter().map(|&(i, _)| i).collect();
        anchor_items.sort_unstable();
        let mut gap_rows: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut after: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut lead: Vec<usize> = Vec::new();
        for u in 1..n {
            if self.used_rows.contains(&u) {
                continue;
            }
            let pos = by_source.partition_point(|&(_, s)| s < u);
            let k = pos.checked_sub(1).map(|p| by_source[p].0);
            let lo = k.map_or(0, |k| k + 1);
            let hi = anchor_items.get(anchor_items.partition_point(|&i| i < lo)).copied().unwrap_or(items.len());
            let gap = (lo..hi).find(|&i| items[i].kind == Kind::Gap);
            match (gap, k) {
                (Some(g), _) => gap_rows.entry(g).or_default().push(u),
                (None, None) => lead.push(u),
                (None, Some(k)) => after.entry(if hi > lo { hi - 1 } else { k }).or_default().push(u),
            }
        }
        enum Out {
            Src(usize),
            Item(usize),
        }
        let mut out: Vec<Out> = lead.into_iter().map(Out::Src).collect();
        for (i, it) in items.iter().enumerate() {
            match it.kind {
                Kind::Add => out.push(Out::Item(i)),
                Kind::Context | Kind::Moved | Kind::Plus | Kind::Update => {
                    if it.source_row >= 0 {
                        out.push(Out::Src(it.source_row as usize));
                    }
                }
                Kind::Delete => {}
                Kind::Gap => out.extend(gap_rows.remove(&i).unwrap_or_default().into_iter().map(Out::Src)),
            }
            out.extend(after.remove(&i).unwrap_or_default().into_iter().map(Out::Src));
        }
        let mut fate: Vec<isize> = vec![-1; n];
        let mut pos: isize = if n == 0 { 0 } else { 1 };
        if n > 0 {
            fate[0] = 0;
        }
        let item_of_source: HashMap<usize, usize> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.source_row >= 0 && it.kind != Kind::Delete)
            .map(|(i, it)| (it.source_row as usize, i))
            .collect();
        for o in &out {
            match o {
                Out::Src(s) => {
                    fate[*s] = pos;
                    if let Some(&i) = item_of_source.get(s) {
                        items[i].dest_row = pos;
                    }
                }
                Out::Item(i) => items[*i].dest_row = pos,
            }
            pos += 1;
        }
        self.source.insert_or_delete_rows(&Self::fate_to_table(&fate), pos as usize);
        self.need_dest_columns();
        let pidc = self.patch_in_dest_col.clone().unwrap();
        for it in &items {
            if it.dest_row < 0 {
                continue;
            }
            match it.kind {
                Kind::Add => {
                    for &c in self.header_post.values() {
                        if let Some(&offset) = pidc.get(&c) {
                            if offset >= 0 {
                                let d = self.patch.get_cell(c, it.patch_row as usize).cloned();
                                self.source.set_cell(offset as usize, it.dest_row as usize, d);
                            }
                        }
                    }
                }
                Kind::Update => {
                    self.current_row = it.patch_row;
                    self.check_act();
                    if !self.row_info.updated {
                        continue;
                    }
                    let vrow = self.row_info.value.clone().unwrap_or_default();
                    for &c in self.header_pre.values() {
                        if skipped.contains(&(it.patch_row, c)) {
                            continue;
                        }
                        let txt = self.patch.get(c, it.patch_row as usize).unwrap_or("").to_string();
                        let cell = examine_cell(0, 0, Some(&txt), "", &vrow, 0);
                        if !cell.updated || cell.conflicted {
                            continue;
                        }
                        let d = Csv::parse_cell(cell.rvalue.as_deref());
                        if let Some(&offset) = pidc.get(&c) {
                            if offset >= 0 {
                                self.source.set(offset as usize, it.dest_row as usize, d);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // New columns: values come from the rows the patch shows; blank in the
        // patch means the blank string, and so does a row it does not show.
        self.mods = items
            .iter()
            .filter(|it| it.dest_row >= 0 && it.kind != Kind::Gap)
            .map(|it| {
                let mut m = PatchUnit::new();
                m.patch_row = it.patch_row;
                m.dest_row = it.dest_row;
                m
            })
            .collect();
        self.fill_in_new_columns();
        let covered: std::collections::HashSet<isize> = self.mods.iter().map(|m| m.dest_row).collect();
        for cmod in &self.cmods {
            if cmod.add && !cmod.rem {
                for y in 1..self.source.height() {
                    if !covered.contains(&(y as isize)) && self.source.get_cell(cmod.dest_row as usize, y).is_none() {
                        self.source.set(cmod.dest_row as usize, y, Some(""));
                    }
                }
            }
        }
        for i in 0..self.source.width() {
            let name = self.source.get(i, 0).unwrap_or("").to_string();
            if let Some(next) = self.header_rename.get(&name).cloned() {
                self.source.set(i, 0, Some(next));
            }
        }
    }

    /// Check the diff against the table before applying it. Cells that
    /// already have their new value are skipped, cells and rows that differ
    /// from what the diff expects are left alone and reported, a row that is
    /// already present is not added again. Returns the (diff row, diff column)
    /// pairs of cells not to update.
    fn check_preconditions(&mut self, items: &mut [Item]) -> std::collections::HashSet<(isize, usize)> {
        let mut skipped = std::collections::HashSet::new();
        let Some(orig) = self.orig.clone() else { return skipped };
        let same = |a: Option<&str>, b: Option<&str>| a.unwrap_or("") == b.unwrap_or("");
        let show = |patch_row: isize| -> String {
            (0..self.patch.width())
                .filter_map(|c| self.patch.get(c, patch_row as usize))
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut issues: Vec<PatchIssue> = Vec::new();
        let mut issue = |it: &Item, kind, message: String| {
            issues.push(PatchIssue { patch_row: it.patch_row as usize, kind, message });
        };
        let column_changes = self.cmods.iter().any(|c| c.add || c.rem);
        let pre: Vec<usize> = self.header_pre.values().copied().collect();
        let post: Vec<usize> = self.header_post.values().copied().collect();
        // Signatures of rows nobody claimed, for spotting rows that are already there.
        let mut present: Option<HashMap<Vec<String>, Vec<usize>>> = None;
        for it in items.iter_mut() {
            match it.kind {
                Kind::Delete if it.source_row >= 0 => {
                    let s = it.source_row as usize;
                    let differs = pre.iter().any(|&c| {
                        self.patch_in_source_col.get(&c).is_some_and(|&sc| {
                            !same(orig.get(sc, s), self.patch.get(c, it.patch_row as usize))
                        })
                    });
                    if differs {
                        issue(it, PatchIssueKind::RowChanged, format!("not deleting row `{}`: it has changed since the diff was made", show(it.patch_row)));
                        it.kind = Kind::Context; // keep the row
                    }
                }
                Kind::Update if it.source_row < 0 => {
                    // Maybe the update was already made: look for a row with the new values.
                    self.current_row = it.patch_row;
                    self.check_act();
                    let vrow = self.row_info.value.clone().unwrap_or_default();
                    let wanted: Vec<(usize, Option<String>)> = pre
                        .iter()
                        .filter_map(|&c| {
                            let sc = *self.patch_in_source_col.get(&c)?;
                            let txt = self.patch.get(c, it.patch_row as usize).unwrap_or("").to_string();
                            let cell = examine_cell(0, 0, Some(&txt), "", &vrow, 0);
                            let v = if cell.updated && !cell.conflicted {
                                Csv::parse_cell(cell.rvalue.as_deref())
                            } else {
                                self.patch.get(c, it.patch_row as usize).map(String::from)
                            };
                            Some((sc, v))
                        })
                        .collect();
                    let found = (1..orig.height()).filter(|_| !wanted.is_empty()).find(|&r| {
                        !self.used_rows.contains(&r)
                            && wanted.iter().all(|(sc, v)| same(orig.get(*sc, r), v.as_deref()))
                    });
                    match found {
                        Some(r) => {
                            self.used_rows.insert(r);
                            it.kind = Kind::Context;
                            it.source_row = r as isize;
                        }
                        None => issue(it, PatchIssueKind::RowNotFound, format!("cannot update: no row matches `{}`", show(it.patch_row))),
                    }
                }
                Kind::Update => {
                    self.current_row = it.patch_row;
                    self.check_act();
                    if !self.row_info.updated {
                        continue;
                    }
                    let vrow = self.row_info.value.clone().unwrap_or_default();
                    for &c in &pre {
                        let txt = self.patch.get(c, it.patch_row as usize).unwrap_or("").to_string();
                        let cell = examine_cell(0, 0, Some(&txt), "", &vrow, 0);
                        if !cell.updated {
                            continue;
                        }
                        if cell.conflicted {
                            skipped.insert((it.patch_row, c));
                            issue(it, PatchIssueKind::Conflict, format!("the diff records a conflict in `{txt}`; left as is"));
                            continue;
                        }
                        let Some(&sc) = self.patch_in_source_col.get(&c) else { continue };
                        let current = orig.get(sc, it.source_row as usize);
                        let old = Csv::parse_cell(cell.lvalue.as_deref());
                        let new = Csv::parse_cell(cell.rvalue.as_deref());
                        if same(current, new.as_deref()) {
                            skipped.insert((it.patch_row, c)); // already applied
                        } else if !same(current, old.as_deref()) {
                            skipped.insert((it.patch_row, c));
                            issue(
                                it,
                                PatchIssueKind::CellChanged,
                                format!(
                                    "not changing `{}` to `{}`: it is `{}`, the diff expects `{}`",
                                    old.as_deref().unwrap_or("NULL"),
                                    new.as_deref().unwrap_or("NULL"),
                                    current.unwrap_or("NULL"),
                                    old.as_deref().unwrap_or("NULL")
                                ),
                            );
                        }
                    }
                }
                Kind::Add if !column_changes && it.patch_row > self.preamble_row => {
                    let n = orig.height();
                    let present = present.get_or_insert_with(|| {
                        let mut m: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
                        for r in 1..n {
                            if self.used_rows.contains(&r) {
                                continue;
                            }
                            let sig = post
                                .iter()
                                .filter_map(|c| self.patch_in_source_col.get(c))
                                .map(|&sc| orig.get(sc, r).unwrap_or("").to_string())
                                .collect();
                            m.entry(sig).or_default().push(r);
                        }
                        m
                    });
                    let sig: Vec<String> = post
                        .iter()
                        .filter(|c| self.patch_in_source_col.contains_key(c))
                        .map(|&c| self.patch.get(c, it.patch_row as usize).unwrap_or("").to_string())
                        .collect();
                    if let Some(rows) = present.get_mut(&sig) {
                        if !rows.is_empty() {
                            // already there: use that row instead of adding another
                            let r = rows.remove(0);
                            self.used_rows.insert(r);
                            it.kind = Kind::Context;
                            it.source_row = r as isize;
                        }
                    }
                }
                _ => {}
            }
        }
        // Changes in columns that cannot be located (elided as `...`, or not in
        // the header) would otherwise be dropped without a word.
        for it in items.iter() {
            if it.kind != Kind::Update {
                continue;
            }
            self.current_row = it.patch_row;
            self.check_act();
            if !self.row_info.updated {
                continue;
            }
            let vrow = self.row_info.value.clone().unwrap_or_default();
            for c in self.payload_col..self.payload_top {
                if pre.contains(&c) {
                    continue;
                }
                let txt = self.patch.get(c, it.patch_row as usize).unwrap_or("").to_string();
                if txt == "..." || !examine_cell(0, 0, Some(&txt), "", &vrow, 0).updated {
                    continue;
                }
                let name = self.patch.get(c, self.header_patch_row).unwrap_or("").to_string();
                issues.push(PatchIssue {
                    patch_row: it.patch_row as usize,
                    kind: PatchIssueKind::ColumnNotFound,
                    message: format!("cannot apply `{txt}`: column `{name}` cannot be located in the table"),
                });
            }
        }
        self.issues.extend(issues);
        skipped
    }

    fn finish_rows(&mut self) {
        if !crate::compat::reference_compat() {
            return self.finish_rows_by_items();
        }
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
        self.source.insert_or_delete_rows(&Self::fate_to_table(&fate), len.max(0) as usize);
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
                            let d = self.patch.get_cell(c, m.patch_row as usize).cloned();
                            self.source.set_cell(offset as usize, m.dest_row as usize, d);
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
        self.source.insert_or_delete_columns(&Self::fate_to_table(&fate), len.max(0) as usize);
    }
}
