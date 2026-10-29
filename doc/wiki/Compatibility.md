## Script load order

ERB and ERH files load in Emuera's order: each folder's files before its
subfolders, sorted the way Emuera sorts them when the config's
`読み込み順をファイル名順にソートする` is on (else the order Windows lists
them in), and folders whose name has a `#` first (Emuera-EE). Sorted
order depends on the .NET Emuera runs on, and games rely on the one they
were made with: a game that ships only .NET Framework builds of Emuera
(Emuera 1821, Anchor, PuchiAnchor) gets their order (`A.ERB` before
`A_B.ERB`), any other the order of today's Emuera-EM/EE (`A_B.ERB` first). Subfolders
are read only when `サブディレクトリを検索する` is on, as in Emuera; a
note at startup says how many files that leaves out. The order decides
which of two functions with the same name wins, the order event
functions run in, and the order variables are saved in.

## Layout

```
src/
  archive.rs           play games from their download archive (zip, tar, 7z, …)
  ast.rs               Expr / Instr / Function types
  atomic64.rs          AtomicU64 that works on all targets (no std atomic on some)
  clipboard.rs         OSC 52 clipboard (Emuera-EE / Anchor compatibility)
  config_names.rs      config key tables (Japanese + English), defaults, precedence
  console.rs           Emuera console model: styled segments, buttons, auto `[N]` buttons
  csvdata.rs           name tables (ABL.csv…), Chara*.csv, GAMEBASE, VariableSize, _Rename, *.ERD
  download.rs          game downloader from the eraGames wiki (git clone + LFS)
  emuconfig.rs         emuera.config / _default.config / _fixed.config (Japanese/English keys)
  expr.rs              expression + FORM-string parser (%…%, {…}, \@ ? # \@, @"…")
  frontend.rs          frontend trait: Headless, Tty, Retro
  fxhash.rs            FxHashMap/FxHashSet (faster than std for our key types)
  gamepad.rs           Linux gamepad support (Steam Deck, Xbox, PS, 8BitDo…)
  gamepad_none.rs      gamepad stub for non-Linux
  html.rs              HTML_PRINT: font/b/i/u/s, p, br, button/nonbutton, clearbutton, img, shape, space, div
  json.rs              JSON parse (for translators, config)
  lint.rs              EMUERA_LINT=1: dubious splits, string >= / <=
  loader.rs            file discovery, UTF-8/BOM vs Shift_JIS detection, SJIS widths
  macros.rs            keyboard macros: \n / \e / (…)*n, F1…F12 in 10 groups, macro.txt
  main.rs              entry point, front-end selection, VM thread with large stack
  mice.rs              Linux console mouse: gpm client or /dev/input/mice, drawn pointer
  mice_none.rs         console mouse stub for non-Linux
  mt.rs                live dialogue translation (Ollama, Argos, LibreTranslate, translate-shell, DeepL)
  parser.rs            ERB/ERH statements → flat instruction lists with patched jumps
  picker.rs            launcher: game library, folder / archive picker, settings page
  regex.rs             .NET-style regex (pattern cache, Emuera-EM/EE compatibility)
  resources.rs         resources/*.csv sprites: decoding, scaling, caching, parallel
  retro.rs             retro / dumb / serial terminals: terminfo-driven, ASCII, changed-cells-only redraw
  savemgr.rs           save slot listing, backup thinning, save-log.tsv
  savesafe.rs          atomic save write: temp file → flush → verify → rename
  settings.rs          saved settings and the game library (~/.config/emuera-tty/config)
  sound.rs             PLAYSOUND / PLAYBGM via mpv, ffplay, mplayer, afplay, fluidsynth (MIDI)
  sys.rs               platform layer: raw mode, size, input polling (termios / Windows console)
  terminfo.rs          compiled terminfo reader, tparm, delay padding
  translate.rs         Emuera-Anchor *_TR.csv / ALL_TR.csv / TRCHARA / PARAM.CSV / TR_NAME
  tty.rs               raw-mode TUI, layout/wrapping/alignment, mouse, colour downgrade (24-bit/256/16)
  xml.rs               XML documents, XPath subset (Emuera-EM/EE XML_* functions)
  gfx/{fb,fb_none,iterm,kitty,mod,sixel}.rs
  tty/envvars.rs       environment-variable overrides for settings
  tty/gamecfg.rs       Emuera's settings window (tabs, fields, emuera.config r/w)
  tty/lineedit.rs      input line editor with history, macros, key bindings
  tty/saves.rs         save manager UI (list, copy, move, export, backup, import)
  vm/builtins.rs       every Emuera / Emuera-EM / EE command and function
  vm/datatable.rs      MAP_* string maps, DT_* data tables (Emuera-EM/EE)
  vm/debugger.rs       interactive VM debugger (--vm-debug: breakpoints, step, watches, REPL)
  vm/erasav.rs         Emuera's binary save format (reader / writer)
  vm/memstats.rs       EMUERA_MEMSTATS: per-input memory breakdown
  vm/mod.rs            variables (global / LOCAL / private #DIM / STATIC / chara), eval, call stack
  vm/save.rs           save/load: what goes into Emuera's .sav / .dat files
  vm/system.rs         TITLE / FIRST / SHOP / TRAIN / ABLUP / AFTERTRAIN / TURNEND flow
  vm/textsav.rs        Emuera's text save format (reader)
  vm/value.rs          Value type (Int/Str/Arr) and operations
  vm/vars.rs           Arr type, VariableCode, CharaData, built-in variable tables
sample/                small test game; ERB/TEST.ERB is a language self-test
```

### Design notes

Function bodies compile to flat instruction vectors (so
GOTO/labels/loops need no recursion); `CALL` recurses on a 512 MB-stack
thread; `BEGIN`/`QUIT`/errors unwind as `Ctrl` values. Identifiers are
case-insensitive (Emuera default). 1-D arrays auto-grow; multi-D are bounds
checked.
