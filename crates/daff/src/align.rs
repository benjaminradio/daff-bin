//! Row/column alignment between tables, ported from `coopy.Alignment`,
//! `coopy.Index*` and `coopy.CompareTable`.
use crate::unit::{stable_sort_by, Ordering, Unit};
use crate::{Flags, Table};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

/// Results of the comparisons of tables nested in cells (`TableComparisonState`
/// children), kept on the parent's alignment so a diff can find them again.
#[derive(Debug, Default)]
pub struct NestState {
    pub flags: Flags,
    pub children: Option<HashMap<String, Option<Rc<Alignment>>>>,
    pub child_order: Vec<String>,
}

/// Where does a row/column of table A appear in table B?
#[derive(Debug, Clone, Default)]
pub struct Alignment {
    map_a2b: BTreeMap<isize, isize>,
    map_b2a: BTreeMap<isize, isize>,
    ta: Option<Rc<Table>>,
    tb: Option<Rc<Table>>,
    ia: isize,
    ib: isize,
    map_count: usize,
    index_columns: Option<Vec<Unit>>,
    manual_order: Option<Ordering>,
    marked_as_identical: bool,
    pub comp: Option<Rc<RefCell<NestState>>>,
    pub reference: Option<Box<Alignment>>,
    pub meta: Option<Box<Alignment>>,
    pub has_addition: bool,
    pub has_removal: bool,
}

impl Alignment {
    pub fn new() -> Self {
        Alignment { ia: -1, ib: -1, ..Default::default() }
    }

    pub fn tables(&mut self, ta: Rc<Table>, tb: Rc<Table>) {
        self.ta = Some(ta);
        self.tb = Some(tb);
    }

    pub fn headers(&mut self, ia: isize, ib: isize) {
        self.ia = ia;
        self.ib = ib;
    }

    pub fn link(&mut self, a: isize, b: isize) {
        if a != -1 {
            self.map_a2b.insert(a, b);
        } else {
            self.has_addition = true;
        }
        if b != -1 {
            self.map_b2a.insert(b, a);
        } else {
            self.has_removal = true;
        }
        self.map_count += 1;
    }

    pub fn add_index_columns(&mut self, unit: Unit) {
        self.index_columns.get_or_insert_with(Vec::new).push(unit);
    }

    pub fn index_columns(&self) -> Option<&[Unit]> {
        self.index_columns.as_deref()
    }

    pub fn a2b(&self, a: isize) -> Option<isize> {
        self.map_a2b.get(&a).copied()
    }

    pub fn b2a(&self, b: isize) -> Option<isize> {
        self.map_b2a.get(&b).copied()
    }

    pub fn count(&self) -> usize {
        self.map_count
    }

    pub fn source(&self) -> Option<&Rc<Table>> {
        self.ta.as_ref()
    }

    pub fn target(&self) -> Option<&Rc<Table>> {
        self.tb.as_ref()
    }

    pub fn source_header(&self) -> isize {
        self.ia
    }

    pub fn target_header(&self) -> isize {
        self.ib
    }

    pub fn mark_identical(&mut self) {
        self.marked_as_identical = true;
    }

    pub fn is_marked_as_identical(&self) -> bool {
        self.marked_as_identical
    }

    /// Manually build the order, instead of deriving it from the links.
    pub fn add_to_order(&mut self, l: isize, r: isize, p: isize) {
        self.manual_order.get_or_insert_with(Ordering::new).add(l, r, p);
    }

    /// The alignment as a merged list of rows/columns.
    pub fn to_order(&self) -> Ordering {
        match &self.manual_order {
            Some(o) => o.clone(),
            None => self.to_order3(),
        }
    }

    fn to_order3(&self) -> Ordering {
        let mut order: Vec<Unit> = Vec::new();
        match &self.reference {
            None => {
                for (&k, &v) in &self.map_a2b {
                    order.push(Unit::new(k, v, -2));
                }
                for (&k, &v) in &self.map_b2a {
                    if v == -1 {
                        order.push(Unit::new(-1, k, -2));
                    }
                }
            }
            Some(reference) => {
                for (&k, &v) in &self.map_a2b {
                    let l = reference.a2b(k).unwrap_or(-2);
                    order.push(Unit::new(l, v, k));
                }
                for (&k, &v) in &reference.map_b2a {
                    if v == -1 {
                        order.push(Unit::new(k, -1, -1));
                    }
                }
                for (&k, &v) in &self.map_b2a {
                    if v == -1 {
                        order.push(Unit::new(-1, k, -1));
                    }
                }
            }
        }
        let top = order.len();
        let mut remotes: Vec<usize> = Vec::new();
        let mut locals: Vec<usize> = Vec::new();
        for (o, u) in order.iter().enumerate().take(top) {
            if u.r >= 0 {
                remotes.push(o);
            } else {
                locals.push(o);
            }
        }
        let has_ref = self.reference.is_some();
        let ord = &order;
        let remote_sort = |a: &usize, b: &usize| -> i64 {
            let (ua, ub) = (ord[*a], ord[*b]);
            if !has_ref {
                return (ua.r - ub.r) as i64;
            }
            if a == b {
                return 0;
            }
            let o1 = (ua.r - ub.r) as i64;
            if ua.p >= 0 && ub.p >= 0 {
                let o2 = (ua.p - ub.p) as i64;
                if o1 * o2 < 0 {
                    return o1;
                }
                return (ua.l - ub.l) as i64;
            }
            o1
        };
        let local_sort = |a: &usize, b: &usize| -> i64 {
            let (ua, ub) = (ord[*a], ord[*b]);
            if a == b {
                return 0;
            }
            if !has_ref {
                if ua.l >= 0 && ub.l >= 0 {
                    return (ua.l - ub.l) as i64;
                }
            } else if ua.l >= 0 && ub.l >= 0 {
                let o1 = (ua.l - ub.l) as i64;
                if ua.p >= 0 && ub.p >= 0 {
                    let o2 = (ua.p - ub.p) as i64;
                    if o1 * o2 < 0 {
                        return o1;
                    }
                    return o2;
                }
            }
            if ua.l >= 0 {
                return 1;
            }
            if ub.l >= 0 {
                return -1;
            }
            *a as i64 - *b as i64
        };
        stable_sort_by(&mut remotes, &remote_sort);
        stable_sort_by(&mut locals, &local_sort);

        let mut revised: Vec<Unit> = Vec::new();
        let (mut at_r, mut at_l) = (0, 0);
        for _ in 0..top {
            if at_r < remotes.len() && at_l < locals.len() {
                let ur = order[remotes[at_r]];
                let ul = order[locals[at_l]];
                if ul.l == -1 && ul.p >= 0 && ur.p >= 0 {
                    if ur.p > ul.p {
                        revised.push(ul);
                        at_l += 1;
                        continue;
                    }
                } else if ur.l > ul.l {
                    revised.push(ul);
                    at_l += 1;
                    continue;
                }
                revised.push(ur);
                at_r += 1;
                continue;
            }
            if at_r < remotes.len() {
                revised.push(order[remotes[at_r]]);
                at_r += 1;
                continue;
            }
            if at_l < locals.len() {
                revised.push(order[locals[at_l]]);
                at_l += 1;
            }
        }
        let mut result = Ordering::new();
        result.set_list(revised);
        if self.reference.is_none() {
            result.ignore_parent();
        }
        result
    }
}

// ---------------------------------------------------------------- index ---

fn cell_text(t: &Table, x: usize, y: usize) -> &str {
    t.get(x, y).unwrap_or("")
}

#[derive(Default, Clone)]
struct Index {
    items: HashMap<String, Vec<usize>>,
    cols: Vec<usize>,
    top_freq: usize,
    hdr: usize,
    ignore_whitespace: bool,
    ignore_case: bool,
    table: Option<Rc<Table>>,
}

impl Index {
    fn new(flags: &Flags) -> Self {
        Index {
            ignore_whitespace: flags.ignore_whitespace,
            ignore_case: flags.ignore_case,
            ..Default::default()
        }
    }

    fn index_table(&mut self, t: &Rc<Table>, hdr: usize) {
        self.table = Some(t.clone());
        self.hdr = hdr;
        for i in 0..t.height() {
            let key = self.to_key(t, i);
            let item = self.items.entry(key).or_default();
            item.push(i);
            self.top_freq = self.top_freq.max(item.len());
        }
    }

    fn to_key(&self, t: &Table, i: usize) -> String {
        self.key_from(i < self.hdr, |c| Some(cell_text(t, c, i).to_string()))
    }

    fn to_key_by_content(&self, row: &dyn Row) -> String {
        self.key_from(row.is_preamble(), |c| row.row_string(c))
    }

    fn key_from(&self, preamble: bool, cell: impl Fn(usize) -> Option<String>) -> String {
        let mut wide = String::from(if preamble { "_" } else { "" });
        for (k, &c) in self.cols.iter().enumerate() {
            let mut txt = cell(c).unwrap_or_default();
            if self.ignore_whitespace {
                txt = txt.trim().to_string();
            }
            if self.ignore_case {
                txt = txt.to_lowercase();
            }
            if k > 0 {
                wide.push_str(" // ");
            }
            if txt.is_empty() || txt == "null" || txt == "undefined" {
                continue;
            }
            wide.push_str(&txt);
        }
        wide
    }
}

/// A row whose cells are read by column of the indexed table
/// (`coopy.Row`); used to look up rows of a table by content.
pub trait Row {
    fn row_string(&self, c: usize) -> Option<String>;
    fn is_preamble(&self) -> bool;
}

pub struct CrossMatch {
    pub spot_a: usize,
    pub spot_b: usize,
    pub item_a: Vec<usize>,
    pub item_b: Option<usize>,
}

#[derive(Clone)]
pub struct IndexPair {
    ia: Index,
    ib: Index,
}

impl IndexPair {
    fn new(flags: &Flags) -> Self {
        IndexPair { ia: Index::new(flags), ib: Index::new(flags) }
    }

    fn add_columns(&mut self, ca: usize, cb: usize) {
        self.ia.cols.push(ca);
        self.ib.cols.push(cb);
    }

    fn index_tables(&mut self, a: &Rc<Table>, b: &Rc<Table>, hdr: usize) {
        self.ia.index_table(a, hdr);
        self.ib.index_table(b, hdr);
    }

    pub fn query_by_content(&self, row: &dyn Row) -> CrossMatch {
        self.query_by_key(&self.ia.to_key_by_content(row))
    }

    fn query_local(&self, row: usize) -> CrossMatch {
        self.query_by_key(&self.local_key(row))
    }

    fn query_by_key(&self, ka: &str) -> CrossMatch {
        let item_a = self.ia.items.get(ka);
        let item_b = self.ib.items.get(ka);
        let (mut spot_a, mut spot_b) = (0, 0);
        if !ka.is_empty() {
            spot_a = item_a.map_or(0, Vec::len);
            spot_b = item_b.map_or(0, Vec::len);
        }
        CrossMatch {
            spot_a,
            spot_b,
            item_a: item_a.cloned().unwrap_or_default(),
            item_b: item_b.and_then(|v| v.first().copied()),
        }
    }

    fn local_key(&self, row: usize) -> String {
        self.ia.to_key(self.ia.table.as_ref().unwrap(), row)
    }

    fn remote_key(&self, row: usize) -> String {
        self.ib.to_key(self.ib.table.as_ref().unwrap(), row)
    }

    fn top_freq(&self) -> usize {
        self.ia.top_freq.max(self.ib.top_freq)
    }
}

// -------------------------------------------------------------- compare ---

/// Prepared comparison between two tables (or three, with a parent).
pub struct CompareTable {
    p: Option<Rc<Table>>,
    a: Rc<Table>,
    b: Rc<Table>,
    flags: Flags,
}

/// Prepare to compare `local` and `remote`; call [`CompareTable::align`] to run it.
pub fn compare_tables(local: &Table, remote: &Table, flags: Option<&Flags>) -> CompareTable {
    CompareTable {
        p: None,
        a: Rc::new(local.clone()),
        b: Rc::new(remote.clone()),
        flags: flags.cloned().unwrap_or_default(),
    }
}

/// As [`compare_tables`], given the common ancestor `parent`.
pub fn compare_tables3(
    parent: Option<&Table>,
    local: &Table,
    remote: &Table,
    flags: Option<&Flags>,
) -> CompareTable {
    CompareTable {
        p: parent.map(|t| Rc::new(t.clone())),
        a: Rc::new(local.clone()),
        b: Rc::new(remote.clone()),
        flags: flags.cloned().unwrap_or_default(),
    }
}

impl CompareTable {
    /// Compare tables that are already shared (nested tables).
    pub fn from_rc(p: Option<Rc<Table>>, a: Rc<Table>, b: Rc<Table>, flags: &Flags) -> Self {
        CompareTable { p, a, b, flags: flags.clone() }
    }

    pub fn align(&self) -> Alignment {
        let mut a = self.align_impl(&mut None);
        a.comp = Some(Rc::new(RefCell::new(NestState { flags: self.flags.clone(), ..Default::default() })));
        a
    }

    /// As [`align`](Self::align), also returning the row indexes built along
    /// the way (`CompareTable.storeIndexes`).
    pub fn align_storing_indexes(&self) -> (Alignment, Vec<IndexPair>) {
        let mut idx = Some(Vec::new());
        let a = self.align_impl(&mut idx);
        (a, idx.unwrap())
    }

    fn align_impl(&self, indexes: &mut Option<Vec<IndexPair>>) -> Alignment {
        let mut align = Alignment::new();
        match &self.p {
            None => self.align_core2(&mut align, &self.a, &self.b, indexes),
            Some(p) => {
                let mut reference = Alignment::new();
                self.align_core2(&mut align, p, &self.b, indexes);
                self.align_core2(&mut reference, p, &self.a, indexes);
                align.meta.as_mut().unwrap().reference = Some(reference.meta.clone().unwrap());
                align.reference = Some(Box::new(reference));
            }
        }
        align
    }

    fn align_core2(
        &self,
        align: &mut Alignment,
        a: &Rc<Table>,
        b: &Rc<Table>,
        indexes: &mut Option<Vec<IndexPair>>,
    ) {
        let mut meta = Alignment::new();
        self.align_columns(&mut meta, a, b);
        let column_order = meta.to_order();
        align.meta = Some(Box::new(meta));
        align.tables(a.clone(), b.clone());

        let ha = a.height();
        let hb = b.height();
        let flags = &self.flags;
        let ids = flags.ids.as_ref();

        // The reference tests ignored columns against the row alignment's
        // header, which is never set at this point, so it never matches.
        let common_units: Vec<Unit> = column_order
            .list()
            .iter()
            .filter(|u| u.l >= 0 && u.r >= 0 && u.p != -1)
            .copied()
            .collect();

        let mut index_top: Option<IndexPair> = None;
        let mut pending_ct = ha as isize;
        let mut reverse_pending_ct = hb as isize;
        let mut used: HashSet<isize> = HashSet::new();
        let mut used_reverse: HashSet<isize> = HashSet::new();

        if let Some(ids) = ids {
            // We've been told which columns form the primary key.
            let mut index = IndexPair::new(flags);
            for unit in &common_units {
                let na = cell_text(a, unit.l as usize, 0);
                let nb = cell_text(b, unit.r as usize, 0);
                if ids.iter().any(|i| i == na || i == nb) {
                    index.add_columns(unit.l as usize, unit.r as usize);
                    align.add_index_columns(*unit);
                }
            }
            index.index_tables(a, b, 1);
            for j in 0..ha {
                let cross = index.query_local(j);
                if cross.spot_a != 1 || cross.spot_b != 1 {
                    continue;
                }
                let jb = cross.item_b.unwrap() as isize;
                align.link(j as isize, jb);
                used.insert(jb);
                if !used_reverse.contains(&(j as isize)) {
                    reverse_pending_ct -= 1;
                }
                used_reverse.insert(j as isize);
            }
            if let Some(v) = indexes {
                v.push(index.clone());
            }
            index_top = Some(index);
        } else {
            // Heuristics: try combinations of columns until rows are identified.
            const N: usize = 5;
            let mut columns: Vec<usize> = Vec::new();
            if common_units.len() > N {
                let mut eval: Vec<(usize, usize)> = Vec::new();
                for (i, u) in common_units.iter().enumerate() {
                    let mut mem: HashSet<&str> = HashSet::new();
                    let mut mem2: HashSet<&str> = HashSet::new();
                    let mut ct = 0;
                    for j in 0..ha {
                        if mem.insert(cell_text(a, u.l as usize, j)) {
                            ct += 1;
                        }
                    }
                    for j in 0..hb {
                        if mem2.insert(cell_text(b, u.r as usize, j)) {
                            ct += 1;
                        }
                    }
                    eval.push((i, ct));
                }
                eval.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
                columns = eval.into_iter().map(|e| e.0).take(N).collect();
            } else {
                columns.extend(0..common_units.len());
            }

            let top: usize = 1usize << columns.len();
            let mut pending: BTreeSet<usize> = (0..ha).collect();
            let mut added_columns: HashSet<usize> = HashSet::new();
            let mut index_ct = 0;

            for k in 1..top {
                if pending_ct == 0 {
                    break;
                }
                let active_columns: Vec<usize> =
                    (0..columns.len()).filter(|bit| k >> bit & 1 == 1).map(|bit| columns[bit]).collect();

                let mut index = IndexPair::new(flags);
                for &col in &active_columns {
                    let unit = common_units[col];
                    index.add_columns(unit.l as usize, unit.r as usize);
                    if added_columns.insert(col) {
                        align.add_index_columns(unit);
                    }
                }
                index.index_tables(a, b, 1);

                let h = a.height().max(b.height()).max(1);
                let ratio = index.top_freq() as f64 / (h as f64 + 20.0);
                let lousy = ratio >= 0.1 && (index_ct > 0 || k < top - 1);
                if k == top - 1 {
                    // Kept for the duplicate-line pass below.
                    if lousy {
                        index_top = Some(index);
                        continue;
                    }
                } else if lousy {
                    continue;
                }
                index_ct += 1;
                if let Some(v) = indexes {
                    v.push(index.clone());
                }

                let mut fixed: Vec<usize> = Vec::new();
                for &j in &pending {
                    let cross = index.query_local(j);
                    if cross.spot_a != 1 || cross.spot_b != 1 {
                        continue;
                    }
                    let val = cross.item_b.unwrap() as isize;
                    if !used.contains(&val) {
                        fixed.push(j);
                        align.link(j as isize, val);
                        used.insert(val);
                        if !used_reverse.contains(&(j as isize)) {
                            reverse_pending_ct -= 1;
                        }
                        used_reverse.insert(j as isize);
                    }
                }
                for j in fixed {
                    pending.remove(&j);
                    pending_ct -= 1;
                }
                if k == top - 1 {
                    index_top = Some(index);
                }
            }
        }

        if let Some(index_top) = &index_top {
            // Duplicated lines: link them when it is a clear win to do so.
            let (ha_i, hb_i) = (ha as isize, hb as isize);
            let (mut offset, mut scale) = (0isize, 1isize);
            for _ in 0..2 {
                if pending_ct > 0 {
                    let mut xb: Option<isize> = if scale == -1 && hb > 0 { Some(hb_i - 1) } else { None };
                    for xa0 in 0..ha_i {
                        let xa = xa0 * scale + offset;
                        if let Some(xb2) = align.a2b(xa) {
                            let nxt = xb2 + scale;
                            xb = Some(nxt);
                            if nxt >= hb_i || nxt < 0 {
                                break;
                            }
                            continue;
                        }
                        let Some(xbv) = xb else { continue };
                        let ka = index_top.local_key(xa as usize);
                        let kb = index_top.remote_key(xbv as usize);
                        if ka != kb || used.contains(&xbv) {
                            continue;
                        }
                        align.link(xa, xbv);
                        used.insert(xbv);
                        used_reverse.insert(xa);
                        pending_ct -= 1;
                        let nxt = xbv + scale;
                        xb = Some(nxt);
                        if nxt >= hb_i || nxt < 0 || pending_ct == 0 {
                            break;
                        }
                    }
                }
                offset = ha_i - 1;
                scale = -1;
            }
            let (mut offset, mut scale) = (0isize, 1isize);
            for _ in 0..2 {
                if reverse_pending_ct > 0 {
                    let mut xa: Option<isize> = if scale == -1 && ha > 0 { Some(ha_i - 1) } else { None };
                    for xb0 in 0..hb_i {
                        let xb = xb0 * scale + offset;
                        if let Some(xa2) = align.b2a(xb) {
                            let nxt = xa2 + scale;
                            xa = Some(nxt);
                            if nxt >= ha_i || nxt < 0 {
                                break;
                            }
                            continue;
                        }
                        let Some(xav) = xa else { continue };
                        let ka = index_top.local_key(xav as usize);
                        let kb = index_top.remote_key(xb as usize);
                        if ka != kb || used_reverse.contains(&xav) {
                            continue;
                        }
                        align.link(xav, xb);
                        used.insert(xb);
                        used_reverse.insert(xav);
                        reverse_pending_ct -= 1;
                        let nxt = xav + scale;
                        xa = Some(nxt);
                        if nxt >= ha_i || nxt < 0 || reverse_pending_ct == 0 {
                            break;
                        }
                    }
                }
                offset = hb_i - 1;
                scale = -1;
            }
        }

        // For consistency, explicitly mark unaligned rows.
        for i in 1..ha as isize {
            if !used_reverse.contains(&i) {
                align.link(i, -1);
            }
        }
        for i in 1..hb as isize {
            if !used.contains(&i) {
                align.link(-1, i);
            }
        }
        // Headers are expected on row 0; link them even if quite different.
        if ha > 0 && hb > 0 {
            align.link(0, 0);
            align.headers(0, 0);
        }
    }

    fn align_columns(&self, align: &mut Alignment, a: &Rc<Table>, b: &Rc<Table>) {
        align.tables(a.clone(), b.clone());
        const SLOP: usize = 5;

        // Column key -> column index; -1 marks a duplicated key.
        type Keys = HashMap<String, isize>;
        let mut ct_best: isize = -1;
        let mut best: Option<(Keys, Keys)> = None;
        let (mut ra_header, mut rb_header) = (0isize, 0isize);
        let (mut ra_uniques, mut rb_uniques) = (0isize, 0isize);
        for ra in 0..SLOP {
            for rb in 0..SLOP {
                let mut ma: Keys = HashMap::new();
                let mut mb: Keys = HashMap::new();
                let mut uniques = 0isize;
                if ra < a.height() {
                    for ca in 0..a.width() {
                        let key = cell_text(a, ca, ra).to_string();
                        if ma.contains_key(&key) {
                            ma.insert(key, -1);
                            uniques -= 1;
                        } else {
                            ma.insert(key, ca as isize);
                            uniques += 1;
                        }
                    }
                    if uniques > ra_uniques {
                        ra_header = ra as isize;
                        ra_uniques = uniques;
                    }
                }
                uniques = 0;
                if rb < b.height() {
                    for cb in 0..b.width() {
                        let key = cell_text(b, cb, rb).to_string();
                        if mb.contains_key(&key) {
                            mb.insert(key, -1);
                            uniques -= 1;
                        } else {
                            mb.insert(key, cb as isize);
                            uniques += 1;
                        }
                    }
                    if uniques > rb_uniques {
                        rb_header = rb as isize;
                        rb_uniques = uniques;
                    }
                }
                let ct = ma
                    .iter()
                    .filter(|(k, &i0)| matches!(mb.get(*k), Some(&i1) if i1 >= 0 && i0 >= 0))
                    .count() as isize;
                if ct > ct_best {
                    ct_best = ct;
                    best = Some((ma, mb));
                }
            }
        }
        let Some((ma, mb)) = best else {
            if a.height() > 0 && b.height() == 0 {
                align.headers(0, -1);
            } else if a.height() == 0 && b.height() > 0 {
                align.headers(-1, 0);
            }
            return;
        };
        for (key, &i0) in &ma {
            match mb.get(key) {
                Some(&i1) => align.link(i0, i1),
                None => align.link(i0, -1),
            }
        }
        for (key, &i1) in &mb {
            if !ma.contains_key(key) {
                align.link(-1, i1);
            }
        }
        align.headers(ra_header, rb_header);
    }
}
