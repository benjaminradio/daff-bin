/// Row/column numbers for related content in the local, remote and parent
/// tables. `-1` means absent; `-2` for `p` means there is no parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unit {
    pub l: isize,
    pub r: isize,
    pub p: isize,
}

impl Unit {
    pub fn new(l: isize, r: isize, p: isize) -> Self {
        Unit { l, r, p }
    }

    /// Parent position if there is one, otherwise local.
    pub fn lp(&self) -> isize {
        if self.p == -2 {
            self.l
        } else {
            self.p
        }
    }

    pub fn to_base26_string(&self) -> String {
        if self.p >= -1 {
            format!("{}|{}:{}", base26(self.p), base26(self.l), base26(self.r))
        } else {
            format!("{}:{}", base26(self.l), base26(self.r))
        }
    }
}

fn describe(i: isize) -> String {
    if i >= 0 { i.to_string() } else { "-".into() }
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.p >= -1 {
            write!(f, "{}|{}:{}", describe(self.p), describe(self.l), describe(self.r))
        } else {
            write!(f, "{}:{}", describe(self.l), describe(self.r))
        }
    }
}

fn base26(num: isize) -> String {
    if num < 0 {
        return "-".into();
    }
    let mut num = num;
    let mut out = String::new();
    loop {
        out.push((b'A' + (num % 26) as u8) as char);
        num = num / 26 - 1;
        if num < 0 {
            break;
        }
    }
    out
}

/// Ordered merge of rows or columns across tables.
#[derive(Debug, Clone, Default)]
pub struct Ordering {
    order: Vec<Unit>,
    ignore_parent: bool,
}

impl Ordering {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, l: isize, r: isize, p: isize) {
        let p = if self.ignore_parent { -2 } else { p };
        self.order.push(Unit::new(l, r, p));
    }
    pub fn list(&self) -> &[Unit] {
        &self.order
    }
    pub fn set_list(&mut self, lst: Vec<Unit>) {
        self.order = lst;
    }
    pub fn ignore_parent(&mut self) {
        self.ignore_parent = true;
    }
}

impl std::fmt::Display for Ordering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, u) in self.order.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{u}")?;
        }
        Ok(())
    }
}

/// Sort that follows the structure of V8's `Array.prototype.sort` (TimSort:
/// natural runs extended by binary insertion sort, then merged).
///
/// The reference comparators are not always a consistent ordering, and what a
/// sort does with an inconsistent comparator depends on its algorithm, so
/// matching the reference output needs the same algorithm rather than
/// `slice::sort_by` (which may even panic on such comparators). Galloping is
/// omitted; it only changes behaviour for inconsistent comparators on arrays
/// of 64+ elements.
pub(crate) fn stable_sort_by<T: Clone>(v: &mut Vec<T>, cmp: &dyn Fn(&T, &T) -> i64) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let minrun = {
        let (mut n, mut r) = (n, 0);
        while n >= 64 {
            r |= n & 1;
            n >>= 1;
        }
        n + r
    };
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (start, len)
    let mut low = 0;
    let mut remaining = n;
    while remaining != 0 {
        let mut run = count_and_make_run(v, low, low + remaining, cmp);
        if run < minrun {
            let forced = minrun.min(remaining);
            binary_insertion_sort(v, low, low + run, low + forced, cmp);
            run = forced;
        }
        runs.push((low, run));
        merge_collapse(v, &mut runs, cmp);
        low += run;
        remaining -= run;
    }
    while runs.len() > 1 {
        let mut k = runs.len() - 2;
        if k > 0 && runs[k - 1].1 < runs[k + 1].1 {
            k -= 1;
        }
        merge_at(v, &mut runs, k, cmp);
    }
}

fn count_and_make_run<T: Clone>(v: &mut [T], low_arg: usize, high: usize, cmp: &dyn Fn(&T, &T) -> i64) -> usize {
    let low = low_arg + 1;
    if low == high {
        return 1;
    }
    let mut run = 2;
    let descending = cmp(&v[low], &v[low - 1]) < 0;
    let mut prev = low;
    for idx in low + 1..high {
        let order = cmp(&v[idx], &v[prev]);
        if descending {
            if order >= 0 {
                break;
            }
        } else if order < 0 {
            break;
        }
        prev = idx;
        run += 1;
    }
    if descending {
        v[low_arg..low_arg + run].reverse();
    }
    run
}

fn binary_insertion_sort<T: Clone>(v: &mut [T], low: usize, start_arg: usize, high: usize, cmp: &dyn Fn(&T, &T) -> i64) {
    let mut start = if low == start_arg { start_arg + 1 } else { start_arg };
    while start < high {
        let (mut left, mut right) = (low, start);
        let pivot = v[start].clone();
        while left < right {
            let mid = left + ((right - left) >> 1);
            if cmp(&pivot, &v[mid]) < 0 {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        for p in (left + 1..=start).rev() {
            v[p] = v[p - 1].clone();
        }
        v[left] = pivot;
        start += 1;
    }
}

fn merge_collapse<T: Clone>(v: &mut Vec<T>, runs: &mut Vec<(usize, usize)>, cmp: &dyn Fn(&T, &T) -> i64) {
    while runs.len() > 1 {
        let mut n = runs.len() - 2;
        let len = |i: usize| runs[i].1;
        if (n > 0 && len(n - 1) <= len(n) + len(n + 1)) || (n > 1 && len(n - 2) <= len(n) + len(n - 1)) {
            if len(n - 1) < len(n + 1) {
                n -= 1;
            }
            merge_at(v, runs, n, cmp);
        } else if len(n) <= len(n + 1) {
            merge_at(v, runs, n, cmp);
        } else {
            break;
        }
    }
}

fn merge_at<T: Clone>(v: &mut Vec<T>, runs: &mut Vec<(usize, usize)>, i: usize, cmp: &dyn Fn(&T, &T) -> i64) {
    let (a, la) = runs[i];
    let (_, lb) = runs[i + 1];
    let left: Vec<T> = v[a..a + la].to_vec();
    let right: Vec<T> = v[a + la..a + la + lb].to_vec();
    let (mut x, mut y, mut k) = (0, 0, a);
    while x < left.len() && y < right.len() {
        if cmp(&right[y], &left[x]) < 0 {
            v[k] = right[y].clone();
            y += 1;
        } else {
            v[k] = left[x].clone();
            x += 1;
        }
        k += 1;
    }
    for item in &left[x..] {
        v[k] = item.clone();
        k += 1;
    }
    for item in &right[y..] {
        v[k] = item.clone();
        k += 1;
    }
    runs[i] = (a, la + lb);
    runs.remove(i + 1);
}
