#!/usr/bin/env python3
"""Robustness fuzzing: arbitrary (often nonsensical) tables as inputs to every
command must never panic or hang. Usage: fuzz_robust.py DAFF_BINARY [N] [SEED]"""
import os, random, subprocess, sys, tempfile

ours = sys.argv[1]
n = int(sys.argv[2]) if len(sys.argv) > 2 else 500
rnd = random.Random(int(sys.argv[3]) if len(sys.argv) > 3 else 1)
CELLS = ["", "a", "b", "1", "@@", "!", "+++", "---", "->", "-->", "...", ":", "+", "(a)", "(b)", "@:@",
         "a->b", "x-->y", "!->", "NULL", "_NULL", "0:0", "A:A", "-:3", "@x@->", "((( a ))) b /// c", "é"]
def q(c):
    return '"' + c.replace('"', '""') + '"' if any(ch in c for ch in ',"\n') else c
def table():
    w, h = rnd.randint(0, 5), rnd.randint(0, 6)
    return "\n".join(",".join(q(rnd.choice(CELLS)) for _ in range(rnd.randint(max(0, w - 1), w + 1))) for _ in range(h)) + "\n"
bad = 0
with tempfile.TemporaryDirectory() as d:
    p = [os.path.join(d, f) for f in ("p.csv", "a.csv", "b.csv")]
    for i in range(n):
        for f in p:
            open(f, "w").write(table())
        cmds = [["diff", p[1], p[2]], ["diff", p[0], p[1], p[2]], ["diff", "--all", "--index", p[1], p[2]],
                ["diff", "--id", "a", p[1], p[2]], ["diff", "--unordered", p[1], p[2]], ["patch", p[1], p[2]],
                ["merge", p[0], p[1], p[2]], ["render", p[1]], ["trim", p[1]], ["diff", "--color", p[1], p[2]],
                ["diff", "--output-format", "html", p[1], p[2]], ["diff", "--act", "update", p[1], p[2]]]
        for c in cmds:
            try:
                r = subprocess.run([ours, *c], capture_output=True, text=True, timeout=10)
                rc = r.returncode
            except subprocess.TimeoutExpired:
                rc = "timeout"
            if rc == 101 or rc == "timeout" or (isinstance(rc, int) and rc < 0):
                bad += 1
                if bad <= 5:
                    print("FAIL", rc, c[0], " ".join(x for x in c[1:] if not x.startswith(d)), "")
                    for f in p:
                        print(os.path.basename(f) + ":\n" + open(f).read())
print("%d robustness failures in %d rounds" % (bad, n))
sys.exit(1 if bad else 0)
