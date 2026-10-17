# emuera-tty

> **⚠️ Read this first: this is a vibecoded proof of concept.**
>
> This project was made by a self-described dumbass who doesn't really know
> what they're doing, by asking an AI (Claude) to write it. Nearly all of the
> code was AI-generated, reviewed only by running the game and seeing what
> breaks. It is not a serious or complete Emuera port, nobody has audited it,
> and it will mis-render, misbehave or crash on plenty of scripts.
>
> - Expect bugs. Don't trust it with save files you care about.
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
gets from its title screen through new-game setup to the main menu in
headless mode; anything past that is untested. Large parts of the Emuera
command set are still missing (see *Roadmap*).

## Build & run

```sh
cargo build --release
./target/release/emuera-tty /path/to/eratohoTW            # full-screen TUI
./target/release/emuera-tty /path/to/eratohoTW --check    # parse only, list errors
./target/release/emuera-tty sample --headless --echo      # stdin/stdout mode
cargo test                                               # runs unit tests + sample/ self-test
```

Options: `--gfx auto|kitty|sixel|fb|none` (or `EMUERA_GFX`), `--headless`,
`--echo`, `--check`, `--strict` (unknown commands become errors), `--debug`
(enables `[IF_DEBUG]` / `;#;` lines). `FRAMEBUFFER=/dev/fbN` picks another fb.

## Controls

Type a number + Enter, or **click** any `[N]` / HTML button (mouse hover
highlights it). Enter / any key / click continues a WAIT. PgUp/PgDn, ↑/↓,
Home/End and the mouse wheel scroll the log. Ctrl-L redraws, Ctrl-C quits.

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
  vm/save.rs     save/load (own text format in GAME/sav/*.rsav)
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
  filters and XML), `LOADTEXT`/`SAVETEXT`, `HTML_TAGSPLIT`,
  `HTML_POPPRINTINGSTR`.

Debugging: `EMUERA_TRACE=<part of a function name>` logs matching calls and
their return values to stderr.

## Roadmap

1. Burn down the remaining AnonTW parse errors / unknown commands
   (`XML_*`, `VARSETEX`, `AWAIT`, `SAVECHARA`, …) and play past the main menu.
2. Emuera graphics API (`GCREATE`, `GDRAWSPRITE`, `SPRITE*`, `CBG*`) — eratohoTW's
   newer UI uses these; currently ignored with a warning.
3. Animated sprites (`ANIME` in resources CSV), `srcb` hover images.
4. `LOCAL@FUNC`, `#LOCALSIZE`; saving `SAVE_MAPS` / `SAVE_DTS` (VarExt.csv)
   maps and data tables with the game.
5. Exact Emuera semantics for TRAIN (NEXTCOM, ASSIPLAY, juel handling) and
   the shop item table layout.
6. Emuera `.sav` compatibility (read existing saves).
7. fbterm 256-colour escapes; damage-tracked redraw to reduce flicker on fb.
