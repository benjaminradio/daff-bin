#!/usr/bin/env python3
"""Differential test: compare `daff diff` output against the reference
implementation (npm `daff`) on random tables.

Usage: [MODE=diff|patch|merge] [THREE=fraction] fuzz_oracle.py REFERENCE_DAFF_JS OUR_DAFF_BINARY [N] [SEED]

MODE=patch applies (patch3: 3-way diffs) reference-generated diffs with both implementations;
MODE=merge merges random parent/local/remote triples.
"""
import os, random, subprocess, sys, tempfile

ref, ours = sys.argv[1], sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv) > 3 else 200
seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
rnd = random.Random(seed)

WORDS = ["a", "b", "c", "x", "y", "NULL", "_NULL", "", "1", "2.5", "->", "hello world", "Q"]
OPTS = [[], ["--all"], ["--id", "c0"], ["--context", "0"], ["--unordered"], ["--index"],
        ["--all-rows"], ["--all-columns"], ["--context-columns", "0"], ["--act", "update"],
        ["--act", "insert"], ["--ignore", "c1"], ["-i"], ["-w"], ["--id", "c0", "--id", "c1"]]

def table(ncols, nrows, names):
    rows = [names[:ncols]]
    for _ in range(nrows):
        rows.append([rnd.choice(WORDS) for _ in range(ncols)])
    return rows

def mutate(rows):
    rows = [r[:] for r in rows]
    for _ in range(rnd.randint(0, 4)):
        op = rnd.choice(["upd", "del", "ins", "swap", "addcol", "delcol", "rename", "dup"])
        if op == "upd" and len(rows) > 1:
            r = rnd.randrange(1, len(rows)); c = rnd.randrange(len(rows[0]))
            rows[r][c] = rnd.choice(WORDS)
        elif op == "del" and len(rows) > 1:
            del rows[rnd.randrange(1, len(rows))]
        elif op == "ins":
            rows.insert(rnd.randrange(1, len(rows) + 1), [rnd.choice(WORDS) for _ in rows[0]])
        elif op == "swap" and len(rows) > 2:
            i, j = rnd.sample(range(1, len(rows)), 2); rows[i], rows[j] = rows[j], rows[i]
        elif op == "addcol":
            p = rnd.randint(0, len(rows[0]))
            for k, r in enumerate(rows):
                r.insert(p, "new%d" % rnd.randint(0, 9) if k == 0 else rnd.choice(WORDS))
        elif op == "delcol" and len(rows[0]) > 1:
            p = rnd.randrange(len(rows[0]))
            for r in rows: del r[p]
        elif op == "rename":
            rows[0][rnd.randrange(len(rows[0]))] = "ren%d" % rnd.randint(0, 9)
        elif op == "dup" and len(rows) > 1:
            rows.insert(rnd.randrange(1, len(rows) + 1), rows[rnd.randrange(1, len(rows))][:])
    return rows

def q(c):
    if c == "":
        return ""
    return '"' + c.replace('"', '""') + '"' if any(ch in c for ch in ',"\n') or c != c.strip() else c

def write(path, rows):
    with open(path, "w") as f:
        for r in rows:
            f.write(",".join(q(c) for c in r) + "\n")

def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr if "merge" in cmd else ""


def cmd_of(mode, files, dp):
    return ["patch", files[-2], dp] if mode == "patch" else [mode, *files]

bad = 0
with tempfile.TemporaryDirectory() as d:
    a, b, pp = os.path.join(d, "a.csv"), os.path.join(d, "b.csv"), os.path.join(d, "p.csv")
    for i in range(n):
        names = ["c0", "c1", "c2", "c3", "c4", "c5", "c6"]
        base = table(rnd.randint(1, 7), rnd.randint(0, rnd.choice([8, 40])), names)
        mode = os.environ.get("MODE", "diff")
        three3 = mode == "patch3"  # patch `a` with the 3-way diff parent/a/b
        if three3:
            mode = "patch"
        three = mode == "merge" or three3 or rnd.random() < float(os.environ.get("THREE", "0.3"))
        if three:
            write(pp, base); write(a, mutate(base)); write(b, mutate(base))
            files = [pp, a, b]
        else:
            write(a, base); write(b, mutate(base))
            files = [a, b]
        opts = rnd.choice(OPTS)
        if mode == "diff":
            r1 = run(["node", ref, "diff", *opts, *files])
            r2 = run([ours, "diff", *opts, *files])
        elif mode == "patch":
            popts = rnd.choice([[], ["--all"], ["--id", "c0"], ["--context", "0"], ["--unordered"]])
            if not three3:
                files = [files[-2], files[-1]]
            dp = os.path.join(d, "d.csv")
            with open(dp, "w") as f:
                f.write(run(["node", ref, "diff", *popts, *files])[1])
            opts = popts
            target = files[-2]
            r1 = run(["node", ref, "patch", target, dp])
            r2 = run([ours, "patch", target, dp])
        else:
            r1 = run(["node", ref, "merge", *files])
            r2 = run([ours, "merge", *files])
        if r1 != r2:
            bad += 1
            keep = os.environ.get("KEEP")
            if keep:
                import shutil
                os.makedirs(keep, exist_ok=True)
                for f in files + ([dp] if mode == "patch" else []):
                    shutil.copy(f, os.path.join(keep, "%d_%s" % (i, os.path.basename(f))))
            if bad <= 3:
                print("MISMATCH", opts, "iteration", i)
                print(*[open(f).read() for f in files], sep="\n---\n")
                if mode == "patch":
                    print("--- patch:\n" + open(dp).read())
                print("reference:\n" + r1[1], "ours:\n" + r2[1])
                if os.environ.get("DEBUG"):
                    print("ours stderr:", subprocess.run([ours, *cmd_of(mode, files, dp if mode == "patch" else None)], capture_output=True, text=True).stderr[:600])
print("%d/%d mismatches" % (bad, n))
sys.exit(1 if bad else 0)
