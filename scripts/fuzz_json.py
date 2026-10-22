#!/usr/bin/env python3
"""Differential test for JSON/NDJSON input and output against the reference.

Usage: fuzz_json.py REFERENCE_DAFF_JS OUR_DAFF_BINARY [N] [SEED]
"""
import json, os, random, subprocess, sys, tempfile

ref, ours = sys.argv[1], sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv) > 3 else 200
rnd = random.Random(int(sys.argv[4]) if len(sys.argv) > 4 else 1)

def val(depth=0):
    r = rnd.random()
    if r < 0.2: return rnd.randint(0, 5)
    if r < 0.3: return rnd.choice([0.5, 1.25, -3.5, 1e21, 1e-7, 123456789.125, 0.1])
    if r < 0.55: return rnd.choice(["a", "b", "x y", "", "é", "NULL", "null", "->", 'q"uote', "tab\there", "new\nline", " ", "\u0001", "\b\f\r"])
    if r < 0.65: return rnd.choice([True, False])
    if r < 0.75: return None
    if depth < 2 and r < 0.88: return [val(depth + 1) for _ in range(rnd.randint(0, 3))]
    if depth < 2: return {rnd.choice(["k", "j", "2", "1", "z"]): val(depth + 1) for _ in range(rnd.randint(0, 3))}
    return 1

def table(w, h):
    cols = ["c%d" % i for i in range(w)]
    rows = [dict(zip(cols, cols))]
    for _ in range(h):
        rows.append({c: val() for c in cols})
    return cols, rows

def mutate(cols, rows):
    rows = [dict(r) for r in rows]
    for _ in range(rnd.randint(0, 4)):
        op = rnd.choice(["upd", "del", "ins", "swap"])
        if op == "upd" and len(rows) > 1:
            rows[rnd.randrange(1, len(rows))][rnd.choice(cols)] = val()
        elif op == "del" and len(rows) > 1:
            del rows[rnd.randrange(1, len(rows))]
        elif op == "ins":
            rows.insert(rnd.randrange(1, len(rows) + 1), {c: val() for c in cols})
        elif op == "swap" and len(rows) > 2:
            i, j = rnd.sample(range(1, len(rows)), 2); rows[i], rows[j] = rows[j], rows[i]
    return cols, rows

def write_json(path, cols, rows):
    with open(path, "w") as f:
        json.dump({"t": {"columns": cols, "rows": rows}}, f, ensure_ascii=False)

def write_ndjson(path, cols, rows):
    with open(path, "w") as f:
        for r in rows[1:]:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
    return p.returncode, p.stdout

def multi(names_a, names_b):
    """Two multi-table files; tables with the same name are related."""
    def tbl(cols, nrows):
        return {"columns": cols, "rows": [{c: val() for c in cols} for _ in range(nrows)]}
    shared = {n: ["c%d" % i for i in range(rnd.randint(1, 4))] for n in set(names_a) | set(names_b)}
    def side(names, other=None):
        tabs = {}
        for n in names:
            cols = list(shared[n])
            if other and n in other and rnd.random() < 0.7:
                src = other[n]
                rows = [dict(r) for r in src["rows"]]
                for _ in range(rnd.randint(0, 3)):
                    if rows and rnd.random() < 0.5:
                        rows[rnd.randrange(len(rows))][rnd.choice(cols)] = val()
                    elif rows and rnd.random() < 0.5:
                        del rows[rnd.randrange(len(rows))]
                    else:
                        rows.insert(rnd.randrange(len(rows) + 1), {c: val() for c in cols})
                tabs[n] = {"columns": cols, "rows": rows}
            else:
                tabs[n] = tbl(cols, rnd.randint(0, 4))
        return tabs
    ta = side(names_a)
    tb = side(names_b, ta)
    return ({"names": names_a, "tables": ta}, {"names": names_b, "tables": tb})

bad = 0
with tempfile.TemporaryDirectory() as d:
    for i in range(n):
        if rnd.random() < 0.35:
            pool = ["a", "b", "c", "d"]
            names_a = rnd.sample(pool, rnd.randint(1, 3))
            names_b = names_a if rnd.random() < 0.8 else rnd.sample(pool, rnd.randint(1, 3))
            ja, jb = multi(names_a, names_b)
            a, b = os.path.join(d, "a.json"), os.path.join(d, "b.json")
            json.dump(ja, open(a, "w"), ensure_ascii=False)
            json.dump(jb, open(b, "w"), ensure_ascii=False)
            cmd = rnd.choice([
                ["diff", a, b], ["diff", "--output-format", "csv", a, b], ["diff", "--output-format", "ndjson", a, b],
                ["diff", "--all", a, b], ["diff", "--table", rnd.choice(pool), a, b], ["diff", "--index", a, b],
                ["diff", "--id", "c0", a, b], ["diff", "--output-format", "csv", "--ordered", a, b],
            ])
            r1, r2 = run(["node", ref, *cmd]), run([ours, *cmd])
            if r1 != r2:
                bad += 1
                if bad <= 2:
                    print("MISMATCH", cmd)
                    print(open(a).read(), "---", open(b).read())
                    print("reference:", r1[0], r1[1][:1500], "\nours:", r2[0], r2[1][:1500])
            continue
        cols, rows = table(rnd.randint(1, 5), rnd.randint(1, 7))
        cols2, rows2 = mutate(cols, rows)
        kind = rnd.choice(["json", "ndjson"])
        a, b = os.path.join(d, "a." + kind), os.path.join(d, "b." + kind)
        (write_json if kind == "json" else write_ndjson)(a, cols, rows)
        (write_json if kind == "json" else write_ndjson)(b, cols2, rows2)
        cmd = rnd.choice([
            ["diff", a, b],
            ["diff", "--unordered", a, b],
            ["diff", "--all", a, b],
            ["diff", "--index", a, b],
            ["diff", "--output-format", "csv", a, b],
            ["diff", "--output-format", "ndjson", a, b],
            ["diff", "--id", "c0", a, b],
            ["copy", a],
            ["copy", "--output-format", "csv", a],
            ["copy", "--output-format", "ndjson", a],
            ["copy", "--output-format", "json", a],
        ])
        r1, r2 = run(["node", ref, *cmd]), run([ours, *cmd])
        if r1 != r2:
            bad += 1
            if bad <= 2:
                print("MISMATCH", cmd)
                print(open(a).read(), "---", open(b).read())
                print("reference:\n" + r1[1], "\nours:\n" + r2[1])
print("%d/%d mismatches" % (bad, n))
sys.exit(1 if bad else 0)
