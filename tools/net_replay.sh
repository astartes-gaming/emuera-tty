#!/usr/bin/env bash
# Replay a game on the Emuera.NET fork's script engine and on emuera-tty
# with the same inputs and random seed, and show where their text differs.
#
#   tools/net_replay.sh GAME_DIR [INPUTS_FILE] [SEED]
#
# INPUTS_FILE: one input per line, as `emuera-tty --headless` reads stdin
# (waits take none). SEED: the random seed for both (default 1). Each engine
# runs on its own throwaway copy of the game (ERB, CSV, config, setting.json,
# font, sav; resources linked), so the game folder is never written to.
#
# Needs: the .NET 10 SDK, a checkout of https://github.com/epizo12345/emuera_Fork
# (EMUERA_FORK, default ../emuera_Fork beside this repository), and
# emuera-tty built (EMUERA_TTY, default target/release/emuera-tty).
# Exit status: 0 same output, 1 different, 2 a run could not be made.
set -euo pipefail

usage() { echo "usage: $0 GAME_DIR [INPUTS_FILE] [SEED]" >&2; exit 2; }
[ $# -ge 1 ] && [ $# -le 3 ] || usage
game=$(realpath -e -- "$1") || usage
[ -d "$game/ERB" ] && [ -d "$game/CSV" ] || { echo "net_replay: $game has no ERB/ and CSV/" >&2; exit 2; }
inputs=/dev/null
if [ $# -ge 2 ]; then inputs=$(realpath -e -- "$2") || usage; fi
seed=${3:-1}
[[ $seed =~ ^-?[0-9]+$ ]] || { echo "net_replay: the seed must be a number" >&2; exit 2; }

repo=$(cd -- "$(dirname -- "$0")/.." && pwd)
export EMUERA_FORK=${EMUERA_FORK:-$repo/../emuera_Fork}
tty_bin=${EMUERA_TTY:-$repo/target/release/emuera-tty}
[ -f "$EMUERA_FORK/Web/Emuera.Web.Runtime/Emuera.Web.Runtime.csproj" ] || { echo "net_replay: no Emuera.NET fork at $EMUERA_FORK (set EMUERA_FORK)" >&2; exit 2; }
[ -x "$tty_bin" ] || { echo "net_replay: no emuera-tty at $tty_bin (cargo build --release, or set EMUERA_TTY)" >&2; exit 2; }

# the runner, built once into target/ (never into the fork's checkout)
runner=$repo/target/net_replay/bin/NetReplay/release/NetReplay.dll
if [ ! -f "$runner" ] || [ -n "$(find "$repo/tools/net_replay" -newer "$runner" -type f)" ]; then
    dotnet build "$repo/tools/net_replay/NetReplay.csproj" -c Release --artifacts-path "$repo/target/net_replay" -v quiet -nologo >&2 \
        || { echo "net_replay: building the runner failed" >&2; exit 2; }
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/net_replay.XXXXXX")
# (only ever this run's own temporary folder is removed)
trap 'case "$work" in "${TMPDIR:-/tmp}"/net_replay.*) rm -rf -- "$work" ;; esac' EXIT

# A copy of the game for one engine: what both read, and its saves.
copy_game() {
    local dst=$1
    mkdir -p "$dst"
    cp -r -- "$game/ERB" "$game/CSV" "$dst/"
    local f
    for f in emuera.config setting.json Emuera.dll; do
        if [ -f "$game/$f" ]; then cp -- "$game/$f" "$dst/"; fi
    done
    if [ -d "$game/font" ]; then cp -r -- "$game/font" "$dst/"; fi
    if [ -d "$game/sav" ]; then cp -r -- "$game/sav" "$dst/"; fi
    if [ -d "$game/resources" ]; then ln -s -- "$game/resources" "$dst/resources"; fi
}
copy_game "$work/net"
copy_game "$work/tty"

# emuera-tty capped at 4 GB of address space, as the repository's game
# tests are; .NET reserves far more address space than it uses, so the fork
# gets a 3 GB heap limit instead
set +e
DOTNET_GCHeapHardLimit=0xC0000000 dotnet "$runner" "$work/net" --seed "$seed" --inputs "$inputs" >"$work/net.out" 2>"$work/net.err"
net_status=$?
(ulimit -v 4000000; EMUERA_SEED=$seed EMUERA_LOG_STYLE=emuera "$tty_bin" "$work/tty" --headless --echo <"$inputs") >"$work/tty.out" 2>"$work/tty.err"
tty_status=$?
set -e

# what only one side prints by itself: the fork's loading line, emuera-tty's
# load summary and end-of-input notes; trailing spaces
# A rule (DRAWLINE, CUSTOMDRAWLINE) becomes `<rule>`: the fork's log
# has it as one character, emuera-tty's as wide as the window. Picture
# markers (`[img:NAME]`, emuera-tty only) go.
clean() {
    sed -e 's/[[:space:]]*$//' -e 's/ *\[img:[^]]*\]//g' -e 's/^\([-=―─]\)\1*$/<rule>/' "$1" \
        | grep -v -e '^Now Loading\.\.\.$' -e '^loaded [0-9]* files, ' -e '^warning: @SYSTEM_TITLE returned without BEGIN' || true
}
clean "$work/net.out" >"$work/net.txt"
# (--echo marks each input `> text`; Emuera's log has the text alone)
clean "$work/tty.out" | sed 's/^> //' >"$work/tty.txt"

grep -v -e 'cannot load .*resources' "$work/net.err" | sed 's/^/[net] /' >&2 || true
grep -v -e 'cannot load .*resources' -e '^loaded [0-9]* files' -e '^graphics backend' "$work/tty.err" | sed 's/^/[tty] /' >&2 || true
[ $net_status -le 1 ] || { echo "net_replay: the Emuera.NET run failed ($net_status)" >&2; exit 2; }

if diff -u --label "Emuera.NET (fork)" --label emuera-tty "$work/net.txt" "$work/tty.txt"; then
    echo "same output ($(wc -l <"$work/net.txt") lines, seed $seed)" >&2
    exit 0
fi
exit 1
