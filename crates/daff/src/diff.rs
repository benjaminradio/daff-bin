//! Highlighter-format diffs, ported from `coopy.TableDiff` (with the flat cell
//! builder). Meta tables, nested tables and SQL sources are not supported.
use crate::mover::move_units;
use crate::unit::Unit;
use crate::value::{Cell, Value};
use crate::{Alignment, CompareTable, Error, Flags, Result, Table, Tables};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Counts of the kinds of change found by a diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub row_deletes: usize,
    pub row_inserts: usize,
    pub row_updates: usize,
    pub row_reorders: usize,
    pub col_deletes: usize,
    pub col_inserts: usize,
    pub col_updates: usize,
    pub col_renames: usize,
    pub col_reorders: usize,
    pub row_count_initial_with_header: usize,
    pub row_count_final_with_header: usize,
    pub row_count_initial: isize,
    pub row_count_final: isize,
    pub col_count_initial: usize,
    pub col_count_final: usize,
    pub different: bool,
}

/// Produces a highlighter-format diff table from an [`Alignment`].
pub struct TableDiff<'a> {
    align: &'a Alignment,
    flags: Flags,

    row_map: HashMap<usize, Unit>,
    col_map: HashMap<usize, Unit>,
    has_parent: bool,
    a: Rc<Table>,
    b: Rc<Table>,
    p: Rc<Table>,
    rp_header: isize,
    ra_header: isize,
    rb_header: isize,
    is_index_p: HashSet<isize>,
    is_index_a: HashSet<isize>,
    is_index_b: HashSet<isize>,
    row_units: Vec<Unit>,
    column_units: Vec<Unit>,
    show_rc_numbers: bool,
    row_moves: HashSet<usize>,
    col_moves: HashSet<usize>,
    active_row: Vec<i32>,
    active_column: Option<Vec<i32>>,
    allow_insert: bool,
    allow_delete: bool,
    allow_update: bool,
    allow_column: bool,
    nested: bool,
    nesting_present: bool,
    error: Option<String>,
    builder: Builder,
    sep: String,
    conflict_sep: String,
    schema: Vec<String>,
    have_schema: bool,
    top_line_done: bool,

    // Per-row state
    have_addition: bool,
    act: String,
    publish: bool,
    diff_found: bool,
    schema_diff_found: bool,
    summary: DiffSummary,
    column_units_updated: HashSet<usize>,
}

fn separator_for(t: &Table, t2: &Table, root: &str) -> String {
    let mut sep = root.to_string();
    for t in [t, t2] {
        for y in 0..t.height() {
            for x in 0..t.width() {
                if let Some(txt) = t.get(x, y) {
                    while txt.contains(&sep) {
                        sep.insert(0, '-');
                    }
                }
            }
        }
    }
    sep
}

fn is_reordered(m: &HashMap<usize, Unit>, ct: usize) -> bool {
    let (mut l, mut r) = (-1isize, -1isize);
    for i in 0..ct {
        let Some(unit) = m.get(&i) else { continue };
        if unit.l >= 0 {
            if unit.l < l {
                return true;
            }
            l = unit.l;
        }
        if unit.r >= 0 {
            if unit.r < r {
                return true;
            }
            r = unit.r;
        }
    }
    false
}

fn spread_context(units: &[Unit], del: usize, active: Option<&mut Vec<i32>>) {
    let Some(active) = active else { return };
    if del == 0 {
        return;
    }
    let del = del as isize;
    let n = units.len() as isize;
    let mut mark = -del - 1;
    let mut skips = 0isize;
    // With `--all-rows` the row activity is empty, as in the reference where
    // out-of-range reads yield null and are never matched.
    let at = |active: &Vec<i32>, i: isize| active.get(i as usize).copied();
    for i in 0..n {
        let Some(a) = at(active, i) else { continue };
        if a == -3 {
            skips += 1;
            continue;
        }
        if a == 0 || a == 3 {
            if i - mark <= del + skips {
                active[i as usize] = 2;
            } else if i - mark == del + 1 + skips {
                active[i as usize] = 3;
            }
        } else if a == 1 {
            mark = i;
            skips = 0;
        }
    }
    mark = n + del + 1;
    skips = 0;
    for j in 0..n {
        let i = n - 1 - j;
        let Some(a) = at(active, i) else { continue };
        if a == -3 {
            skips += 1;
            continue;
        }
        if a == 0 || a == 3 {
            if mark - i <= del + skips {
                active[i as usize] = 2;
            } else if mark - i == del + 1 + skips {
                active[i as usize] = 3;
            }
        } else if a == 1 {
            mark = i;
            skips = 0;
        }
    }
}

fn count_active(active: &[i32]) -> usize {
    let mut ct = 0;
    let mut showed_dummy = false;
    for &a in active {
        let publish = a > 0;
        let dummy = a == 3;
        if dummy && showed_dummy {
            continue;
        }
        if !publish {
            continue;
        }
        showed_dummy = dummy;
        ct += 1;
    }
    ct
}

/// JavaScript-style `parseFloat`: parses the longest numeric prefix.
fn parse_float(s: &str) -> Option<f64> {
    let s = s.trim_start();
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut had_digits = i > digits_start;
    if i < b.len() && b[i] == b'.' {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > frac_start || had_digits {
            had_digits |= j > frac_start;
            i = j;
        }
    }
    if !had_digits {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    s[..i].parse().ok()
}

/// How update, conflict and marker cells are built (`coopy.CellBuilder`):
/// flat text (`a->b`) for tabular output, or nested hashes for JSON.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Builder {
    Flat,
    Nested,
}

fn neg_to_null(x: isize) -> Value {
    if x < 0 { Value::Null } else { Value::Num(x as f64) }
}

fn value_of(c: Option<&Cell>) -> Value {
    c.map_or(Value::Null, Cell::to_value)
}

/// `FlatCellBuilder.quoteForDiff`.
fn quote_for_diff(d: Option<&Cell>) -> String {
    let Some(s) = d.map(Cell::text) else { return "NULL".into() };
    let score = s.chars().take_while(|&c| c == '_').count();
    if s.chars().skip(score).collect::<String>() == "NULL" {
        format!("_{s}")
    } else {
        s.to_string()
    }
}

impl<'a> TableDiff<'a> {
    pub fn new(align: &'a Alignment, flags: Flags) -> Self {
        let empty = Rc::new(Table::default());
        TableDiff {
            align,
            flags,
            row_map: HashMap::new(),
            col_map: HashMap::new(),
            has_parent: false,
            a: empty.clone(),
            b: empty.clone(),
            p: empty,
            rp_header: 0,
            ra_header: 0,
            rb_header: 0,
            is_index_p: HashSet::new(),
            is_index_a: HashSet::new(),
            is_index_b: HashSet::new(),
            row_units: Vec::new(),
            column_units: Vec::new(),
            show_rc_numbers: false,
            row_moves: HashSet::new(),
            col_moves: HashSet::new(),
            active_row: Vec::new(),
            active_column: None,
            allow_insert: true,
            allow_delete: true,
            allow_update: true,
            allow_column: true,
            nested: false,
            nesting_present: false,
            error: None,
            builder: Builder::Flat,
            sep: String::new(),
            conflict_sep: String::new(),
            schema: Vec::new(),
            have_schema: false,
            top_line_done: false,
            have_addition: false,
            act: String::new(),
            publish: false,
            diff_found: false,
            schema_diff_found: false,
            summary: DiffSummary::default(),
            column_units_updated: HashSet::new(),
        }
    }

    /// Diff, including the diffs of any tables nested in cells
    /// (`hiliteWithNesting`): one output table per nested table.
    pub fn hilite_with_nesting(&mut self) -> Result<Tables> {
        let mut out = Tables::new();
        let ok = {
            let base = out.add("base");
            self.hilite(base)
        };
        if let Some(e) = self.error.take() {
            return Err(Error::Nested(e));
        }
        if !ok {
            return Ok(out);
        }
        if let Some(comp) = self.align.comp.clone() {
            let (order, children) = {
                let st = comp.borrow();
                (st.children.is_some().then(|| st.child_order.clone()), st.children.clone().unwrap_or_default())
            };
            let Some(order) = order else { return Ok(out) };
            out.set_ins_del(self.align.has_addition || self.align.has_removal);
            for name in order {
                let Some(Some(alignment)) = children.get(&name) else { continue };
                if alignment.is_marked_as_identical() {
                    continue;
                }
                let mut td = TableDiff::new(alignment, self.flags.clone());
                let child_out = out.add(&name);
                td.hilite(child_out);
                if let Some(e) = td.error.take() {
                    return Err(Error::Nested(e));
                }
            }
        }
        Ok(out)
    }

    pub fn has_difference(&self) -> bool {
        self.diff_found
    }

    pub fn has_schema_difference(&self) -> bool {
        self.schema_diff_found
    }

    /// Run the diff into a fresh table.
    pub fn highlight(&mut self) -> Table {
        let mut out = Table::new(0, 0);
        self.hilite(&mut out);
        out
    }

    pub fn summary(&self) -> DiffSummary {
        let mut ds = self.summary.clone();
        let src = self.align.source().map(|t| (t.height(), t.width())).unwrap_or((0, 0));
        let dst = self.align.target().map(|t| (t.height(), t.width())).unwrap_or((0, 0));
        ds.row_count_initial_with_header = src.0;
        ds.row_count_final_with_header = dst.0;
        ds.row_count_initial = src.0 as isize - self.align.source_header() - 1;
        ds.row_count_final = dst.0 as isize - self.align.target_header() - 1;
        ds.col_count_initial = src.1;
        ds.col_count_final = dst.1;
        ds.different = ds.row_deletes
            + ds.row_inserts
            + ds.row_updates
            + ds.row_reorders
            + ds.col_deletes
            + ds.col_inserts
            + ds.col_updates
            + ds.col_renames
            + ds.col_reorders
            > 0;
        ds
    }

    fn reset(&mut self) {
        self.has_parent = false;
        self.rp_header = 0;
        self.ra_header = 0;
        self.rb_header = 0;
        self.is_index_p.clear();
        self.is_index_a.clear();
        self.is_index_b.clear();
        self.row_map.clear();
        self.col_map.clear();
        self.show_rc_numbers = false;
        self.row_moves.clear();
        self.col_moves.clear();
        self.allow_insert = true;
        self.allow_delete = true;
        self.allow_update = true;
        self.allow_column = true;
        self.sep.clear();
        self.conflict_sep.clear();
        self.top_line_done = false;
        self.diff_found = false;
        self.schema_diff_found = false;
        self.summary = DiffSummary::default();
        self.column_units_updated.clear();
    }

    fn setup_tables(&mut self) {
        let align = self.align;
        let order = align.to_order();
        self.row_units = order.list().to_vec();
        self.has_parent = align.reference.is_some();
        let meta = align.meta.as_ref().expect("alignment has no column alignment");
        if let Some(reference) = &align.reference {
            self.p = align.source().unwrap().clone();
            self.a = reference.target().unwrap().clone();
            self.b = align.target().unwrap().clone();
            let rmeta = reference.meta.as_ref().unwrap();
            self.rp_header = rmeta.source_header();
            self.ra_header = rmeta.target_header();
            self.rb_header = meta.target_header();
            for u in align.index_columns().unwrap_or(&[]) {
                if u.l >= 0 {
                    self.is_index_p.insert(u.l);
                }
                if u.r >= 0 {
                    self.is_index_b.insert(u.r);
                }
            }
            for u in reference.index_columns().unwrap_or(&[]) {
                if u.l >= 0 {
                    self.is_index_p.insert(u.l);
                }
                if u.r >= 0 {
                    self.is_index_a.insert(u.r);
                }
            }
        } else {
            self.a = align.source().unwrap().clone();
            self.b = align.target().unwrap().clone();
            self.p = self.a.clone();
            self.ra_header = meta.source_header();
            self.rp_header = self.ra_header;
            self.rb_header = meta.target_header();
            for u in align.index_columns().unwrap_or(&[]) {
                if u.l >= 0 {
                    self.is_index_a.insert(u.l);
                }
                if u.r >= 0 {
                    self.is_index_b.insert(u.r);
                }
            }
        }
        self.allow_insert = self.flags.allow_insert();
        self.allow_delete = self.flags.allow_delete();
        self.allow_update = self.flags.allow_update();
        self.allow_column = self.flags.allow_column();
        self.builder = if self.flags.allow_nested_cells { Builder::Nested } else { Builder::Flat };
        let common = &self.a;
        self.nested = common.meta().is_some_and(|m| m.nested);
        self.nesting_present = false;
    }

    fn header_cell(t: &Table, x: isize, y: isize) -> Option<&Cell> {
        if x < 0 || y < 0 {
            return None;
        }
        t.get_cell(x as usize, y as usize)
    }

    fn header_text(t: &Table, x: isize, y: isize) -> Option<&str> {
        Self::header_cell(t, x, y).map(Cell::text)
    }

    fn setup_columns(&mut self) {
        let column_order = self.align.meta.as_ref().unwrap().to_order();
        self.column_units = column_order.list().to_vec();
        if self.flags.columns_to_ignore.is_some() {
            let ignored = |flags: &Flags, tab: &Table, header: isize| -> HashSet<isize> {
                let mut s = HashSet::new();
                if tab.height() as isize >= header {
                    for i in 0..tab.width() {
                        let name = Self::header_text(tab, i as isize, header).unwrap_or("");
                        if flags.is_ignored(name) {
                            s.insert(i as isize);
                        }
                    }
                }
                s
            };
            let p_ignore = ignored(&self.flags, &self.p, self.rp_header);
            let a_ignore = ignored(&self.flags, &self.a, self.ra_header);
            let b_ignore = ignored(&self.flags, &self.b, self.rb_header);
            self.column_units.retain(|c| {
                !(p_ignore.contains(&c.p) || a_ignore.contains(&c.l) || b_ignore.contains(&c.r))
            });
        }
    }

    fn setup_moves(&mut self) {
        if self.flags.ordered {
            self.row_moves = move_units(&self.row_units).into_iter().collect();
            self.col_moves = move_units(&self.column_units).into_iter().collect();
        }
    }

    fn scan_activity(&mut self) {
        self.active_row = Vec::new();
        self.active_column = None;
        if !self.flags.show_unchanged {
            self.active_row = vec![0; self.row_units.len()];
        }
        if !self.flags.show_unchanged_columns {
            let mut ac = Vec::with_capacity(self.column_units.len());
            for unit in &self.column_units {
                let mut v = 0;
                if unit.l >= 0 && self.is_index_a.contains(&unit.l) {
                    v = 1;
                }
                if unit.r >= 0 && self.is_index_b.contains(&unit.r) {
                    v = 1;
                }
                if unit.p >= 0 && self.is_index_p.contains(&unit.p) {
                    v = 1;
                }
                ac.push(v);
            }
            self.active_column = Some(ac);
        }
    }

    fn is_equal(&self, aa: Option<&str>, bb: Option<&str>) -> bool {
        let f = &self.flags;
        if f.ignore_epsilon > 0.0 {
            if let Some(fa) = aa.and_then(parse_float) {
                if let Some(fb) = bb.and_then(parse_float) {
                    if (fa - fb).abs() < f.ignore_epsilon {
                        return true;
                    }
                }
            }
        }
        if f.ignore_whitespace || f.ignore_case {
            let norm = |s: Option<&str>| {
                s.map(|s| {
                    let mut t = s.to_string();
                    if f.ignore_whitespace {
                        t = t.trim().to_string();
                    }
                    if f.ignore_case {
                        t = t.to_lowercase();
                    }
                    t
                })
            };
            return norm(aa) == norm(bb);
        }
        aa == bb
    }

    fn scan_schema(&mut self) {
        self.schema = Vec::new();
        self.have_schema = false;
        for j in 0..self.column_units.len() {
            let cunit = self.column_units[j];
            let mut reordered = false;
            if self.flags.ordered && self.col_moves.contains(&j) {
                reordered = true;
                self.show_rc_numbers = true;
            }
            let mut act = String::new();
            if cunit.r >= 0 && cunit.lp() == -1 {
                self.have_schema = true;
                act = "+++".into();
                if let Some(ac) = &mut self.active_column {
                    if self.allow_column {
                        ac[j] = 1;
                    }
                }
                if self.allow_column {
                    self.summary.col_inserts += 1;
                }
            }
            if cunit.r < 0 && cunit.lp() >= 0 {
                self.have_schema = true;
                act = "---".into();
                if let Some(ac) = &mut self.active_column {
                    if self.allow_column {
                        ac[j] = 1;
                    }
                }
                if self.allow_column {
                    self.summary.col_deletes += 1;
                }
            }
            if cunit.r >= 0 && cunit.lp() >= 0 {
                if self.p.height() as isize >= self.rp_header && self.b.height() as isize >= self.rb_header {
                    let pp = Self::header_text(&self.p, cunit.lp(), self.rp_header);
                    let bb = Self::header_text(&self.b, cunit.r, self.rb_header);
                    if !self.is_equal(pp, bb) {
                        self.have_schema = true;
                        act = format!("({})", pp.unwrap_or(""));
                        if let Some(ac) = &mut self.active_column {
                            ac[j] = 1;
                            self.summary.col_renames += 1;
                        }
                    }
                }
            }
            if reordered {
                act = format!(":{act}");
                self.have_schema = true;
                self.active_column = None;
                self.summary.col_reorders += 1;
            }
            self.schema.push(act);
        }
    }

    fn check_rc_numbers(&mut self, w: usize, h: usize) {
        if !self.show_rc_numbers {
            if self.flags.always_show_order {
                self.show_rc_numbers = true;
            } else if self.flags.ordered {
                self.show_rc_numbers = is_reordered(&self.row_map, h);
                if !self.show_rc_numbers {
                    self.show_rc_numbers = is_reordered(&self.col_map, w);
                }
            }
        }
    }

    fn links(&self, unit: &Unit, row_like: bool) -> Cell {
        match self.builder {
            Builder::Flat => Cell::Str(if self.flags.count_like_a_spreadsheet && !row_like {
                unit.to_base26_string()
            } else {
                unit.to_string()
            }),
            Builder::Nested => {
                let fields = if unit.p >= -1 {
                    vec![
                        ("before".to_string(), neg_to_null(unit.p)),
                        ("ours".to_string(), neg_to_null(unit.l)),
                        ("theirs".to_string(), neg_to_null(unit.r)),
                    ]
                } else {
                    vec![
                        ("before".to_string(), neg_to_null(unit.l)),
                        ("after".to_string(), neg_to_null(unit.r)),
                    ]
                };
                Cell::from_value(Value::Hash(fields)).unwrap()
            }
        }
    }

    fn add_rc_numbers(&mut self, output: &mut Table) -> usize {
        let mut admin_w = 1;
        if self.show_rc_numbers && !self.flags.never_show_order {
            admin_w += 1;
            let target: Vec<Option<usize>> = (0..output.width()).map(|i| Some(i + 1)).collect();
            output.insert_or_delete_columns(&target, output.width() + 1);
            for i in 0..output.height() {
                match self.row_map.get(&i) {
                    None => output.set(0, i, Some("")),
                    Some(unit) => output.set_cell(0, i, Some(self.links(unit, true))),
                }
            }
            let target: Vec<Option<usize>> = (0..output.height()).map(|i| Some(i + 1)).collect();
            output.insert_or_delete_rows(&target, output.height() + 1);
            for i in 1..output.width() {
                match self.col_map.get(&(i - 1)) {
                    None => output.set(i, 0, Some("")),
                    Some(unit) => output.set_cell(i, 0, Some(self.links(unit, false))),
                }
            }
            output.set(0, 0, Some("@:@"));
        }
        admin_w
    }

    fn elide_columns(&self, output: &mut Table, admin_w: usize) {
        let Some(active) = &self.active_column else { return };
        if active.iter().all(|&a| a != 0) {
            return;
        }
        let mut fate: Vec<Option<usize>> = (0..admin_w).map(Some).collect();
        let mut at = admin_w;
        let mut ct = 0;
        let mut dots = Vec::new();
        for &a in active {
            let off = a == 0;
            ct = if off { ct + 1 } else { 0 };
            if off && ct > 1 {
                fate.push(None);
            } else {
                if off {
                    dots.push(at);
                }
                fate.push(Some(at));
                at += 1;
            }
        }
        output.insert_or_delete_columns(&fate, at);
        for d in dots {
            for j in 0..output.height() {
                output.set(d, j, Some("..."));
            }
        }
    }

    fn add_schema(&mut self, output: &mut Table) {
        if self.have_schema {
            let at = output.height();
            output.resize(self.column_units.len() + 1, at + 1);
            output.set(0, at, Some("!"));
            for j in 0..self.column_units.len() {
                output.set(j + 1, at, Some(self.schema[j].clone()));
            }
            self.schema_diff_found = true;
        }
    }

    fn add_header(&mut self, output: &mut Table) {
        if self.flags.always_show_header {
            let at = output.height();
            output.resize(self.column_units.len() + 1, at + 1);
            output.set(0, at, Some("@@"));
            for j in 0..self.column_units.len() {
                let cunit = self.column_units[j];
                let cell = if cunit.r >= 0 {
                    (self.b.height() != 0).then(|| Self::header_cell(&self.b, cunit.r, self.rb_header))
                } else if cunit.l >= 0 {
                    (self.a.height() != 0).then(|| Self::header_cell(&self.a, cunit.l, self.ra_header))
                } else if cunit.lp() >= 0 {
                    (self.p.height() != 0).then(|| Self::header_cell(&self.p, cunit.lp(), self.rp_header))
                } else {
                    None
                };
                if let Some(c) = cell {
                    output.set_cell(j + 1, at, c.cloned());
                }
                self.col_map.insert(j + 1, cunit);
            }
            self.top_line_done = true;
        }
    }

    fn refine_activity(&mut self) {
        spread_context(&self.row_units, self.flags.unchanged_context, Some(&mut self.active_row));
        spread_context(
            &self.column_units,
            self.flags.unchanged_column_context,
            self.active_column.as_mut(),
        );
        if let Some(ac) = &mut self.active_column {
            for a in ac.iter_mut() {
                if *a == 3 {
                    *a = 0;
                }
            }
        }
    }

    /// Compare tables held in cells, recording the results on the alignment,
    /// and return the cells to show instead (`TableDiff.checkNesting`).
    #[allow(clippy::too_many_arguments)]
    fn check_nesting(
        &mut self,
        have_ll: bool,
        ll: &Option<Cell>,
        have_rr: bool,
        rr: &Option<Cell>,
        have_pp: bool,
        pp: &Option<Cell>,
        x: usize,
        y: usize,
    ) -> (Option<Cell>, Option<Cell>, Option<Cell>) {
        let table_of = |c: &Option<Cell>| match c {
            Some(Cell::Native { value: Value::Table(t), .. }) => Some(t.clone()),
            _ => None,
        };
        let mut all_tables = true;
        if have_ll {
            all_tables &= table_of(ll).is_some();
        }
        if have_rr {
            all_tables &= table_of(rr).is_some();
        }
        if have_pp {
            all_tables &= table_of(pp).is_some();
        }
        if !all_tables {
            return (ll.clone(), rr.clone(), pp.clone());
        }
        let (lt, rt, pt) = (
            if have_ll { table_of(ll) } else { None },
            if have_rr { table_of(rr) } else { None },
            if have_pp { table_of(pp) } else { None },
        );
        let name_of = |t: &Option<Rc<Table>>| t.as_ref().and_then(|t| t.meta().and_then(|m| m.name.clone()));
        let key = name_of(&lt).or_else(|| name_of(&rt)).unwrap_or_else(|| format!("{x}_{y}"));
        let mut compare = false;
        let mut child: Option<Rc<Alignment>> = None;
        if let Some(comp) = &self.align.comp {
            let mut st = comp.borrow_mut();
            match &st.children {
                None => {
                    st.children = Some(Default::default());
                    st.child_order = Vec::new();
                    compare = true;
                }
                Some(c) => compare = !c.contains_key(&key),
            }
            if !compare {
                child = st.children.as_ref().unwrap().get(&key).cloned().flatten();
            }
        }
        if compare {
            self.nesting_present = true;
            let (Some(a), Some(b)) = (lt, rt) else {
                // The reference dereferences a missing table here and crashes.
                self.error = Some(format!("Cannot compare nested table '{key}': it is missing on one side"));
                return (ll.clone(), rr.clone(), pp.clone());
            };
            let aligned = Rc::new(CompareTable::from_rc(pt, a, b, &self.flags).align());
            let comp = self.align.comp.as_ref().unwrap();
            let mut st = comp.borrow_mut();
            st.children.as_mut().unwrap().insert(key.clone(), Some(aligned.clone()));
            st.child_order.push(key.clone());
            child = Some(aligned);
        }
        let Some(child) = child else {
            self.error = Some(format!("Nested comparison '{key}' is not available"));
            return (ll.clone(), rr.clone(), pp.clone());
        };
        let s = |t: String| Some(Cell::Str(t));
        if child.is_marked_as_identical() || (have_ll && !have_rr) || (have_rr && !have_ll) {
            let txt = format!("[{key}]");
            (s(txt.clone()), s(txt.clone()), s(txt))
        } else {
            (
                ll.as_ref().and_then(|_| s(format!("[a.{key}]"))),
                rr.as_ref().and_then(|_| s(format!("[b.{key}]"))),
                pp.as_ref().and_then(|_| s(format!("[p.{key}]"))),
            )
        }
    }

    fn scan_row(&mut self, unit: Unit, output: &mut Table, at: usize, i: usize, out: usize) {
        let mut row_update = false;
        for j in 0..self.column_units.len() {
            let cunit = self.column_units[j];
            let (mut pp, mut ll, mut rr): (Option<Cell>, Option<Cell>, Option<Cell>) = (None, None, None);
            let (mut have_pp, mut have_ll, mut have_rr) = (false, false, false);
            if cunit.p >= 0 && unit.p >= 0 {
                pp = self.p.get_cell(cunit.p as usize, unit.p as usize).cloned();
                have_pp = true;
            }
            if cunit.l >= 0 && unit.l >= 0 {
                ll = self.a.get_cell(cunit.l as usize, unit.l as usize).cloned();
                have_ll = true;
            }
            if cunit.r >= 0 && unit.r >= 0 {
                rr = self.b.get_cell(cunit.r as usize, unit.r as usize).cloned();
                have_rr = true;
                if (if have_pp { cunit.p } else { cunit.l }) < 0
                    && rr.as_ref().is_some_and(|c| !c.text().is_empty())
                    && self.allow_column
                {
                    self.have_addition = true;
                }
            }
            if self.nested {
                let (l2, r2, p2) = self.check_nesting(have_ll, &ll, have_rr, &rr, have_pp, &pp, i, j);
                ll = l2;
                rr = r2;
                pp = p2;
            }
            let text = |c: &Option<Cell>| c.as_ref().map(|c| c.text().to_string());
            let (pp_t, ll_t, rr_t) = (text(&pp), text(&ll), text(&rr));

            let dd: Option<Cell>;
            let mut dd_to: Option<Cell> = None;
            let mut have_dd_to = false;
            let mut dd_to_alt: Option<Cell> = None;
            let mut have_dd_to_alt = false;
            if have_pp {
                if !have_rr {
                    dd = pp.clone();
                } else if self.is_equal(pp_t.as_deref(), rr_t.as_deref()) {
                    dd = ll.clone();
                } else {
                    dd = pp.clone();
                    dd_to = rr.clone();
                    have_dd_to = true;
                    if !self.is_equal(pp_t.as_deref(), ll_t.as_deref()) {
                        dd_to_alt = ll.clone();
                        have_dd_to_alt = true;
                    }
                }
            } else if have_ll {
                if !have_rr {
                    dd = ll.clone();
                } else if self.is_equal(ll_t.as_deref(), rr_t.as_deref()) {
                    dd = ll.clone();
                } else {
                    dd = ll.clone();
                    dd_to = rr.clone();
                    have_dd_to = true;
                }
            } else {
                dd = rr.clone();
            }

            let mut cell: Option<Cell> = dd.clone();
            if have_dd_to && ((dd.is_some() && self.allow_update) || self.allow_column) {
                if !row_update {
                    if out == 0 {
                        self.summary.row_updates += 1;
                    }
                    row_update = true;
                }
                if let Some(ac) = &mut self.active_column {
                    ac[j] = 1;
                }
                if self.sep.is_empty() {
                    self.sep = match self.builder {
                        Builder::Flat => separator_for(&self.a, &self.b, "->"),
                        Builder::Nested => "->".into(),
                    };
                }
                let is_conflict = have_dd_to_alt
                    && !self.is_equal(dd_to.as_ref().map(Cell::text), dd_to_alt.as_ref().map(Cell::text));
                if !is_conflict {
                    cell = Some(match self.builder {
                        Builder::Flat => Cell::Str(format!(
                            "{}{}{}",
                            quote_for_diff(dd.as_ref()),
                            self.sep,
                            quote_for_diff(dd_to.as_ref())
                        )),
                        Builder::Nested => Cell::from_value(Value::Hash(vec![
                            ("before".into(), value_of(dd.as_ref())),
                            ("after".into(), value_of(dd_to.as_ref())),
                        ]))
                        .unwrap(),
                    });
                    if self.sep.len() > self.act.len() {
                        self.act = self.sep.clone();
                    }
                } else {
                    if self.conflict_sep.is_empty() {
                        self.conflict_sep = match self.builder {
                            Builder::Flat => format!("{}{}", separator_for(&self.p, &self.a, "!"), self.sep),
                            Builder::Nested => "!->".into(),
                        };
                    }
                    cell = Some(match self.builder {
                        Builder::Flat => {
                            let cs = &self.conflict_sep;
                            let t = |c: &Option<Cell>| c.as_ref().map_or(String::new(), |c| c.text().to_string());
                            Cell::Str(format!("{}{cs}{}{cs}{}", t(&dd), t(&dd_to_alt), t(&dd_to)))
                        }
                        Builder::Nested => Cell::from_value(Value::Hash(vec![
                            ("before".into(), value_of(dd.as_ref())),
                            ("ours".into(), value_of(dd_to_alt.as_ref())),
                            ("theirs".into(), value_of(dd_to.as_ref())),
                        ]))
                        .unwrap(),
                    });
                    self.act = self.conflict_sep.clone();
                }
                if self.column_units_updated.insert(j) {
                    self.summary.col_updates += 1;
                }
            }
            if self.act.is_empty() && self.have_addition {
                self.act = "+".into();
            }
            if self.act == "+++" && have_rr {
                if let Some(ac) = &mut self.active_column {
                    ac[j] = 1;
                }
            }
            if self.publish && self.active_column.as_ref().map_or(true, |ac| ac[j] > 0) {
                output.set_cell(j + 1, at, cell);
            }
        }
        if self.publish {
            output.set(0, at, Some(self.act.clone()));
            self.row_map.insert(at, unit);
        }
        if !self.act.is_empty() {
            self.diff_found = true;
            if !self.publish && !self.active_row.is_empty() {
                self.active_row[i] = 1;
            }
        }
    }

    /// Write the diff into `output` (which is cleared first).
    pub fn hilite(&mut self, output: &mut Table) -> bool {
        output.resize(0, 0);
        output.clear();
        self.reset();
        self.setup_tables();
        self.setup_columns();
        self.setup_moves();
        self.scan_activity();
        self.scan_schema();
        self.add_schema(output);
        self.add_header(output);

        let outer_reps = if self.flags.show_unchanged && self.flags.show_unchanged_columns { 1 } else { 2 };
        let mut output_height = output.height();
        let output_height_init = output.height();

        for out in 0..outer_reps {
            if out == 1 {
                self.refine_activity();
                let mut rows = count_active(&self.active_row) + output_height_init;
                if self.top_line_done {
                    rows -= 1;
                }
                output_height = output_height_init;
                if rows > output.height() {
                    output.resize(self.column_units.len() + 1, rows);
                }
            }
            let mut showed_dummy = false;
            for i in 0..self.row_units.len() {
                let unit = self.row_units[i];
                let mut reordered = false;
                if self.flags.ordered && self.row_moves.contains(&i) {
                    reordered = true;
                    self.show_rc_numbers = true;
                }
                if unit.r < 0 && unit.l < 0 {
                    continue;
                }
                if unit.r == 0 && unit.lp() <= 0 && self.top_line_done {
                    continue;
                }
                self.publish = self.flags.show_unchanged;
                let mut dummy = false;
                if out == 1 {
                    let value = self.active_row.get(i).copied();
                    self.publish = value.is_some_and(|v| v > 0);
                    dummy = value == Some(3);
                    if dummy && showed_dummy {
                        continue;
                    }
                    if !self.publish {
                        continue;
                    }
                }
                if !dummy {
                    showed_dummy = false;
                }
                let at = output_height;
                if self.publish {
                    output_height += 1;
                    if output.height() < output_height {
                        output.resize(self.column_units.len() + 1, output_height);
                    }
                }
                if dummy {
                    for j in 0..self.column_units.len() + 1 {
                        output.set(j, at, Some("..."));
                    }
                    showed_dummy = true;
                    continue;
                }
                self.have_addition = false;
                let mut skip = false;
                self.act = String::new();
                if reordered {
                    self.act = ":".into();
                    if out == 0 {
                        self.summary.row_reorders += 1;
                    }
                }
                if unit.p < 0 && unit.l < 0 && unit.r >= 0 {
                    if !self.allow_insert {
                        skip = true;
                    }
                    self.act = "+++".into();
                    if out == 0 && !skip {
                        self.summary.row_inserts += 1;
                    }
                }
                if (unit.p >= 0 || !self.has_parent) && unit.l >= 0 && unit.r < 0 {
                    if !self.allow_delete {
                        skip = true;
                    }
                    self.act = "---".into();
                    if out == 0 && !skip {
                        self.summary.row_deletes += 1;
                    }
                }
                if skip {
                    if !self.publish && !self.active_row.is_empty() {
                        self.active_row[i] = -3;
                    }
                    continue;
                }
                self.scan_row(unit, output, at, i, out);
            }
        }
        self.check_rc_numbers(output.width(), output.height());
        let admin_w = self.add_rc_numbers(output);
        self.elide_columns(output, admin_w);
        true
    }
}
