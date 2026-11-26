# CODE_DETAILS.md

A brief description of what each `.rs` file does and how the parts operate
together, plus the Android app's `.kt` files. Use it to find where a feature
lives; `CLAUDE.md` holds the rules, constraints and commands.

## The whole picture

```
main.rs / lib.rs
  └─ startup: options → CSVs (csvdata) → ERH then ERB files (parser) →
     resources → frontend → VM system loop (vm/mod.rs → vm/system.rs)
```

`parser.rs` lazily compiles each `@FUNCTION` body to a flat `Vec<Instr>`;
`vm/mod.rs` runs those instructions, resolving variables through
`vm/vars.rs` (`Arr`s), and a user `CALL` is a Rust call into `call_func`.
`vm/system.rs` drives Emuera's game-flow state machine (TITLE, SHOP, TRAIN…).
Built-ins live in `vm/builtins.rs` (commands `COMMANDS`, functions
`FUNCTIONS`, the `MAP_*`/`DT_*` families in `vm/datatable.rs` and
`vm/sql.rs`). The VM only appends styled segments to a `console::Console`;
frontends lay them out and collect input.

Files saves go through `vm/save.rs` (Emuera's binary format in
`vm/erasav.rs`, text format in `vm/textsav.rs`), and the launcher's save
manager is `savemgr.rs` (with `savesafe.rs` for backups).

## Entry points

- `src/main.rs` — the binary. Parses CLI options, reads CSVs (`csvdata`),
  parses ERH then ERB files (`parser`), loads images (`resources`), picks a
  frontend, reads the game's config (`emuconfig`), and runs the VM's system
  loop on a thread with a big stack (ERB calls recurse deeply).
- `src/lib.rs` — `include!("main.rs")` plus `mod android`; the shared
  library the Android app loads as a cdylib. Everything is `#[cfg(android)]`
  so a host build compiles the engine once.

## Parsing

- `src/ast.rs` — the compiled program representation: `Expr`, `Form`,
  `Instr` (one statement; bodies are flat, jumps patched to instruction
  indices), `Function`, `Body`. Identifiers stored upper-cased.
- `src/parser.rs` — turns `.ERH`/`.ERB` source into a `Program`:
  logical lines (`[SKIPSTART]`, `[IF]`, `[[rename]]`, `{ }` joining), `#DEFINE`
  / global `#DIM` headers, then per-function `@NAME(args)` bodies compiled to
  `Instr`s on first use. `Funcs` keeps a small `Head` per name until first
  lookup (big games: ~100k functions, heads take 8 MB vs 20 MB for full
  functions). Unparseable lines become `Instr::BadLine`.
- `src/expr.rs` — expression and FORM-string (`%…%`, `{…}`, `\@ ? # \@`)
  recursive-descent parser, with `scan_*` helpers that agree on nested
  boundaries (quotes inside `%…%` don't end strings).
- `src/loadcheck.rs` — Emuera's load warnings, filtered by the game's
  settings, in Emuera's words and format (shown by `--check`).
- `src/lint.rs` — `EMUERA_LINT=1`: points at statements the parser probably
  split wrongly, and string `>=`/`<=` comparisons.

## The VM

- `src/vm/mod.rs` — variable resolution and the instruction loop. Every
  variable is an `Arr`; `locate` turns a `VarRef` into a `Place`
  (`Loc` + flat index). Storage: globals, `Chara::vars` per character,
  `statics[FUNC][NAME]` for LOCAL/ARG/private `#DIM`s, the call's
  `Frame::vars` for `#DIM DYNAMIC`, `Frame::refs` aliases for `#DIM REF`.
  Errors/`BEGIN`/`QUIT` unwind as `Err(Ctrl)` to the system loop.
- `src/vm/builtins.rs` — every built-in command (`COMMANDS`) and function
  (`FUNCTIONS`), implemented in `Vm::command` / `Vm::builtin_fn`. Arguments
  arrive unevaluated (`&[Expr]`), evaluated with the `arg`/`ai`/`as_`/
  `avar` helpers. Also `map_fn`/`dt_fn` entry points and the
  `STR_FUNCTIONS` list.
- `src/vm/system.rs` — Emuera's game-flow state machine: `Vm::run` loops
  over states (TITLE, FIRST, SHOP, TRAIN, ABLUP…), running one state per
  `run_state` and moving on via `BEGIN`.
- `src/vm/save.rs` — what goes in a save, in Emuera's order: system
  variables, `#DIM SAVEDATA` (creation order via `declared_dims`),
  characters' saved vars + `#DIM CHARADATA SAVEDATA`, then the EM maps/XML/
  data tables named in VarExt*.csv. Decides `.sav` location (`sav/` folder
  or game folder).
- `src/vm/erasav.rs` — Emuera's binary save format (`1808`, .NET
  `BinaryWriter`-style strings, compressed ints, run-length zero arrays).
  Reader/writer; byte-compatible with Emuera both ways.
- `src/vm/textsav.rs` — reader for Emuera's text save format (one value per
  line; writing is always binary).
- `src/vm/vars.rs` — variable storage: `Arr` holding `Data` (dense `Vec`,
  sparse `Z` for mostly-zero int arrays, `S` for strings with a hash search
  table), and the built-in variable table with default sizes.
- `src/vm/value.rs` — `Value` (Int/Str), `Ctrl` (Begin/Quit/Error control
  flow), `VmResult`, and `StrMap` (insertion-ordered string map for `MAP_*`).
- `src/vm/datatable.rs` — the `DT_*` / `MAP_*` Emuera-EM family (`dt_fn`,
  `map_fn`).
- `src/vm/sql.rs` — `SQL_*` functions: one SQLite database per name in
  `temp_db/`, databases travel with saves under `saveNN/`.
- `src/vm/debugger.rs` — breakpoints, stepping, variable inspection, call
  stack; integrated into `exec`/`call_func_in`, via `EMUERA_VM_DEBUG` or
  `--vm-debug`.
- `src/vm/debugwin.rs` — Emuera's debug window's VM side: watched
  expressions, stack trace, debug console; reads/writes
  `debug/watchlist.csv` and `debug/console.log` so they move between
  Emuera and emuera-tty.
- `src/vm/memstats.rs` — `EMUERA_MEMSTATS=<file>` report of what memory
  holds, rewritten at every input.

## Console, output and i18n

- `src/console.rs` — the console model: a scrollback of `Line`s made of
  styled `Seg`s (text, buttons, images, spaces, rects). The VM only appends;
  frontends wrap and lay out. Buttons live only for lines since the last
  input (`gen`).
- `src/i18n.rs` — emuera-tty's own interface text via `t!`/`tc!` (English
  text as key), gettext `.po` catalogues in `i18n/` compiled in.
- `src/emuera_lang.rs` — Emuera's own messages (load warnings, script
  errors) keyed by ID like Emuera's `Lang.cs`; Japanese + our English,
  never copied from Emuera's `lang/*.xml`.
- `src/html.rs` — HTML_PRINT subset (`p`, `font`, `b`, `i`, `u`, `s`,
  `button`, `nonbutton`, `img`, `shape`, `br`) interpreted straight into
  console segments.
- `src/clipboard.rs` — Emuera-EE's clipboard: printed text to the system
  clipboard via OSC 52 on chosen triggers, for translation tools.
- `src/macros.rs` — keyboard macros (`\n` separates inputs, `\e` skips
  messages, `(…)*n` repeats).
- `src/mt.rs` — live dialog translation via a local translator (Ollama,
  LibreTranslate, Bergamot, argos, a custom command, translate-shell, DeepL).
- `src/translate.rs` — Emuera-Anchor's `*_TR.csv` / `TRCHARA` translation
  tables.
- `src/json.rs` — small JSON reader for the launcher's downloader (wiki
  API answers).

## Data and resources

- `src/csvdata.rs` — CSV-side data: name tables (`ABL.csv`…), character
  templates (`Chara*.csv`), `GAMEBASE.csv`, `VariableSize.csv`, `_Rename.csv`,
  `*.ERD`, Anchor `*_TR.csv`.
- `src/loader.rs` — file discovery and text decoding (UTF-8 with/without BOM
  vs Shift_JIS) for a game directory; `emuera_lines` (C# ReadAllLines
  semantics, lone `\r` too).
- `src/resources.rs` — `resources/*.csv` sprites + image loading/scaling
  cache (whole files, cropped sprites, scaled sprites; LRU within
  `EMUERA_PICTURES_MB`; background warmup on spare cores). Graphics buffers
  (`GCREATE…`), `SPRITECREATE` sprites, client background layers (`CBG*`).
- `src/gamefont.rs` — fonts a game ships in `font/` (`.ttf`/`.otf` by
  family name); glyphs only such a font has (private-use code points) drawn
  as pictures.
- `src/xml.rs` — minimal XML reader/writer for `DT_FROMXML`/`DT_TOXML` and
  the `XML_*` functions.

## Frontends

- `src/frontend.rs` — the `Frontend` trait: `input(kind, req)` shows the
  console and returns what was typed/clicked; `InputKind`, `InputReq`,
  `RawInput` (click/wheel/key/timeout/quit/debug).
- `src/tty.rs` — full-screen TUI: raw-mode input with mouse (SGR/urxvt/X10,
  or `/dev/input/mice` on the Linux console), redraw-per-frame renderer,
  colour downgrade (24-bit → 256 → 16), Emuera's two-column width rules in
  `char_cells`. Submodules: `complete` (shell completion), `debugpanel`,
  `envvars`, `gamecfg` (Emuera's settings window copy), `lineedit`, `saves`
  (save manager).
- `src/frontend.rs` also defines `Headless` (stdin/stdout, used by
  `--headless`, tests, scripts).
- `src/mcp.rs` — `--mcp`: the game as an MCP server on stdin/stdout (`eval`/
  `run`/`state` tools reach the VM as `DebugAction`s).
- `src/retro.rs` — terminfo-driven frontend for retro/serial terminals
  (VT52, VT100, ADM-3A, Wyse, Teletype).
- `src/terminfo.rs` — small terminfo reader + `tparm`/`$<n>` expansion for
  the retro frontend.
- `src/gfx/mod.rs` + `kitty.rs`, `iterm.rs`, `sixel.rs`, `fb.rs` (and
  `fb_none.rs`) — inline-image backends for the TUI; pictures blended into
  one layer before sending.

## Platform

- `src/sys.rs` — terminal raw mode, size, input polling, clock, file-system
  details (termios/poll on Unix; Windows console API in VT mode on Windows).
- `src/mice.rs` / `src/mice_none.rs` — Linux-console mouse via gpm or
  `/dev/input/mice` (stub elsewhere).
- `src/gamepad.rs` / `src/gamepad_none.rs` — gamepads from evdev (Linux
  only).
- `src/sound.rs` — PLAYSOUND/PLAYBGM via an external player (mpv, ffplay,
  mplayer, afplay, pw-play, paplay, aplay, PowerShell on Windows); no audio
  library linked.
- `src/atomic64.rs` — `AtomicU64` where 64-bit atomics exist; two 32-bit
  halves elsewhere (32-bit PowerPC, MIPS, ARMv5, RISC-V 32…).
- `src/fxhash.rs` — fast non-cryptographic hasher (rustc's Fx) for name
  tables.

## Game lifecycle, launcher, settings

- `src/picker.rs` — game launcher/picker: library page, add-a-game folder
  browser, settings page; lists games, launches one, imports archives.
- `src/archive.rs` — games played from their download archive.
- `src/download.rs` — the eraGames wiki downloader (git repositories only;
  archive links become branches).
- `src/savemgr.rs` — save file management: list/copy/move/export/import
  saves, back up/restore the save folder (backups under
  `$XDG_DATA_HOME/emuera-tty/backups`).
- `src/savesafe.rs` — writing saves safely (replace → backup → write) and
  save history/fingerprints.
- `src/settings.rs` — the program's settings (`key=value` in
  `$XDG_CONFIG_HOME/emuera-tty/config`); `ITEMS` describes every setting for
  the menus.
- `src/emuconfig.rs` — the game's Emuera settings (`_default.config`,
  `emuera.config`, `_fixed.config`, Emuera's key names); edits go to the
  game's `emuera.config`, shared with Emuera.exe.
- `src/config_names.rs` — Emuera config keys, Japanese + English, from
  Emuera-EM's `ConfigData`.
- `src/completions.rs` — `--completions bash|zsh|fish` and `--install`.
- `src/emuconfig.rs`/`settings.rs` feed `t!`-translated menus.

## Android app (`android/kotlin/`)

The same engine (`src/lib.rs` + `src/android.rs`, built as a cdylib by
`android/build.sh`) with a Kotlin UI. One game per process; ending the
process returns its memory to Android. See `android/CLAUDE.md` and
`android/README.md` for build/install.

- `MainActivity.kt` — the game list: import a ZIP/TAR or a folder (system
  file picker, no storage permission), play a game, settings, font picker,
  delete. Shows import progress, recovery markers for dead/restarted games.
- `GameActivity.kt` — a running game in its own (`:game`) process: starts
  the VM, shows `GameView`, handles replays (`--replay` bench files), extra
  `env` settings, columns, tablet mode.
- `GameService.kt` — foreground service keeping a game alive while another
  app is in front; killed when swiped away in Recents.
- `GameView.kt` — the console view: draws rows of `Run`s (styled text with
  buttons, images, rects) onto a canvas; touch → `RawInput`; image reach
  above the view; client background layers.
- `Native.kt` — the JNI bridge: `start`/`setConfigHome`/`viewRows`/click/
  `line`/tap/wheel/quit, save list/copy/delete/import/backup/restore,
  `gameSettings`/`setGameSetting`/`globalSettings`/`setGlobalSetting`,
  `showMemory`, `toTitle`/`reloadConfig`/`reloadErb`, `trim`, `drawn`,
  `pointer`, `importArchive`/`finishImport`/`onImportProgress`.
- `GameSettings.kt` — the game's own settings screen: every Emuera settings
  item tab by tab, widget per kind, source/effect legend, per-item reset;
  edits the game's `emuera.config` (read by Emuera.exe too).
- `SettingsActivity.kt` — the engine's global settings: one screen listing
  every `globalSettings()` item, grouped by category; tap to pick a choice;
  items not applying on Android are grayed out; values written via
  `setGlobalSetting`, shared with the desktop program.
- `Saves.kt` — the running game's saves: copy/move/slot, export, delete;
  back up/restore/import all; deleted or overwritten saves kept in backups
  first.
- `Crash.kt` / `gameMarker` / `restartMarker` / `showEngineLog` — the last
  game's marker (died without a word), restart marker, engine log dialog.
- `GameAudio.kt` — sound: ten effect channels + one looping BGM via
  `MediaPlayer`; silent while hidden.
- `Insets.kt` — keeps view content clear of status/navigation bars, notch,
  keyboard (Android 15+ lays out under all of them).
- `Paths.kt` — `inside(root, id)` (path traversal guard) and `cleanName`
  (safe folder name from an outside name); tested by `tools/paths_test.kt`.
- `GamesProvider.kt` — a `DocumentsProvider` exposing the app's games folder
  to the system file manager (move saves to/from other devices, add to a
  game).
- `android/tools/paths_test.kt` — PC-side test for `inside`/`cleanName`
  (needs `kotlinc` + a JDK; no phone).

## How a game runs

1. `main.rs` reads CSVs (`csvdata`), then ERH headers (`#DEFINE`, global
   `#DIM`) then ERB files on all cores (`parser`); only each function's `@`
   line and attributes are parsed at load — bodies compile on first call by
   re-reading the file.
2. A frontend is chosen (`tty::Tty` full-screen, `Headless` for
   `--headless`/tests/MCP, `retro.rs` for serial terminals, `Android` on
   the phone).
3. `Vm::run` (`vm/system.rs`) drives the state machine; each state runs the
   script's event functions and moves on via `BEGIN <state>`.
4. Statements run through `Vm::command` → `vm/builtins.rs`; expressions
   through `Vm::builtin_fn` → `vm/datatable.rs` (`MAP_*`/`DT_*`) or
   `vm/sql.rs` (`SQL_*`).
5. The VM appends styled segments to `console::Console`; the frontend lays
   them out and collects input; pictures go through `resources` → a
   `gfx` backend.
6. Saves (`vm/save.rs`, `vm/erasav.rs`) are byte-compatible with Emuera;
   `savemgr.rs` manages save files for the menus.
