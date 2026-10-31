What emuera-tty supports of Emuera, Emuera-EM/EE and Anchor. Deliberate
differences from Emuera are listed in
[doc/differences.md](https://github.com/astartes-gaming/emuera-tty/blob/master/doc/differences.md)
and [doc/anchor.md](https://github.com/astartes-gaming/emuera-tty/blob/master/doc/anchor.md).

## Scripts and control flow

- ERB: functions with params/defaults, `#FUNCTION(S)`, event functions with
  `#PRI/#LATER/#SINGLE/#ONLY`, `#DIM(S)` incl. CONST/STATIC/SAVEDATA/GLOBAL/
  CHARADATA and initialisers, `#DEFINE`, `[SKIPSTART]`, `[IF_DEBUG]`, `[IF X]`,
  `{ }` line joining, `;!;` / `;^;`, `_Rename.csv`.
- Control flow: IF/ELSEIF/ELSE, SIF, SELECTCASE (`TO`, `IS`), FOR/NEXT,
  REPEAT/REND, WHILE/WEND, DO/LOOP, BREAK/CONTINUE, GOTO/TRYGOTO,
  CALL/JUMP/TRY*/TRYC*…CATCH, *FORM variants, TRY*LIST, RETURN/RETURNF,
  BEGIN, RESTART, THROW, QUIT.
- Strings, math, arrays (VARSET, ARRAYSHIFT/REMOVE/SORT/COPY, SPLIT, …).
  `TOSTR` takes .NET format strings exactly as Emuera does: standard
  (`D2`, `X6`, `N0`, `F2`, `E3`, `P0`, `C`) and custom (`#,0`, `0.0`,
  `00;-00`, `0,.###`, `0%`, quoted text, `E+0`). `TOINT` / `ISNUMERIC`
  read `0x` hex, `0b` binary and exponents; FORM strings and literals
  know `\n \t \s \S`.
- `#LOCALSIZE` / `#LOCALSSIZE`; `LOCAL@FUNC` (and `LOCALS@`, `ARG@`,
  `ARGS@`) for another function's own.
- `REPLACE` / `REGEXPMATCH` / `FINDELEMENT` use .NET-style regular
  expressions as in Emuera (groups, `$1`, lookaround).
- Emuera variable lifetimes: private `#DIM` static unless `DYNAMIC`,
  per-function `LOCAL`/`ARG`, `#DIM REF` parameters.

## Output, input and sound

- PRINT family (V/S/FORM/FORMS × K/D × L/W/C/LC, SINGLE, PLAIN), PRINTDATA,
  STRDATA, PRINTBUTTON, PRINT_IMG, HTML_PRINT, DRAWLINE, colours/styles,
  ALIGNMENT, REUSELASTLINE, CLEARLINE.
- INPUT/INPUTS/TINPUT/ONEINPUT/WAIT/TWAIT/INPUTMOUSEKEY (approx.); INPUTS'
  default answer is FORM text, as in Emuera.
- Sound: PLAYSOUND / PLAYBGM / STOPSOUND / STOPBGM / SETSOUNDVOLUME /
  SETBGMVOLUME / EXISTSOUND (see [Sound](Playing#sound)); GETMEMORYUSAGE.
- Keyboard macros: the `\n` / `\e` / `(…)*n` input syntax, F1…F12 in ten
  groups, the game's `macro.txt` read and written as Emuera does.

## Pictures and HTML_PRINT

- HTML_PRINT as in Emuera-EM: `px` sizes, `<div>` boxes laid out at their
  offset (`xpos`/`ypos`/`width`/`height`, `rect=`, `size=`) with their
  background colour, border (`border` / `bcolor`, drawn as coloured
  cells), rounded corners (`radius`, a quarter block), `padding` and
  `margin`; `<clearbutton>`; `<button title=…>` and `<nonbutton title=…>`
  (text that isn't a button) tooltips shown by the pointer after Emuera's
  delay (`TOOLTIP_SETDELAY`, `TOOLTIP_SETDURATION`, `TOOLTIP_SETCOLOR`;
  `<br>` and CR break their lines; the font is the terminal's);
  `<img srcb=…>` hover pictures; colours by name or `#` and hex, `0x`
  allowed as in Emuera (`#0xFFA500`); newlines in the text break the line
  like `<br>`.
- Animated sprites: `ANIME` blocks in resources CSVs and
  `SPRITEANIMECREATE` / `SPRITEANIMEADDFRAME`, redrawn on the
  `SETANIMETIMER` interval (at most ten times a second).
- Graphics buffers and sprites (`GCREATE`, `GDRAW*`, `GSETFONT`/text,
  `SPRITECREATE`, `SPRITE*`), shown through the image backends.
- Client background layers (`CBGSETG`, `CBGSETSPRITE`, `CBGSETBUTTONSPRITE`,
  button maps) and the mouse in pixels (`MOUSEX` / `MOUSEY`,
  `INPUTMOUSEKEY` clicks, wheel, keys, timeouts), so menus drawn as
  pictures work, like pops-tw's title. Terminals without images get the
  text that was drawn into the picture, at its place on the screen.

## Game flow, characters and saves

- Characters: ADD/DEL/SWAP/COPY/SORT/PICKUP, CSV* lookups, FINDCHARA.
- TRAIN as in Emuera: commands numbered by their position in TRAIN.csv
  (other numbers go to `@USERCOM`), NEXTCOM, the WAIT after a command,
  and the per-step resets (UP / DOWN / LOSEBASE, SOURCE, NOWEX, STAIN to
  `_Replace.csv`'s defaults). PRINT_SHOPITEM, MONEYSTR and MONEYLABEL use
  `_Replace.csv`'s currency unit and its position. `CALLTRAIN` /
  `STOPCALLTRAIN` / `@CALLTRAINEND` (continuous training; pops-tw forces
  commands with it). `DOTRAIN` as in Emuera: the event it is in (only
  @EVENTTRAIN, @SHOW_STATUS, @SHOW_USERCOM, @EVENTCOMEND, @USERCOM) runs to
  its end, then the command runs.
- SAVEGAME/LOADGAME menus, SAVEDATA/LOADDATA, SAVEGLOBAL/LOADGLOBAL, @SAVEINFO.

## Game data and config files

- EMEE `.als` name aliases.
- `DAY.csv` / `TIME.csv` / `MONEY.csv` names (`DAYNAME`, `TIMENAME`).
- Config files with Emuera's Japanese or English keys (`ウィンドウ幅` or
  `WINDOW WIDTH`), including PRINTC items per line and width, the font,
  and button wrapping (a button that doesn't fit moves to the next line
  whole, as in Emuera).

## Emuera-EM, EE and Anchor

- Emuera-Anchor: the `*_TR.csv` / `ALL_TR.csv` / `TRCHARA` translation
  files and `TR_NAME`, `PARAM.CSV` (read in place of `PALAM.CSV`, with
  `PARAM` usable for `PALAM` in scripts), the Ctrl-T / Ctrl-R / Ctrl-E
  hotkeys, the clipboard.
- Emuera-EM/EE: `MAP_*` string maps, `DT_*` data tables (incl. Select
  filters and XML), `XML_*` documents (XPath subset: steps, `//`, `*`,
  `text()`, `@attr`, predicates), `VARSETEX`, `LOADTEXT`/`SAVETEXT`,
  `HTML_TAGSPLIT`, `HTML_POPPRINTINGSTR`, and `AWAIT` with the window
  still live: typing goes into the input line (`GETTEXTBOX`), the button
  under the pointer is `MOUSEB()`, and clicks and keys reach
  `GETKEY`/`GETKEYTRIGGERED` (AnonTW's dish search by name); so does
  the key or click that ends a `WAIT`/`TWAIT` (AnonTW's lectures end on a
  click).
- Infinite loop detection (`無限ループ警告までのミリ秒数` /
  `Milliseconds for infinite loop warning`): after a configurable timeout
  (default 5 s) without input, a confirmation dialog appears (TUI); headless
  runs go on with a note on stderr; configurable via `emuera.config`, the in-game
  settings (Environment tab), or `EMUERA_INFINITE_LOOP_ALERT_TIME`.
