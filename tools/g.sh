#!/bin/bash
# tools/g.sh WORKDIR COMMAND… — send one command to a game run by
# tools/gamed.py and print the screen once it has settled (W=columns).
W_DIR=$1; shift
before=$(stat -c %Y.%N "$W_DIR/screen.txt" 2>/dev/null)
echo "$*" > "$W_DIR/game.fifo"
for _ in $(seq 1 600); do
  sleep 0.5
  now=$(stat -c %Y.%N "$W_DIR/screen.txt" 2>/dev/null)
  [ "$now" != "$before" ] && break
done
cut -c1-"${W:-150}" "$W_DIR/screen.txt"
