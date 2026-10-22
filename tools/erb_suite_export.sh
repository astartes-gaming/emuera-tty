#!/bin/bash
# tools/erb_suite_export.sh OUTDIR — build the ERB test suite (tests/erb,
# tests/golden, tests/save) as ordinary Emuera game folders, to run on the
# real Emuera and check that the suite's expectations are Emuera's.
#
# For each folder: put Emuera.exe (Emuera.em / EE) in it and run it; the
# test runs at once and its screen is written to emuera.log (OUTPUTLOG)
# beside the exe. Then: tools/erb_suite_compare.py OUTDIR
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: $0 OUTDIR}
mkdir -p "$out"
for f in "$root"/tests/erb/[!_]*.erb "$root"/tests/golden/*.erb "$root"/tests/save/*.erb; do
  name=$(basename "$f" .erb)
  kind=$(basename "$(dirname "$f")")
  d="$out/$kind-$name"
  rm -rf "$d"; mkdir -p "$d/CSV" "$d/ERB"
  cp "$root"/sample/CSV/* "$d/CSV/"
  if grep -q '^@SYSTEM_TITLE' "$f"; then
    # the file runs the system flow itself: the helpers without their title
    python3 -c 'import sys; s=open(sys.argv[1],encoding="utf-8").read(); print(s.replace("@SYSTEM_TITLE\nCALL TEST_MAIN\nCALL TEST_DONE\nQUIT\n","",1),end="")' "$root/tests/erb/_helpers.erb" > "$d/ERB/_HELPERS.ERB"
  else
    cp "$root/tests/erb/_helpers.erb" "$d/ERB/_HELPERS.ERB"
  fi
  cp "$root/tests/erb/_helpers.erh" "$d/ERB/_HELPERS.ERH"
  cp "$f" "$d/ERB/$(echo "$name" | tr a-z A-Z).ERB"
  [ -f "${f%.erb}.erh" ] && cp "${f%.erb}.erh" "$d/ERB/$(echo "$name" | tr a-z A-Z).ERH"
  [ -f "${f%.erb}.out" ] && cp "${f%.erb}.out" "$d/expected.out"
  [ "$kind" = save ] && cp "$root"/tests/save/*.sav "$d/" && mkdir -p "$d/expected" && mv "$d"/*.sav "$d/expected/"
  # ;@input lines: what to type, in order, if the test asks
  grep -h '^;@input ' "$f" | sed 's/^;@input //' > "$d/input.txt" || true
  [ -s "$d/input.txt" ] || rm -f "$d/input.txt"
done
cat > "$out/README.txt" <<'TXT'
emuera-tty's ERB test suite as Emuera games. In each folder: copy Emuera.exe
in, run it (type what input.txt lists, if there is one), let it close. Each
leaves emuera.log. Then, from the emuera-tty source folder:

    python3 tools/erb_suite_compare.py <this folder>

which reports any check that fails on Emuera, golden output that differs,
and (for save-*) whether Emuera's save00.sav / global.sav match ours.
TXT
echo "exported $(ls -d "$out"/*/ | wc -l) games to $out"
