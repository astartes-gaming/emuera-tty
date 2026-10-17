# emuera-tty

> **⚠️ Read this first: this is a vibecoded proof of concept.**
>
> This project was made by a self-described dumbass who doesn't really know
> what they're doing, by asking an AI (Claude) to write it. Nearly all of the
> code was AI-generated, reviewed only by running the game and seeing what
> breaks. It is not a serious or complete Emuera port, nobody has audited it,
> and it will mis-render, misbehave or crash on plenty of scripts.
>
> - Expect bugs. Keep a copy of save files you care about.
> - It is not affiliated with Emuera, Emuera-EM/EE, eratohoTW or AnonTW.
>   Where behaviour was copied, it was by reading
>   [EvilMask's emuera.em](https://gitlab.com/EvilMask/emuera.em) source for
>   reference; no code from it is included.
> - No game files are included. You need your own copy of the game.
>
> If it works for you anyway: nice. If it doesn't: that's the expected
> outcome of a proof of concept.

A from-scratch reimplementation of the **Emuera** ERB engine in Rust, aimed at
running **eratohoTW** in a terminal: the Linux framebuffer console (fbcon /
fbterm) and graphics-capable terminals (kitty, WezTerm, Ghostty, foot, xterm
with sixel, yaft…).

Status: **proof of concept**. The language core, game flow, save/load, the
terminal UI and all three image backends run. AnonTW (eratohoTW, English)
gets through new-game setup to the in-game screen, where basic commands
(resting, moving between rooms, menus) work; deeper gameplay is largely
untested. Large parts of the Emuera command set are still missing (see
*Roadmap*).

# Guide

## 1. What you need

- **Linux** (other Unix-likes may work; Windows does not).
- A **terminal**. Any modern one works for text; see
  [Picking a terminal](#5-picking-a-terminal) for pictures and Japanese text.
- **Rust** to build it: install with [rustup](https://rustup.rs)
  (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`), then
  open a new terminal so `cargo` is on your `PATH`.
- **git**, to download the source and for `--upgrade`.
- **An Emuera game**, downloaded from the game's own project page. Either
  keep the downloaded archive (`.zip`, `.tar.gz`, `.7z`, …) as it is, or
  unpack it anywhere: what emuera-tty plays is the folder containing
  `CSV/` and `ERB/` (the `Emuera.exe` next to them isn't used).

## 2. Install

```sh
git clone https://github.com/astartes-gaming/emuera-tty.git
cd emuera-tty
cargo build --release
./target/release/emuera-tty --install
```

`--install` copies the program into a folder on your `PATH`, so
`emuera-tty` runs from any terminal: the one already holding an installed
copy, else your personal program folder (`$XDG_BIN_HOME`, `~/.local/bin`
or `~/bin`, whichever your PATH has), else any other folder on PATH you
can write to (tool folders like `~/.cargo/bin` are skipped). No root
needed. Only if none of those exist does it fall back to `~/.local/bin`
and tell you to add that to `PATH`, with this line in `~/.bashrc` (or
`~/.profile`), then open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

## 3. First run: choose your game

```sh
emuera-tty
```

The first time, a **folder picker** opens in your home folder. Game folders
are marked with **★** and their title, archives with **▣** and their size.

| Key | Action |
|---|---|
| ↑ / ↓ (or `k` / `j`), PgUp / PgDn, Home / End | move |
| Enter | play the ★ game under the cursor, extract and play a ▣ archive, or open a folder |
| → / ← (or Backspace) | open a folder / go up |
| `~` | jump to your home folder |
| `.` | show hidden folders |
| `q` / Esc | cancel |

The mouse works too: click an entry to select it, click it again to open
or play it, scroll with the wheel.

emuera-tty remembers the game. From then on `emuera-tty` starts it
straight away. Other ways to start:

```sh
emuera-tty --choose                 # open the picker again (switch games)
emuera-tty ~/games/eratohoTW        # play a specific folder
cd ~/games/eratohoTW && emuera-tty  # a game in the current folder wins
emuera-tty ~/Downloads/eratohoTW.zip # play straight from the archive
```

**Archives** are extracted once into `~/.local/share/emuera-tty/games/`
(named after the archive) and reused after that. Download a newer version
under the same file name and it is re-extracted automatically, **keeping
your saves**. Zip, tar and tar.gz work out of the box; tar.xz / tar.bz2 /
tar.zst, 7z and rar need `bsdtar`, `tar` or `7z` installed
(`sudo apt install libarchive-tools p7zip-full`). Japanese file names in
zips made on Windows are handled. Delete a game's folder there to remove
it.

## 4. Playing

Emuera games are menus of numbered choices like `[100] Wake Up`.

- **Choose**: type the number and press Enter, or **click** it. Buttons
  under the mouse are highlighted.
- **Continue** (when the game waits after text, `▼ (Enter)` at the
  bottom): Enter, any key, or a click.
- **Scroll back** through earlier text: PgUp / PgDn, ↑ / ↓, Home / End or
  the mouse wheel. A `[scroll -N]` tag shows you're looking at history;
  End jumps back.
- **Redraw** a garbled screen: Ctrl-L. **Quit**: Ctrl-C (save first!).
  Ctrl-C also works while the game is busy computing, so a script stuck
  in a loop can always be stopped; the screen keeps updating meanwhile.

**Saving** uses the game's own save menu, and the files are Emuera's own
(binary) format: `save00.sav`, `save01.sav`, … and `global.sav`, plus
`dat/chara_*.dat` for SAVECHARA. Saves move both ways between emuera-tty
and Emuera on Windows. They go where Emuera puts them: in `sav/` when
the game's config says so (`セーブデータをsavフォルダ内に作成する`, as
AnonTW's does), else next to the game; compressed when it asks for that
(`セーブデータを圧縮して保存する`). Emuera-EM maps, XML documents and data
tables named in `VarExt.csv` are saved too, and the autosave runs on
entering the shop, like Emuera's. Older emuera-tty saves (`sav/*.rsav`)
still load, and the next save of that slot writes a `.sav`. Emuera's
older text save format (games without the binary option) can't be read
yet.

## 5. Picking a terminal

Text works everywhere. Images and Japanese/Chinese text depend on the
terminal:

| Terminal | Images | Notes |
|---|---|---|
| **kitty**, **WezTerm**, **Ghostty**, Konsole | yes (kitty graphics) | best choice |
| **foot**, xterm (`-ti vt340`), mlterm, yaft | yes (sixel) | |
| GNOME Terminal, Alacritty, others | `░` placeholders | fine for playing |
| Linux console under **fbterm** | yes (framebuffer) | needs the `video` group |
| bare Linux console (no fbterm) | yes (framebuffer) | can't show Japanese/Chinese text |

- Install a font with **CJK** glyphs (e.g. Noto Sans CJK) so Japanese
  names and symbols show up.
- **Maps** (box drawing, `■`, `※`…) are two columns per symbol in Emuera.
  emuera-tty pads them to two columns; in GNOME Terminal you can instead set
  *Preferences → profile → Compatibility → Ambiguous-width characters:
  Wide* (WezTerm: `treat_east_asian_ambiguous_width_as_wide = true`).
  Other terminals get padding, which keeps maps aligned but less solid.
- Make the window **wide**: many games lay out menus for ~150 columns.
  If a game warns that the window is too narrow, shrink the font or
  widen the window.
- `--gfx none` turns images off; `--gfx kitty|sixel|fb` forces a backend
  if auto-detection guesses wrong.

**On the Linux console** (Ctrl-Alt-F3 etc.):

- Run inside fbterm for CJK text: `fbterm -- emuera-tty`.
- Images draw straight to `/dev/fb0`, which needs the `video` group:
  `sudo usermod -aG video $USER`, then log in again.
- The mouse needs either **gpm** (`sudo apt install gpm`) or read access
  to `/dev/input/mice` (the `input` group). Be aware the `input` group can
  read every input device, keyboards included; gpm is the safer choice.

## 6. Updating and removing

```sh
emuera-tty --upgrade      # download the latest version, rebuild, reinstall
emuera-tty --reset-data   # forget the remembered game (saves are kept)
emuera-tty --uninstall    # remove the installed program
```

`--upgrade` keeps its own copy of the source in
`~/.local/share/emuera-tty/src` and needs `git` and `cargo`. Settings live
in `~/.config/emuera-tty/`; delete that folder (or use `--reset-data`) to
start fresh. Games extracted from archives (and their saves) are in
`~/.local/share/emuera-tty/games/`; neither `--reset-data` nor
`--uninstall` touches them.

## 7. When something goes wrong

This is a proof of concept, so it will happen.

- **A red `*** script error ***` screen** means the game hit something
  emuera-tty doesn't support yet. The message names the game file, line
  and function. Press Enter to quit; your last save is safe.
- **`warning: … not implemented (ignored)`** lines in the terminal after
  quitting are commands emuera-tty skipped; the game usually carries on.
- **Check a game without playing it:** `emuera-tty /path/to/game --check`
  lists every line that fails to parse.
- **Screen looks broken:** Ctrl-L redraws; try `--gfx none` to rule out
  image problems; make the window wider.
- **Reporting a bug:** include the error text and, if you can, a trace:
  `EMUERA_TRACE=FUNCTIONNAME emuera-tty 2> trace.txt` logs every call to
  functions whose name contains `FUNCTIONNAME`, with their results.

# Reference

## Command-line options

```sh
emuera-tty [GAME_DIR] [options]
```

| Option | |
|---|---|
| `--choose` | open the game picker even if a game is remembered |
| `--gfx auto\|kitty\|sixel\|fb\|none` | image backend (also `EMUERA_GFX`) |
| `--check` | parse the game, list errors, exit |
| `--headless` | plain stdin/stdout mode (scripts, tests); `--echo` shows typed input |
| `--strict` | unimplemented commands are errors instead of warnings |
| `--debug` | enable `[IF_DEBUG]` blocks and `;#;` lines |
| `--install` / `--upgrade` / `--uninstall` / `--reset-data` | see the guide |

Environment: `FRAMEBUFFER=/dev/fbN` picks another framebuffer,
`EMUERA_MOUSE=mice|off|term` overrides the console-mouse choice,
`EMUERA_AMBIGUOUS=wide|narrow|osc66` overrides how box-drawing and symbol
characters are fitted to Emuera's two columns (detected at startup:
terminals that draw them wide need nothing, others are padded; `osc66`
tries kitty's text-sizing protocol instead, which some may prefer).
At startup emuera-tty also measures how wide the terminal really draws
those characters (terminals' width tables differ), which keeps maps
aligned in Konsole, kitty and others; `EMUERA_MEASURE=0` skips that,
`EMUERA_TRACE` as above. Game scripts are read and parsed on every CPU
core (AnonTW loads in about a second instead of five, and takes about
600 MB of memory);
`EMUERA_THREADS=N` caps the thread count, and `EMUERA_TIMING=1` prints
how long each loading phase took. `EMUERA_SEED=N` fixes the random
number seed, so a run can be repeated exactly (tests, bug reports).

For development: `cargo test` runs the unit tests and the `sample/` game's
self-test; `emuera-tty sample --headless --echo` plays the sample in plain
text.

### Mouse

| Where | How |
|---|---|
| xterm, kitty, WezTerm, foot, Ghostty, Alacritty, GNOME/KDE terminals, tmux | SGR (1006) mouse reports |
| urxvt and other 1015 terminals | urxvt (1015) reports |
| older terminals; Linux console with **gpm** running | legacy X10 reports |
| Linux console / fbterm **without** gpm | reads `/dev/input/mice` directly (wheel included) and draws its own pointer |

Modifier keys held during a click are ignored. The console-mouse path needs
read access to `/dev/input/mice`: either run gpm (`apt install gpm`), or add
yourself to the `input` group — note that the `input` group can read *all*
input devices, keyboards included. `EMUERA_MOUSE=mice` forces the console
mouse anywhere, `EMUERA_MOUSE=off` (or `term`) disables it.

## Graphics backends

`--gfx auto` picks, in order:

| Environment | Backend | How |
|---|---|---|
| `TERM=linux` / `fbterm*` (VT) | **fb** | mmaps `/dev/fb0`, alpha-blends sprites onto the console after each frame. Needs read/write access to the device (`video` group). Cell size = fb resolution ÷ cols/rows. |
| kitty graphics reply `_Gi=31;OK` | **kitty** | RGBA transmit once per sprite+size, `a=p` placements, source-rect crop at the screen edge. |
| DA1 reply contains `4` | **sixel** | median-cut 255-colour quantiser, transparent background, cached per sprite. |
| otherwise | none | images shown as `░` placeholders of the right size. |

Images are sized in Emuera's coordinate space (font-size–relative, as in
`HTML_PRINT <img height=…>`) and scaled by `terminal cell height / Emuera
line height`, so layouts keep their proportions. `emuera.config` font size /
line height / colours are honoured.

**fbcon caveat:** the in-kernel console can't draw CJK glyphs. For
eratohoTW's Japanese/Chinese text on a bare VT, run under **fbterm** (still
uses the fb backend) or **yaft** (sixel).

## Layout

```
src/
  loader.rs      file discovery, UTF-8/BOM vs Shift_JIS detection, SJIS widths
  csvdata.rs     name tables (ABL.csv…), Chara*.csv, GAMEBASE, VariableSize, _Rename, *.ERD
  expr.rs        expression + FORM-string parser (%…%, {…}, \@ ? # \@, @"…")
  parser.rs      ERB/ERH statements → flat instruction lists with patched jumps
  ast.rs         Expr / Instr / Function types
  vm/mod.rs      variables (global / LOCAL / private #DIM / STATIC / chara), eval, call stack
  vm/builtins.rs ~200 commands & functions
  vm/system.rs   TITLE / FIRST / SHOP / TRAIN / ABLUP / AFTERTRAIN / TURNEND flow
  vm/save.rs     save/load: what goes into Emuera's .sav / .dat files
  vm/erasav.rs   Emuera's binary save format (reader / writer)
  console.rs     Emuera console model: styled segments, buttons, auto `[N]` buttons
  html.rs        HTML_PRINT subset
  tty.rs         raw-mode TUI, layout/wrapping/alignment, mouse, colour downgrade (24-bit/256/16)
  gfx/{kitty,sixel,fb}.rs
sample/          small test game; ERB/TEST.ERB is a language self-test
```

Design notes: function bodies compile to flat instruction vectors (so
GOTO/labels/loops need no recursion); `CALL` recurses on a 512 MB-stack
thread; `BEGIN`/`QUIT`/errors unwind as `Ctrl` values. Identifiers are
case-insensitive (Emuera default). 1-D arrays auto-grow; multi-D are bounds
checked.

## Implemented

- ERB: functions with params/defaults, `#FUNCTION(S)`, event functions with
  `#PRI/#LATER/#SINGLE/#ONLY`, `#DIM(S)` incl. CONST/STATIC/SAVEDATA/GLOBAL/
  CHARADATA and initialisers, `#DEFINE`, `[SKIPSTART]`, `[IF_DEBUG]`, `[IF X]`,
  `{ }` line joining, `;!;`, `_Rename.csv`.
- Control flow: IF/ELSEIF/ELSE, SIF, SELECTCASE (`TO`, `IS`), FOR/NEXT,
  REPEAT/REND, WHILE/WEND, DO/LOOP, BREAK/CONTINUE, GOTO/TRYGOTO,
  CALL/JUMP/TRY*/TRYC*…CATCH, *FORM variants, TRY*LIST, RETURN/RETURNF,
  BEGIN, RESTART, THROW, QUIT.
- PRINT family (V/S/FORM/FORMS × K/D × L/W/C/LC, SINGLE, PLAIN), PRINTDATA,
  STRDATA, PRINTBUTTON, PRINT_IMG, HTML_PRINT, DRAWLINE, colours/styles,
  ALIGNMENT, REUSELASTLINE, CLEARLINE.
- INPUT/INPUTS/TINPUT/ONEINPUT/WAIT/TWAIT/INPUTMOUSEKEY (approx.).
- Characters: ADD/DEL/SWAP/COPY/SORT/PICKUP, CSV* lookups, FINDCHARA.
- Strings, math, arrays (VARSET, ARRAYSHIFT/REMOVE/SORT/COPY, SPLIT, …).
- SAVEGAME/LOADGAME menus, SAVEDATA/LOADDATA, SAVEGLOBAL/LOADGLOBAL, @SAVEINFO.
- Emuera variable lifetimes: private `#DIM` static unless `DYNAMIC`,
  per-function `LOCAL`/`ARG`, `#DIM REF` parameters.
- Emuera-EM/EE: `MAP_*` string maps, `DT_*` data tables (incl. Select
  filters and XML), `XML_*` documents (XPath subset: steps, `//`, `*`,
  `text()`, `@attr`, predicates), `VARSETEX`, `AWAIT`, `LOADTEXT`/`SAVETEXT`,
  `HTML_TAGSPLIT`, `HTML_POPPRINTINGSTR`.

Debugging: `EMUERA_TRACE=<part of a function name>` logs matching calls and
their return values to stderr.

## Roadmap

1. Play deeper into AnonTW. Its 5 remaining parse errors are mistakes in
   the game's own scripts (Emuera reports them too).
2. Emuera graphics API (`GCREATE`, `GDRAWSPRITE`, `SPRITE*`, `CBG*`) — eratohoTW's
   newer UI uses these; currently ignored with a warning.
3. Animated sprites (`ANIME` in resources CSV), `srcb` hover images.
4. `LOCAL@FUNC`, `#LOCALSIZE`.
5. Exact Emuera semantics for TRAIN (NEXTCOM, ASSIPLAY, juel handling) and
   the shop item table layout.
6. Emuera's older text save format (binary `.sav` files already work).
7. fbterm 256-colour escapes; damage-tracked redraw to reduce flicker on fb.
