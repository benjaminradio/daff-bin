//! Detect which rows/columns moved, ported from `coopy.Mover`.
use crate::unit::{stable_sort_by, Unit};
use std::collections::BTreeMap;

/// Indexes into `units` of the elements considered to have moved.
pub fn move_units(units: &[Unit]) -> Vec<usize> {
    let mut ltop = -1isize;
    let mut rtop = -1isize;
    let mut in_src: BTreeMap<isize, usize> = BTreeMap::new();
    let mut in_dest: BTreeMap<isize, usize> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        if u.l >= 0 && u.r >= 0 {
            ltop = ltop.max(u.l);
            rtop = rtop.max(u.r);
            in_src.insert(u.l, i);
            in_dest.insert(u.r, i);
        }
    }
    let isrc: Vec<usize> = (0..=ltop).filter_map(|i| in_src.get(&i).copied()).collect();
    let idest: Vec<usize> = (0..=rtop).filter_map(|i| in_dest.get(&i).copied()).collect();
    move_without_extras(&isrc, &idest)
}

fn move_without_extras(src: &[usize], dest: &[usize]) -> Vec<usize> {
    if src.len() != dest.len() || src.len() <= 1 {
        return Vec::new();
    }
    let len = src.len();
    let in_src: BTreeMap<usize, usize> = src.iter().enumerate().map(|(i, &s)| (s, i)).collect();
    let mut blk_len: BTreeMap<usize, usize> = BTreeMap::new();
    let mut blk_src_loc: BTreeMap<usize, usize> = BTreeMap::new();
    let mut blk_dest_loc: BTreeMap<usize, usize> = BTreeMap::new();
    let mut ct = 0;
    let mut in_cursor: isize = -2;
    let mut blk = 0;
    for (out_cursor, &v) in dest.iter().enumerate().take(len) {
        let next = in_src[&v];
        if next as isize != in_cursor + 1 {
            blk = v;
            ct = 1;
            blk_src_loc.insert(blk, next);
            blk_dest_loc.insert(blk, out_cursor);
        } else {
            ct += 1;
        }
        blk_len.insert(blk, ct);
        in_cursor = next as isize;
    }
    let mut blks: Vec<usize> = blk_len.keys().copied().collect();
    stable_sort_by(&mut blks, &|a, b| {
        let diff = blk_len[b] as i64 - blk_len[a] as i64;
        if diff == 0 { *a as i64 - *b as i64 } else { diff }
    });
    let mut moved = Vec::new();
    while !blks.is_empty() {
        let blk = blks.remove(0);
        let ref_src = blk_src_loc[&blk];
        let ref_dest = blk_dest_loc[&blk];
        let mut i = blks.len() as isize - 1;
        while i >= 0 {
            let blki = blks[i as usize];
            let mut src_loc = blk_src_loc[&blki];
            let to_left_src = src_loc < ref_src;
            let to_left_dest = blk_dest_loc[&blki] < ref_dest;
            if to_left_src != to_left_dest {
                for _ in 0..blk_len[&blki] {
                    moved.push(src[src_loc]);
                    src_loc += 1;
                }
                blks.remove(i as usize);
            }
            i -= 1;
        }
    }
    moved
}
