#!/bin/bash
# tools/game_smoke.sh GAME_DIR [STEPS] [SEED] — play a real game for a while
# with random menu choices, headless, and report script errors or crashes.
#
# The game is never touched: it runs in a temporary copy (scripts and CSV
# copied, pictures and sound linked) with its own settings, under a 4 GB
# memory cap (VERBOSE=1 shows the last screen). Each step answers with one of the buttons on screen
# (EMUERA_LIST_BUTTONS), or Enter / 0 / 1. Exit status 1 on a script error.
# Run at most two at once on a small machine.
set -e
game=$(cd "${1:?usage: $0 GAME_DIR [STEPS] [SEED]}" && pwd)
steps=${2:-300}
seed=${3:-1}
bin=${EMUERA_TTY:-$(cd "$(dirname "$0")/.." && pwd)/target/release/emuera-tty}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/game"
for d in "$game"/*; do
  case "$(basename "$d" | tr a-z A-Z)" in
    ERB|CSV) cp -r "$d" "$tmp/game/" ;;
    SAV|DAT) ;;                                   # a fresh game: no saves
    *) ln -s "$d" "$tmp/game/$(basename "$d")" ;;
  esac
done
# answers decided as the game asks: a FIFO fed by this script
mkfifo "$tmp/in"
( ulimit -v 4000000; EMUERA_SEED=$seed EMUERA_LIST_BUTTONS=1 XDG_CONFIG_HOME="$tmp/cfg" \
    "$bin" "$tmp/game" --headless < "$tmp/in" > "$tmp/out" 2>&1 ) &
pid=$!
exec 3> "$tmp/in"
RANDOM=$seed
for ((i = 0; i < steps; i++)); do
  sleep 0.05
  kill -0 $pid 2>/dev/null || break
  btns=$(grep -a '^\[buttons: ' "$tmp/out" | tail -1 | sed 's/^\[buttons: //; s/\]$//' | tr -d ' ')
  IFS=, read -ra arr <<< "$btns"
  if ((${#arr[@]} > 0 && RANDOM % 10 < 9)); then
    echo "${arr[RANDOM % ${#arr[@]}]}" >&3
  else
    case $((RANDOM % 3)) in 0) echo >&3 ;; 1) echo 0 >&3 ;; *) echo 1 >&3 ;; esac
  fi
done
exec 3>&-
wait $pid || true
if grep -aq "script error\|panicked" "$tmp/out"; then
  echo "FAIL $(basename "$game") seed $seed:"
  grep -a -A8 "script error\|panicked" "$tmp/out" | head -20
  exit 1
fi
[ -n "$VERBOSE" ] && { echo "--- last screen:"; tail -25 "$tmp/out"; }
echo "ok $(basename "$game"): $i steps, seed $seed, no script errors"
