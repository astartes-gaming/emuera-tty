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

| Terminal | Text redraws/s | Picture frames/s (eraFlappyTengu) | Pictures |
|---|---:|---:|---|
| Konsole 25.04 | 275 | 102 | kitty graphics |
| Ghostty 1.3 | 268 | 91 | kitty graphics |
| foot 1.21 | 261 | 26 | sixel |
| xterm 398 | 254 | 66 | sixel |
| qterminal 2.1 | 180 | — | none |
| kitty 0.41 | 62 | 46 | kitty graphics |
| GNOME Terminal 3.56, Xfce Terminal 1.1, MATE Terminal 1.26, LXTerminal 0.4 (VTE 0.80) | 57–58 | — | none |

What the tests do:

- **Text:** 300 full redraws of a 160×44 screen of coloured text that mixes
  ASCII, Japanese and box drawing, like a busy era screen.
- **Pictures:** the title screen of eraFlappyTengu (by Mr Pops Alot), a
  game that redraws a whole-window picture every frame, played for 20 s.
  Only terminals that show pictures take part; the others get
  placeholders, which barely change, so a number would mean nothing.
- **How it counts:** emuera-tty waits after each frame until the terminal
  has answered a query sent behind it (`EMUERA_BENCH_SYNC`), so a frame
  counts only once the terminal has handled it. Without the wait, emuera-tty
  writes up to about 140 picture frames a second into a terminal that
  can't show them that fast.
- **Setup:** median of three runs, each terminal at 160×45 cells (Konsole
  155×42, Ghostty 158×41, foot 160×42, qterminal 143×38: they wouldn't
  take that size) with a 10 pt font. An AMD Ryzen 5 5500U with 7 GB of
  RAM, Debian 13.

Read the numbers as a comparison, not as what your desktop will show:

- **No graphics card:** the terminals ran off screen (Xvfb, and a headless
  sway for foot), where they draw with Mesa's software renderer.
  GPU-drawn terminals (kitty, Ghostty) pay most for that and should do
  better on a real desktop.
- **Variation:** results moved by up to a third between sessions on the
  same machine; xterm's text test gave 173/s in another session.

What they show:

- **Frame pacing caps some terminals:** kitty and the VTE terminals
  (GNOME Terminal, Xfce, MATE, LXTerminal) answer once per screen refresh,
  about every 13 ms, so they handle at most 60–75 frames a second however
  little there is to draw. That's smooth enough for play. Lowering kitty's
  `repaint_delay` and `input_delay` made it slower here, since each extra
  repaint costs a software-rendered frame.
- **The quickest:** Konsole, Ghostty, foot and xterm handle a frame as
  soon as it arrives. Konsole and Ghostty are also the fastest with
  pictures, through kitty graphics.
- **Sixel is slower for animation:** foot and xterm show pictures as
  sixel, which sends a whole picture for every frame. kitty graphics
  sends each distinct picture once and then only moves it.

To measure your own terminals:

```sh
tools/termbench.sh path/to/eraflappytengu xterm konsole
```

Without terminal names it measures every one of the list above that is
installed. Without the game folder it runs only the text test. It needs
Xvfb and xdotool, and sway for foot. It runs one terminal at a time and
caps the game's memory.

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
