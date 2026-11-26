## Terminal support

Text works everywhere. Images and Japanese/Chinese text depend on the
terminal:

| Terminal | Images | Notes |
|---|---|---|
| **kitty**, **Ghostty**, **Konsole** | yes (kitty graphics) | best choice |
| **iTerm2** (macOS), **WezTerm** | yes (iTerm2 inline images) | WezTerm's kitty graphics flicker, so auto uses iTerm images there |
| **foot**, xterm (`-ti vt340`), mlterm, yaft, Windows Terminal 1.22+ | yes (sixel) | |
| macOS Terminal.app, GNOME Terminal, Alacritty, others | `░` placeholders | fine for playing |
| Linux console under **fbterm** | yes (framebuffer) | needs the `video` group |
| bare Linux console (no fbterm) | yes (framebuffer) | can't show Japanese/Chinese text |
| illumos / Solaris console (`sun-color`) | no | can't show Japanese/Chinese text (padded to Emuera's columns), no mouse |
| real VT100 / VT220 / VT52 / ADM-3A / Wyse…, local or on a **serial line** | no | ASCII, see below |
| **tmux** inside kitty or Ghostty | yes (kitty Unicode placeholders) | needs `set -g allow-passthrough all` |
| **tmux** inside a sixel terminal | yes (tmux draws the sixel) | tmux built with sixel |
| Termux (Android) | no | `iterm`/`sixel` floods the terminal with data, `kitty` renders nothing, Termux in general doesn't support in-terminal image rendering |
| KMSCON | not yet (under consideration by KMSCON devs, refer to [KMSCON limitations](Pictures-and-Graphics#kmscon-limitations)) | supports full unicode glyphs, including CJK ones, but **does not support images (yet)** |
| Android to other device (ADB) | depends on terminal used, conditions above | `fb` not supported for framebuffer consoles, has an issue that floods the input bar when moving the mouse around |
| Android to other device (SSH) | depends on terminal used, conditions above | `fb` not supported for framebuffer consoles |
| Android recovery shell (builtins in TWRP, OrangeFox, etc...) | no | images never supported, Japanese/Chinese text untested, very finicky in general (and why are you even running it there of all places?) |

## Tips for any terminal

- Install a font with **CJK** glyphs (e.g. Noto Sans CJK) so Japanese
  names and symbols show up. For resemblance to Emuera, install the
  MS Gothic font.
- Generally speaking, a terminal theme with a black background works best
  for Era games, as with Emuera. While other background colors are fine,
  some parts of a game may be hard to see, or images with a black background
  fail to blend into the terminal.
- **Maps** (box drawing, `■`, `※`…) are two columns per symbol in Emuera.
  emuera-tty pads them to two columns; in GNOME Terminal you can instead set
  *Preferences → profile → Compatibility → Ambiguous-width characters:
  Wide* (WezTerm: `treat_east_asian_ambiguous_width_as_wide = true`).
  Other terminals get padding, which keeps maps aligned but less solid.
- **Other scripts:** combining marks (Hindi vowel signs and virama, accents
  written as separate characters, emoji variation selectors and joiners)
  are sent with the character they belong to, and the terminal composes
  them. Right-to-left text (Arabic, Hebrew) is sent in the order it was
  written: terminals with bidirectional support (mlterm, recent VTE-based
  ones with it turned on, partly Konsole) show it right to left, and others
  show its letters left to right.
- Make the window **wide**: many games lay out menus for ~150 columns.
  If a game warns that the window is too narrow, shrink the font or
  widen the window.
- `--gfx none` turns images off; `--gfx kitty|iterm|sixel|fb` forces a backend
  if auto-detection guesses wrong.
- **tmux**: with kitty or Ghostty outside it, put `set -g allow-passthrough
  all` in `~/.tmux.conf` (then `tmux source ~/.tmux.conf`). (`on` works
  too, but tmux then drops a picture sent while the pane isn't showing,
  say during a loading screen while you are in another window, and it
  stays blank until the game draws a new one.) Without it, in kitty or
  Ghostty, emuera-tty stops at start and says what to set; `--gfx none`
  plays without pictures instead. Pictures are
  then written as kitty's Unicode placeholders: text tmux keeps in its
  pane, so they follow splits, window switches and redraws. A picture
  meant to sit behind text isn't shown there, and crops are to whole
  cells. Without the setting there are no pictures (the startup message
  says so). byobu runs on tmux by default, so the same applies; GNU
  screen can't pass pictures through, so there are none there.

## Performance

How fast terminals keep up with emuera-tty, measured with
`tools/termbench.sh`. Ordinary era games redraw rarely, so any terminal
here is fast enough for them. The difference shows in animated games
and in fast scrolling through long logs.

There are two tests:

- **Text:** 300 full redraws of a 160×44 screen of coloured text that mixes
  ASCII, Japanese and box drawing, like a busy era screen.
- **Pictures:** the title screen of eraFlappyTengu (by Mr Pops Alot), a
  game that redraws a whole-window picture every frame, played for 20 s.
  Only terminals that show pictures take part; the others get
  placeholders, which barely change, so a number would mean nothing.

Each is counted two ways:

- **Throughput:** how fast the terminal takes in what emuera-tty sends.
  The pty between them holds little, so emuera-tty can't run far ahead.
- **Answered:** emuera-tty waits after each frame until the terminal has
  answered a query sent behind it (`EMUERA_BENCH_SYNC`), so each frame
  counts only once the terminal has handled it. Terminals that answer at
  their own pace (once per screen refresh, or when the compositor is
  ready) score lower here.

Numbers are the median of three runs, each terminal at 160×45 cells
(kitty 156×44, Konsole 159×49, qterminal 143×38: they wouldn't take that
size) with a 10 pt font.

### On a desktop

KDE Plasma 6.3 on Wayland, AMD Ryzen 5 5500U with its integrated Radeon
graphics, 7 GB of RAM, Debian 13, a 60 Hz screen. Frames beyond 60 a
second are never shown on such a screen, so above that a terminal is as
smooth as it can be.

| Terminal | Text throughput | Text answered | Pictures throughput | Pictures answered | Pictures |
|---|---:|---:|---:|---:|---|
| kitty 0.41 | 226/s | 119/s | 101/s | 43/s | kitty graphics |
| foot 1.21 | 213/s | 193/s | 36/s | 28/s | sixel |
| Ghostty 1.3 | 209/s | 182/s | 42/s | 29/s | kitty graphics |
| xterm 398 (XWayland) | 141/s | 116/s | 57/s | 43/s | sixel |
| qterminal 2.1 | 136/s | 107/s | — | — | none |
| Konsole 25.04 | 111/s | 90/s | 71/s | 43/s | kitty graphics |
| GNOME Terminal 3.56, Xfce Terminal 1.1, MATE Terminal 1.26, LXTerminal 0.4 (VTE 0.80) | 51–57/s | 13–16/s | — | — | none |

What it shows:

- **Text:** kitty, foot and Ghostty are the quickest, all far beyond
  what a 60 Hz screen can show. Every terminal here keeps up with text
  at screen rate except the VTE ones, which manage just under it.
- **Pictures:** kitty is the quickest. It takes in 100 frames a second
  and handles about 43 of them fully, as do Konsole and xterm.
  Ghostty and foot manage about 30, enough to play eraFlappyTengu
  but visibly less smooth.
- **VTE terminals** (GNOME Terminal, Xfce, MATE, LXTerminal) take text in
  at about 55 redraws a second. They answer a query only about every
  60 ms, because they wait for the compositor first, so their
  "answered" numbers are low. That doesn't slow ordinary play. They show
  no pictures.
- **sixel (foot, xterm)** sends a whole picture for every frame. kitty
  graphics sends each distinct picture once and then only moves it, which
  helps kitty and Konsole.

### Off screen, without a GPU

The same tests on a hidden X server (Xvfb; a headless sway for foot),
answered counts only, on the same machine. Here everything draws with
Mesa's software renderer, which handicaps the GPU terminals (kitty,
Ghostty), and no compositor paces the VTE terminals. Konsole ran at
155×42, Ghostty at 158×41 and foot at 160×42.

| Terminal | Text answered | Pictures answered | Pictures |
|---|---:|---:|---|
| Konsole 25.04 | 275/s | 102/s | kitty graphics |
| Ghostty 1.3 | 268/s | 91/s | kitty graphics |
| foot 1.21 | 261/s | 26/s | sixel |
| xterm 398 | 254/s | 66/s | sixel |
| qterminal 2.1 | 180/s | — | none |
| kitty 0.41 | 62/s | 46/s | kitty graphics |
| GNOME Terminal, Xfce, MATE, LXTerminal (VTE 0.80) | 57–58/s | — | none |

Off screen, kitty and VTE answer once per refresh (about 13 ms), so
they top out around 60–75 frames a second. Lowering kitty's
`repaint_delay` and `input_delay` made it slower there, since each extra
repaint costs a software-rendered frame.

### Caveats

- **Variation:** results moved by up to a third between sessions, and on
  the desktop with whatever else the machine was doing.
- **Pictures throughput** in kitty-graphics terminals may run ahead of
  what is drawn: a terminal can take a picture in before it has decoded
  it. The answered count doesn't have that problem.
- **Not measured:** tmux, GNU screen and the Linux console.

### Measuring your own terminals

```sh
tools/termbench.sh --desktop path/to/eraflappytengu
tools/termbench.sh --desktop --nosync path/to/eraflappytengu kitty foot
```

`--desktop` opens the windows on your desktop, one at a time, each for
the length of its test. Without it they run on a hidden X server.
`--nosync` measures throughput; without it, answered frames. Without
terminal names it measures every terminal in the tables above that is
installed. Without the game folder it runs only the text test. It needs
Xvfb and xdotool for the hidden server, and sway for foot there. It caps
the game's memory.

## On the Linux console

For a text console (Ctrl-Alt-F3 etc.):

- Run inside fbterm for CJK text: `fbterm -- emuera-tty`.
- Images draw straight to `/dev/fb0`, which needs the `video` group:
  `sudo usermod -aG video $USER`, then log in again.
- The mouse needs either **gpm** (`sudo apt install gpm`) or read access
  to `/dev/input/mice` (the `input` group). Be aware the `input` group can
  read every input device, keyboards included; gpm is the safer choice.
  Without either, the input line says so (and so does the terminal after
  quitting). emuera-tty draws the pointer itself; moving it repaints only
  the cells it crosses, so images don't flicker.
- The console has no double-width characters, so emuera-tty notices that
  at startup and pads kanji, kana and full-width symbols itself, which
  keeps menus and maps lined up (`--ambiguous single`, or `EMUERA_AMBIGUOUS=single`, forces this).
- Its font has a few hundred glyphs, and the rest show as a diamond.
  emuera-tty reads which characters the font has and draws the missing
  ones as the nearest it has: `【】「」` as `[ ]`, full-width `（Ａ１）` as
  `(A1)`, `ー` as `─`, `★※` as `*`, and so on, in the same width. Kana and
  kanji have no stand-in; use fbterm for those.
- The console has 16 colours. Dim colours keep their hue (dark teal
  walls stay cyan, brown floors brown) and greys step through black, dark
  grey, light grey and white, so coloured maps stay readable.

## On a retro or dumb terminal

Real hardware, or an emulator like cool-retro-term or `xterm -ti vt52`:

- emuera-tty drives the terminal from its **terminfo** entry (cursor
  addressing, clearing, bold / underline, colours if any, the line-drawing
  set, its keys), so anything the system describes works: `infocmp NAME`
  shows whether yours is there (the `ncurses-term` package has most old
  ones). Terminals whose cursor control isn't ANSI (`vt52`, `adm3a`,
  `wy50`…) and `dumb` are picked up from `$TERM` by themselves; for an ANSI
  one that can't show UTF-8 or colour (a real VT100 / VT220) add
  `--retro`.
- Output is plain ASCII. Box drawing uses the terminal's line-drawing
  characters when it has them (`+`, `-`, `|` otherwise), kana are written
  in romaji, kanji as `??`, and full-width letters as a letter and a space,
  so menus and maps keep their columns. Images are left out.
- Only what changed since the last screen is sent: a typical menu is a
  few hundred bytes (under a second at 9600 baud), a full map screen about
  4 KB (4 seconds at 9600; slower lines work, but maps take a while).
- No cursor control at all (`dumb`, a Teletype, `--retro=line`): the game
  is printed line by line as it scrolls, and you type answers after `>`.
- Keys: Return, Backspace / Delete, Ctrl-U clears the line, the cursor
  keys or Ctrl-P / Ctrl-N scroll a line, Page Up / Down or Ctrl-B / Ctrl-F a
  page, Ctrl-L repaints the screen (after line noise), Ctrl-C quits.

## On a serial line

From the computer the terminal is wired to:

```sh
emuera-tty /path/to/game --serial /dev/ttyUSB0 --term vt220 --baud 9600
```

`--serial-format 7E1` (default `8N1`) and `--flow rts` or `none`
(default XON/XOFF) match the terminal's setup; `--size 132x24` if it isn't
the terminfo size. The serial port needs the `dialout` group
(`sudo usermod -aG dialout $USER`). Messages and warnings stay on the
computer's own screen; the game quits when the line hangs up.

## Mouse

| Where | How |
|---|---|
| xterm, kitty, WezTerm, foot, Ghostty, Alacritty, GNOME/KDE terminals, tmux | SGR (1006) mouse reports |
| urxvt and other 1015 terminals | urxvt (1015) reports |
| older terminals | legacy X10 reports |
| Linux console / fbterm with **gpm** running | connects to gpm (`/dev/gpmctl`) as a client (the console's own mouse reports stay off, or gpm would send them clicks only), draws its own pointer |
| Linux console / fbterm **without** gpm | reads `/dev/input/mice` directly (wheel included), draws its own pointer |
| **KMSCON** | draws its own mouse cursor; clicks work, but only reports motion while a mouse button is held (left/right/middle) — no hover/motion reports otherwise, so tooltips, button highlighting, and picture menus relying on hover will not function |

Modifier keys held during a click are ignored. On the console, gpm needs
nothing else; the `/dev/input/mice` fallback needs read access to the
device, i.e. the `input` group — note that it can read *all* input
devices, keyboards included. `EMUERA_MOUSE=mice` forces the console mouse
anywhere, `EMUERA_MOUSE=off` (or `term`) disables it. The pointer on the
console is a blinking inverted cell that emuera-tty draws (gpm's own
would be wiped out by every redraw). If the mouse misbehaves there,
`EMUERA_MOUSE_LOG=mouse.log emuera-tty` writes what it does (where the
events come from, each event, where the pointer was drawn) for a bug
report.

### No mouse?

*Keyboard pointer* in the settings (`auto` turns it on by itself on the
Linux console when no mouse can be found, and on the Hurd's console) puts a pointer on the screen
that the **cursor keys** move a cell at a time; **Insert** clicks under it,
and so does **Shift+Enter** where the terminal sends it apart from Enter
(Konsole, xterm; not the Linux console). Enter keeps its own job, and
PgUp / PgDn still scroll. Hovering shows tooltips and the button's number
as with a mouse, and picture menus (pops-tw's title) take the clicks too.
(A key held alone, such as right Shift, can't be used: terminals send
nothing for it.)
