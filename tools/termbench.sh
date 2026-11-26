#!/bin/bash
# termbench.sh [FLAPPY_DIR] [TERMINAL…]: how fast terminals show emuera-tty.
#
# Each terminal runs, off screen (Xvfb :99; foot under a headless sway),
# at 160x45 cells with a 10 pt font:
#   text   - 300 full redraws of 44 rows of coloured text (ASCII, Japanese,
#            box drawing): redraws per second
#   flappy - 20 s of eraFlappyTengu's title (a whole-window picture redrawn
#            every frame; only terminals that show pictures): frames per second
# three times each, printing the median. EMUERA_BENCH_SYNC makes emuera-tty
# wait for the terminal to answer after each frame, so the numbers are what
# the terminal handled, not what emuera-tty could push into the pty.
#
# FLAPPY_DIR is eraFlappyTengu's folder (not part of emuera-tty; without it
# only the text test runs). Terminals: xterm konsole kitty ghostty foot
# gnome-terminal xfce4-terminal mate-terminal lxterminal qterminal (default:
# those installed). Needs Xvfb, xdotool; sway for foot. Runs one terminal at
# a time and caps the game's memory. Off-screen terminals draw without a GPU
# (Mesa's software renderer), which handicaps GPU terminals (kitty, Ghostty).
set -u
REPO=$(cd "$(dirname "$0")/.." && pwd)
EMU=${EMUERA_TTY:-$REPO/target/release/emuera-tty}
FLAPPY=""
[ $# -gt 0 ] && [ -d "$1" ] && { FLAPPY=$(cd "$1" && pwd); shift; }
W=$(mktemp -d "${TMPDIR:-/tmp}/termbench.XXXXXX")
RD=$(mktemp -d /tmp/tb.XXXX) # (short: a Wayland socket path has a length limit)
chmod 700 "$RD"
trap 'kill $XP 2>/dev/null; rm -rf "$W" "$RD"' EXIT

# the text test game
mkdir -p "$W/text/CSV" "$W/text/ERB" "$W/home/.config"
printf 'コード,1\n' > "$W/text/CSV/GameBase.csv"
cat > "$W/text/ERB/T.ERB" <<'EOF'
@SYSTEM_TITLE
#DIM F
#DIM R
FOR F, 0, 300
	REDRAW 0
	CLEARLINE LINECOUNT
	FOR R, 0, 44
		SETCOLOR (R * 37 + F * 11) % 256 * 65536 + (R * 91) % 256 * 256 + (F * 53 + 128) % 256
		PRINTFORML {F,4} {R,2} ■□■ 霊夢 魔理沙 ┏━━┓ Frame text benchmark ※※ abcdefghijklmnopqrstuvwxyz 0123456789 ━━━━ 咲夜 ★☆★
	NEXT
	RESETCOLOR
	REDRAW 1
	INPUTMOUSEKEY 1
NEXT
QUIT
EOF
[ -n "$FLAPPY" ] && cp -r "$FLAPPY" "$W/flappy"

# what runs inside the terminal: one test, then a line of timings
cat > "$W/run.sh" <<EOF
#!/bin/bash
export HOME=$W/home XDG_CONFIG_HOME=$W/home/.config EMUERA_SEED=1 EMUERA_DEBUG_FRAMES=1 EMUERA_BENCH_SYNC=1
ulimit -v 4000000
for i in \$(seq 1 30); do [ "\$(tput cols)" -ge 140 ] && break; sleep 0.1; done
t0=\$(date +%s.%N)
case \$1 in
  flappy) timeout --foreground 20 $EMU $W/flappy 2> \$2.log ;;
  text)   timeout --foreground 120 $EMU $W/text 2> \$2.log ;;
esac
echo "\$t0 \$(date +%s.%N) \$(tput cols)x\$(tput lines)" > \$2.time
EOF
chmod +x "$W/run.sh"

Xvfb :99 -screen 0 1920x1080x24 -nolisten tcp > /dev/null 2>&1 &
XP=$!
sleep 2

# one run of TEST in TERMINAL; prints "fps cols x rows backend"
once() {
	local name=$1 t=$2 out=$W/$1-$2 p
	rm -f "$out.time"
	local cmd="$W/run.sh $t $out"
	if [ "$name" = foot ]; then
		printf 'output HEADLESS-1 resolution 1280x720\ndefault_border none\n' > "$RD/sway.conf"
		(
			export XDG_RUNTIME_DIR=$RD WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1
			unset DISPLAY WAYLAND_DISPLAY
			sway -c "$RD/sway.conf" > /dev/null 2>&1 &
			sp=$!
			sleep 2
			WAYLAND_DISPLAY=$(ls "$RD" | grep -m1 '^wayland-[0-9]*$') foot -o font=monospace:size=10 $cmd > /dev/null 2>&1 &
			fp=$!
			for _ in $(seq 1 170); do [ -f "$out.time" ] && break; sleep 1; done
			kill $fp $sp 2> /dev/null
		)
	else
		(
			export DISPLAY=:99 QT_QPA_PLATFORM=xcb GDK_BACKEND=x11 NO_AT_BRIDGE=1
			unset WAYLAND_DISPLAY
			case $name in
				xterm) set -- xterm -geometry 160x45 -ti vt340 -xrm 'XTerm*numColorRegisters: 256' -fa 'DejaVu Sans Mono' -fs 10 -e $cmd ;;
				konsole) set -- dbus-run-session konsole --nofork --separate -p TerminalColumns=160 -p TerminalRows=45 -e $cmd ;;
				kitty) set -- kitty -o initial_window_width=160c -o initial_window_height=45c -o remember_window_size=no -o font_size=10 $cmd ;;
				ghostty) set -- ghostty --window-width=160 --window-height=45 --font-size=10 -e $cmd ;;
				gnome-terminal) set -- dbus-run-session gnome-terminal --wait --geometry=160x45 -- $cmd ;;
				xfce4-terminal) set -- dbus-run-session xfce4-terminal --disable-server --geometry=160x45 -x $cmd ;;
				mate-terminal) set -- dbus-run-session mate-terminal --disable-factory --geometry=160x45 -x $cmd ;;
				lxterminal) set -- lxterminal --no-remote --geometry=160x45 -e $cmd ;;
				qterminal) set -- qterminal -e "$cmd" ;;
			esac
			"$@" > /dev/null 2>&1 &
			p=$!
			if [ "$name" = qterminal ]; then
				sleep 1
				xdotool windowsize "$(xdotool search --onlyvisible --class qterminal | head -1)" 1300 780
			fi
			for _ in $(seq 1 170); do [ -f "$out.time" ] && break; sleep 1; done
			sleep 1
			kill $p 2> /dev/null
			sleep 1
			kill -9 $p 2> /dev/null
		)
	fi
	[ -f "$out.time" ] || { echo "- - -"; return; }
	local frames secs
	frames=$(grep -c '^frame ' "$out.log")
	secs=$(awk '{print $2 - $1}' "$out.time")
	awk -v f="$frames" -v s="$secs" -v t="$t" 'BEGIN { printf "%.0f ", (t == "text" ? 300 : f) / s }'
	echo "$(cut -d' ' -f3 "$out.time") $(grep -o 'graphics backend: [a-z]*' "$out.log" | cut -d' ' -f3)"
}

if [ $# -eq 0 ]; then
	for t in xterm konsole kitty ghostty foot gnome-terminal xfce4-terminal mate-terminal lxterminal qterminal; do
		command -v "$t" > /dev/null && set -- "$@" "$t"
	done
fi
printf '%-16s %-7s %8s  %-8s %s\n' terminal test median size pictures
for name in "$@"; do
	for t in text flappy; do
		[ $t = flappy ] && [ -z "$FLAPPY" ] && continue
		runs=()
		for _ in 1 2 3; do runs+=("$(once "$name" $t)"); done
		backend=$(echo "${runs[0]}" | cut -d' ' -f3)
		# (flappy only where pictures are shown: without them little is drawn)
		[ $t = flappy ] && [ "$backend" = none ] && continue
		median=$(printf '%s\n' "${runs[@]}" | cut -d' ' -f1 | sort -n | sed -n 2p)
		printf '%-16s %-7s %8s  %-8s %s\n' "$name" $t "$median/s" "$(echo "${runs[0]}" | cut -d' ' -f2)" "$backend"
	done
done
