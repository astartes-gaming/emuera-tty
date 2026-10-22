#!/usr/bin/env python3
"""Check the ERB test suite's results on the real Emuera.

    tools/erb_suite_compare.py OUTDIR

OUTDIR is what tools/erb_suite_export.sh made, after running Emuera.exe in
each folder. For every folder: the checks in emuera.log (up to the DONE
line) must all be OK; a golden test's log must match expected.out; a save
test's save00.sav / global.sav must match expected/ byte for byte (see
tools/savdiff.py for where they differ). Exit status 1 if anything fails.
"""
import os, sys

def log_lines(path):
    raw = open(path, "rb").read()
    for enc in ("utf-8-sig", "cp932", "utf-16"):
        try:
            text = raw.decode(enc)
            break
        except UnicodeDecodeError:
            continue
    lines = [l.rstrip() for l in text.replace("\r\n", "\n").split("\n")]
    for i, l in enumerate(lines):
        if l.startswith("DONE "):
            return lines[: i + 1], True
    return lines, False

out = sys.argv[1]
bad = 0
for name in sorted(os.listdir(out)):
    d = os.path.join(out, name)
    if not os.path.isdir(d):
        continue
    log = os.path.join(d, "emuera.log")
    if not os.path.exists(log):
        print(f"{name}: not run (no emuera.log)")
        continue
    lines, done = log_lines(log)
    problems = [l for l in lines if l.startswith("NG ")]
    if not done:
        problems.append("never reached DONE (a script error?)")
    exp = os.path.join(d, "expected.out")
    if os.path.exists(exp):
        want = [l.rstrip() for l in open(exp, encoding="utf-8").read().rstrip("\n").split("\n")]
        got = [l for l in lines]
        # (Emuera's log starts where the game's output does)
        if got != want:
            k = next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))
            problems.append(f"output differs at line {k + 1}: emuera {got[k] if k < len(got) else '<end>'!r} vs ours {want[k] if k < len(want) else '<end>'!r}")
    for sav in ("save00.sav", "global.sav"):
        e = os.path.join(d, "expected", sav)
        if os.path.exists(e):
            g = os.path.join(d, sav)
            if not os.path.exists(g):
                g = os.path.join(d, "sav", sav)
            if not os.path.exists(g):
                problems.append(f"{sav} wasn't written")
            elif open(g, "rb").read() != open(e, "rb").read():
                problems.append(f"{sav} differs from ours (tools/savdiff.py {g} {e})")
    if problems:
        bad += 1
        print(f"{name}: FAIL")
        for p in problems:
            print(f"   {p}")
    else:
        print(f"{name}: ok")
sys.exit(1 if bad else 0)
