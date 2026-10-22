#!/usr/bin/env python3
"""Property tests of intended behaviour (no reference needed):

  patch:  patch(a, diff(a, b)) == b
  merge:  merge(a, a, b) == b   and   merge(a, b, a) == b   (parent, local, remote)
  noop:   diff(a, a) has no changes

Usage: prop_roundtrip.py DAFF_BINARY [N] [SEED] [pool=plain|nulls|unique|unique-nulls]
Prints a few minimal failing cases per property.
"""
import os, random, subprocess, sys, tempfile

ours = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 300
rnd = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
pool = sys.argv[4] if len(sys.argv) > 4 else "plain"
WORDS = ["a", "b", "c", "x", "y", "1", "2.5", "hello world", "Q", "", "->", "a,b", 'q"t']
if pool.endswith("nulls"):
    WORDS += ["NULL", "_NULL", "__NULL"]

def q(c):
    if c == "":
        return ""
    return '"' + c.replace('"', '""') + '"' if any(ch in c for ch in ',"\n') or c != c.strip() else c

def write(path, rows):
    with open(path, "w") as f:
        for r in rows:
            f.write(",".join(q(c) for c in r) + "\n")

UNIQUE = pool.startswith("unique")
counter = [0]

def fresh_name(prefix):
    counter[0] += 1
    return "%s%d" % (prefix, counter[0])

def fresh():
    counter[0] += 1
    return "id%d" % counter[0]

def row(w):
    r = [rnd.choice(WORDS) for _ in range(w)]
    if UNIQUE:
        r[0] = fresh()
    return r

def table(w, h):
    w = max(w, 2) if UNIQUE else w
    return [["c%d" % i for i in range(w)]] + [row(w) for _ in range(h)]

def mutate(rows, kinds):
    rows = [r[:] for r in rows]
    for _ in range(rnd.randint(1, 3)):
        op = rnd.choice(kinds)
        if op == "upd" and len(rows) > 1:
            rows[rnd.randrange(1, len(rows))][rnd.randrange(len(rows[0]))] = rnd.choice(WORDS)
        elif op == "del" and len(rows) > 1:
            del rows[rnd.randrange(1, len(rows))]
        elif op == "ins":
            nr = [rnd.choice(WORDS) for _ in rows[0]]
            if UNIQUE:
                nr[0] = fresh()
            rows.insert(rnd.randrange(1, len(rows) + 1), nr)
        elif op == "swap" and len(rows) > 2:
            i, j = rnd.sample(range(1, len(rows)), 2); rows[i], rows[j] = rows[j], rows[i]
        elif op == "addcol":
            p = rnd.randint(0, len(rows[0]))
            for k, r in enumerate(rows):
                r.insert(p, fresh_name("new") if k == 0 else rnd.choice(WORDS))
        elif op == "delcol" and len(rows[0]) > 2:
            p = rnd.randrange(len(rows[0]))
            for r in rows: del r[p]
        elif op == "rename":
            rows[0][rnd.randrange(len(rows[0]))] = fresh_name("ren")
        elif op == "movecol" and len(rows[0]) > 1:
            i, j = rnd.sample(range(len(rows[0])), 2)
            for r in rows: r.insert(j, r.pop(i))
    return rows

import csv, io

def norm(txt):
    """Treat NULL and the empty string as the same value."""
    return [["" if c == "NULL" else c for c in r] for r in csv.reader(io.StringIO(txt))]

def run(*cmd):
    p = subprocess.run([ours, *cmd], capture_output=True, text=True, timeout=20)
    return p.returncode, p.stdout, p.stderr

def read(path):
    return open(path).read()

KINDS = {
    "rows": ["upd", "del", "ins", "swap"],
    "cols": ["addcol", "delcol", "rename", "movecol"],
    "all": ["upd", "del", "ins", "swap", "addcol", "delcol", "rename", "movecol"],
}
fails = {}
with tempfile.TemporaryDirectory() as d:
    a, b, dp, out = (os.path.join(d, x) for x in ("a.csv", "b.csv", "d.csv", "out.csv"))
    for i in range(n):
        kind = rnd.choice(list(KINDS))
        base = table(rnd.randint(1, 5), rnd.randint(0, 7))
        other = mutate(base, KINDS[kind])
        write(a, base); write(b, other)
        for opts in ([], ["--id", "c0"], ["--all"]):
            d_rc, d_txt, _ = run("diff", *opts, a, b)
            open(dp, "w").write(d_txt)
            rc, p_txt, _ = run("patch", a, dp)
            want = run("trim", b)[1]
            # Without --all the diff cannot show rows it omits, so values of an
            # added column in those rows are unknowable: null and blank match.
            eq = norm(p_txt) == norm(want) if (os.environ.get("NULLEQ") or "--all" not in opts) and kind != "rows" else p_txt == want
            if not eq:
                key = ("patch", kind, " ".join(opts))
                fails.setdefault(key, []).append((read(a), read(b), d_txt, p_txt, want))
        rc, m_txt, _ = run("merge", a, a, b)
        want = run("trim", b)[1]
        if norm(m_txt) != norm(want):
            fails.setdefault(("merge a a b", kind, ""), []).append((read(a), read(b), "", m_txt, want))
        rc, m_txt, _ = run("merge", a, b, a)  # remote unchanged: local (b) wins
        if norm(m_txt) != norm(want):
            fails.setdefault(("merge a b a", kind, ""), []).append((read(a), read(b), "", m_txt, want))
        t = run("diff", a, a)[1]
        if t.count("\n") > 1 or "->" in t:
            fails.setdefault(("noop", "", ""), []).append((read(a), read(a), t, "", ""))

total = sum(len(v) for v in fails.values())
for key, cases in sorted(fails.items()):
    print("FAIL %-14s %-5s %-12s %d/%d" % (key[0], key[1], key[2], len(cases), n))
    cases.sort(key=lambda c: len(c[0]) + len(c[1]))
    for a_t, b_t, d_t, got, want in cases[:int(os.environ.get("SHOW", "1"))]:
        print("  a:\n" + a_t + "  b:\n" + b_t + "  diff:\n" + d_t + "  got:\n" + got + "  want:\n" + want)
print("%d failing checks" % total)
