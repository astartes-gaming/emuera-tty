#!/usr/bin/env python3
"""Which of Emuera's built-in functions and statements the ERB test suite
uses: tests/emuera_builtins.txt (Emuera's whole list) against the words
in tests/erb, tests/golden, tests/save and the sample game's self-test.

    tools/erb_coverage.py           summary and the names not used yet
    tools/erb_coverage.py --used    the names used

A name being used doesn't mean every case of it is checked: it is the
list of what no test touches at all.
"""
import os, re, sys

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
names = [l.strip() for l in open(os.path.join(root, "tests/emuera_builtins.txt"), encoding="utf-8") if l.strip() and not l.startswith("#")]
words = set()
for d in ("tests/erb", "tests/golden", "tests/save", "sample/ERB"):
    for f in os.listdir(os.path.join(root, d)):
        if f.lower().endswith((".erb", ".erh")):
            text = open(os.path.join(root, d, f), encoding="utf-8-sig", errors="replace").read()
            # code only: comments and ;@input lines left out
            text = "\n".join(l.split(";")[0] for l in text.splitlines())
            words |= set(re.findall(r"[A-Z_][A-Z_0-9]*", text.upper()))
used = [n for n in names if n in words]
missing = [n for n in names if n not in words]
if "--used" in sys.argv:
    print("\n".join(used))
    sys.exit()
print(f"{len(used)} of {len(names)} Emuera built-ins used by the suite ({100 * len(used) // len(names)}%)")
print("not used yet:")
line = ""
for n in missing:
    if len(line) + len(n) > 100:
        print(" ", line)
        line = ""
    line += n + " "
if line:
    print(" ", line)
