# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

emuera-tty is a Rust reimplementation of Emuera (the Windows/.NET engine for
eraBasic "ERB" games) that runs in a terminal. Fidelity to Emuera is the
goal: script semantics, layout, and save files that move both ways between
emuera-tty and Emuera.

# CRITICAL: Token Optimization & Constraints

## Communication Style
- **Be brief**: Skip all pleasantries, preambles, and post-explanations (e.g., NO "Certainly, I can help with that...").
- **Direct answers only**: Respond directly with code or the answer. 
- **Terse explanations**: Use bullet points. Keep explanations under 2 sentences unless deeply architectural.

## Coding Constraints
- **Diffs Only**: Never output full file rewrites. Only provide unified diff chunks (`+++`/`---`) or targeted snippet replacements.
- **Context Preservation**: Do not read full files if you only need a specific block. Use specific line offsets if supported.
- **No Refactoring Spree**: Do not fix stylistic issues or add unprompted dependencies in surrounding code unless explicitly asked.
- **snip**: Ensure that `snip` (https://github.com/edouard-claude/snip) is installed prior to running any commands.
  It can hide what matters: `cargo test`'s result lines, and errors such as
  a missing command's "command not found". Filter test and error output with
  `grep` instead (`cargo test --release 2>&1 | grep -E "^test result|FAILED|panicked"`),
  and check that a command exists (`which`) before trusting its silence.

## Dependencies

Package names are Debian/Ubuntu's (Fedora's where they differ).

- **Building:** stable Rust (developed on 1.99) and git. Crate
  dependencies come through cargo; there are no system libraries to
  install. Some systems ship older Rust: the clippy lints whose
  suggestions need Rust 1.87+ are turned off in `Cargo.toml`, so don't
  reach for newer std APIs casually.
- **Development tools:** clippy (`cargo clippy`, the linter) and rustfmt
  (`cargo fmt`, the formatter). With rustup, `rust-toolchain.toml`
  installs both on the first `cargo` run here. With the distribution's
  Rust (`rustc` + `cargo` packages), install the `rust-clippy` and
  `rustfmt` packages (Fedora: `clippy`, `rustfmt`). Run `cargo fmt`
  before committing (settings in `rustfmt.toml`); the one-time reformat
  is in `.git-blame-ignore-revs`, which `git config blame.ignoreRevsFile
  .git-blame-ignore-revs` makes `git blame` skip. There are no
  `[dev-dependencies]` crates: the tests use only std.
- **The test scripts in `tools/`:** Python 3 and pyte (`python3-pyte`,
  or `pip install -r tools/requirements.txt`) for `gamed.py` and
  `console_mouse.py`; `savdiff.py` needs only Python 3. `gamed.py` reads
  `/proc`, so it runs on Linux only.
- **Other CPUs:** `rustup target add <target>` (musl targets, e.g.
  `arm-unknown-linux-musleabihf`, `riscv64gc-unknown-linux-musl`);
  `rust-lld` comes with the rustup toolchain. Running them needs
  `qemu-user`. Tier-3 targets (m68k, MIPS…) need a nightly toolchain
  (`cargo +nightly check -Z build-std=std,panic_abort --target …`).
- **Testing in a real Konsole off-screen:** `xvfb`, `konsole`, `xdotool`,
  `imagemagick` (its `import`), `dbus-daemon` (`dbus-run-session`).
- **Optional at run time:** `gpm` for the Linux-console mouse, `mpv` or
  `ffplay` for game sound, and a translator (Ollama, Argos Translate,
  LibreTranslate, translate-shell, or DeepL through curl) for the
  translation settings.
- **Planning:** the OpenSpec CLI (`openspec`) for the changes in
  `openspec/` (see *OpenSpec* below).

## Commands

```sh
cargo build --release                       # LTO "fat", 1 codegen unit: slow to link
cargo build --profile fastbuild             # release speed without LTO, quicker
cargo test --release                        # unit tests, sample self-test, ERB suite
cargo test --release --test erb_suite       # just the ERB suite (tests/erb, golden, save)
cargo test --release emuera_name_order      # one test by name
cargo test --release --test sample_selftest # only the sample-game integration tests
cargo clippy --release                      # lint config lives in Cargo.toml [lints.clippy]
cargo fmt                                   # format before committing (rustfmt.toml: 160 columns)
./target/release/emuera-tty --install       # install to ~/.local/bin
```

Running a game without the TUI (scripts, tests, reproductions):

```sh
printf '0\n1\n' | ./target/release/emuera-tty GAME_DIR --headless --echo
./target/release/emuera-tty GAME_DIR --check     # compile every function, list parse errors
EMUERA_SEED=1 ...                                # deterministic RAND
```

`sample/` is the test game: `sample/ERB/TEST.ERB` is a language self-test,
and the integration tests run a copy of it headlessly (a self-test and a
save/load round trip).

- Each `CALL CHECK` / `CALL CHECKS` in TEST.ERB prints `OK  <name>` or
  `NG <name>`. The Rust test requires a minimum count (`oks >= 104` in
  `tests/sample_selftest.rs`), so raise it when you add checks. A check in
  TEST.ERB is the usual way to cover a new builtin.
- Tests can also build a tiny game inline (see
  `dotrain_runs_after_the_event`): write `CSV/` and `ERB/` files into a
  temp dir and run the binary on it.
- Unit tests are `#[cfg(test)]` modules inside the source files.
- `sample/`'s save output (`*.sav`, `sav/`, `dat/`) is gitignored; the
  tests work on temp copies.

Useful environment variables: `EMUERA_TIMING=1` (load phases + memory),
`EMUERA_PROFILE=<file>` (ERB-level profile, any build), `EMUERA_PPROF=<file>`
(native profile; needs `--features profile`, written only on a clean exit),
`EMUERA_PARSE=all` (compile everything at load), `EMUERA_THREADS=N`,
`EMUERA_TRACE=<part of a function name>` (calls and results to stderr),
`EMUERA_MEMSTATS=<file>`, `EMUERA_LIST_BUTTONS=1` (headless prints the
buttons' values before each input, for scripted play),
`EMUERA_INJECT="STMT||STMT"` with `EMUERA_INJECT_AT=n` (run statements
before the n-th input), `EMUERA_LINT=1` with `--check` (statements the
parser probably split wrongly), `EMUERA_MOUSE_LOG=<file>`,
`EMUERA_AMBIGUOUS=wide|narrow|osc66|single` (also `--ambiguous`),
`EMUERA_MOUSE=mice|off|term`. `--strict` makes unimplemented commands
errors instead of warnings. The translation log is set with
`--translate-log FILE` (append), `--translate-load FILE` (read) and
`--translate-storage FILE` (both), not with variables. The full list is in the README's *Reference*
section.

For trustworthy native profiles use a release-equivalent build:
`CARGO_TARGET_DIR=target/prof CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release --features profile`
(the `fastbuild` profile has no LTO, so inlined wrappers look hot).

## Architecture

Startup (`main.rs`): read CSVs (`csvdata.rs`) → list ERH/ERB files in
Emuera's order (`loader::emuera_files` / `erb_files`, honouring the game
config's "search subfolders" / "sort by file name") → parse ERH headers
(`#DEFINE`, global `#DIM`) then ERB files on all cores (`parser.rs`, merged
in file order) → pick a frontend → run the VM's system loop on a thread
with a very large stack.

- **Lazy compilation.** At load only each function's `@` line and attributes
  are parsed; bodies are compiled on first call by re-reading the file.
  Parse errors in a body surface when it first runs (or with `--check`).
  A line that fails to parse becomes `Instr::BadLine` so jumps still line up.
- **Parser → flat instructions.** Function bodies compile to flat `Instr`
  vectors with patched jumps (IF/ELSE/loops/SIF), so GOTO and loops need no
  recursion. `expr.rs` parses expressions and FORM strings (`%…%`, `{…}`,
  `\@ ? # \@`).
- **VM (`vm/`).** `mod.rs`: variable resolution and the instruction loop.
  Every variable is an `Arr` (`vars.rs`), even scalars; `Vm::locate` turns a
  `VarRef` into a `Place` (`Loc` + flat index) and all reads/writes go
  through places (`get_place` / `set_place`). Where a variable lives
  depends on its kind: globals, `Chara::vars` (created lazily per
  character), `statics[FUNC][NAME]` for LOCAL / ARG / non-DYNAMIC private
  `#DIM`s, the call's frame for `#DIM DYNAMIC`, an alias for `#DIM REF`
  (the table is in `mod.rs`'s header). A user `CALL` is a Rust call;
  `BEGIN`, `QUIT` and errors unwind as `Err(Ctrl)` back to the system
  loop. `system.rs` is the TITLE/SHOP/TRAIN/… state machine,
  `builtins.rs` every Emuera / Emuera-EM / EE command and function.
- **Adding a builtin** (`builtins.rs`' header explains it). The parser
  accepts only names listed in `COMMANDS` or `FUNCTIONS`; any other name
  on a statement line parses as an assignment. Statements go through
  `Vm::command`, expression functions through `Vm::builtin_fn`, which
  returns `Ok(None)` for names it doesn't know. Arguments arrive
  unevaluated (`&[Expr]`): use the `arg` / `ai` / `as_` / `avar` helpers.
  A statement without Emuera's `METHOD_SAFE` flag (FunctionIdentifier.cs)
  also goes in `NOT_IN_FUNCTION`: a `#FUNCTION(S)` can't use it.
  The EM families have their own functions: `map_fn`, `dt_fn`
  (`vm/datatable.rs`), XML (`xml.rs`), .NET-style regex (`regex.rs`).
- **Saves.** `vm/erasav.rs` is Emuera's binary format (reader/writer);
  `vm/save.rs` decides what goes in, in Emuera's order (system variables,
  then the game's `#DIM SAVEDATA` in Emuera's creation order via
  `declared_dims`, then EM maps/XML/DataTables named in VarExt*.csv).
  `vm/textsav.rs` reads Emuera's text format (writing is always binary,
  which Emuera reads whatever its config says).
- **Console model (`console.rs`).** The VM only appends styled segments
  (text, buttons, images) to a scrollback; frontends lay it out and wrap.
  Buttons are live only for lines printed since the last input.
- **Frontends (`frontend.rs` trait).** `Headless` (stdin/stdout),
  `tty::Tty` (full-screen TUI: layout, mouse, colour downgrade, Emuera's
  two-column width rules in `char_cells`; `tty/gamecfg.rs` is the copy of
  Emuera's settings window, `tty/saves.rs` the save manager), `retro.rs`
  (terminfo-driven via `terminfo.rs`, for retro/serial terminals).
  `sys.rs` is the platform layer (raw mode, size, input polling).
- **Pictures.** `html.rs` (HTML_PRINT, `<div>` boxes), `resources.rs`
  (resources/*.csv sprites; decoding and scaling, cached, on all cores),
  then the `gfx/` backends (kitty, iTerm, sixel, Linux framebuffer).
  Overlapping layers are blended into one picture before sending. On the
  framebuffer, pictures are blitted after the text, over it.
- **Linux console specifics.** Mouse via gpm or `/dev/input/mice`
  (`mice.rs`), pointer drawn by emuera-tty as a blinking inverted cell;
  characters missing from the console font are drawn as lookalikes
  (`console_lookalike`, font map read with GIO_UNIMAP). Linux-only pieces
  have stubs for other systems (`mice_none.rs`, `gfx/fb_none.rs`); Unix
  code goes through `libc`, Windows through `windows-sys`.
- **Around the game.** `picker.rs` (game library/launcher), `archive.rs`
  (games played from their download archive), `settings.rs`
  (F3 / Ctrl-O settings, `~/.config/emuera-tty/config`), `emuconfig.rs` (Emuera's
  `emuera.config` / `_default.config` / `_fixed.config`, Japanese or
  English keys from `config_names.rs`), `mt.rs` (live dialogue
  translation), `translate.rs` (Emuera-Anchor's `*_TR.csv` / `TRCHARA`
  files), `sound.rs` (external players).

## Conventions and constraints

- **Emuera is the reference.** When behaviour is in doubt, read Emuera's
  C# source rather than guessing: Emuera.em (EM + EE),
  https://gitlab.com/EvilMask/emuera.em, checked out locally as described
  in *Reference sources* below. Comments here often name the Emuera
  function they mirror (`beginDataLoaded`, `SaveToStreamBinary`…), and
  module docs explain *what Emuera does* and why ("as in Emuera"): keep
  that style. An offline copy of the Emuera-EM documentation (command
  reference, eramaker spec) is in the EraSlop repository on gitgud.io
  (`era-games/EraSlop`, `.agents/eradoc/docs`); where it disagrees with
  the source, the source wins. Emuera itself can't run on Linux here (no
  .NET/Wine), so compatibility is checked against its source and against
  files it wrote.
- **Bug-for-bug only where games need it.** Copy Emuera's (or Anchor's)
  behaviour, quirks included, where not doing so would make a game not
  work or not work as expected. Otherwise differing to fix a bug or do
  the right thing is fine, as long as saves stay compatible both ways and
  nothing the games rely on in Emuera breaks. Record every deliberate
  difference with its reason: Anchor's in `doc/anchor.md` (*Deliberate
  differences*), EM/EE's in `doc/differences.md`. Errors are not
  lenient: what is an error in both Emuera-EM/EE and Anchor is an error
  here too, so games written on emuera-tty run on the others.
- **Saves are byte-compatible with Emuera, both ways.** Players switch
  between Emuera.exe and emuera-tty on the same saves. Never write anything
  Emuera can't load. Any change near `vm/save.rs`, `erasav.rs`,
  `textsav.rs`, `savemgr.rs`, `#DIM` handling or file load order needs the
  save round trip below. Things that already matter for byte identity:
  `#DIM` creation order (`declared_dims`, which depends on the file order
  in `loader::emuera_files`), and `Chara::names_unset` (a character name
  never set is null in Emuera and left out of the save; one set to "" is
  written).
- **Low memory.** It must run on a 512 MB Raspberry Pi Zero (ARMv6): lazy
  per-function compile, compiled code dropped beyond `EMUERA_CODE_MB`
  (default RAM/16), graphics buffers RLE-packed beyond `EMUERA_GBUF_MB`
  (RAM/8). AnonTW peaks around 145 MB, pops-tw around 350 MB. After changes
  to the parser, AST node sizes or resource caches, check peak memory
  (`EMUERA_TIMING=1`, `EMUERA_MEMSTATS=<file>`).
- **Odd CPUs.** i586, ARMv5–7, AArch64, RISC-V 64, PowerPC 64 (both
  endians) and LoongArch run the games under qemu-user with output
  identical to x86-64; MIPS, m68k, s390x and others are compile-checked.
  Use `crate::atomic64::AtomicU64`, never the std one; keep `tar` without
  default features (its xattr/rustix dependency doesn't build everywhere).
  No CPU-specific code. Runnable builds: a musl target with
  `RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static"`,
  then `qemu-<arch> ./emuera-tty …` (without an address-space cap, or qemu
  can't map the guest). The README's "Other CPUs" section has the list.
- **Docs track behaviour.** User-visible changes update `README.md` and the
  man page `doc/emuera-tty.1` (`--manual` prints it; it is compiled into
  the binary from `src/picker.rs`).
- **The maintainer's round:** fix, look for optimisations, update the
  README, test, `cargo fmt`, commit, push to `origin master`, then
  `./target/release/emuera-tty --install`. Commit messages: a one-line
  summary of the user-visible result (often prefixed by area, e.g.
  `Linux console: …`), then a short paragraph on why.

## The ERB test suite

`tests/erb_suite.rs` runs each `tests/erb/*.erb` as a tiny game (the
sample's CSVs + `tests/erb/_helpers.erb` + the file, optional
`<name>.erh`), headless with `EMUERA_LOG_STYLE=emuera` (output as Emuera's
log has it). A file defines `@TEST_MAIN` with `CALL CHECK, "what", cond` /
`CALL CHECKS, "what", got, want`; `;@input` lines are typed in order; a file
with its own `@SYSTEM_TITLE` runs the system flow and calls `TEST_DONE`.
`tests/golden/*.erb` must print exactly their `.out`; `tests/save/` must
save byte-identical files (`UPDATE_GOLDEN=1` regenerates both, only after
checking the new output is what Emuera does).

**Expectations are Emuera's, not ours.** Before writing or changing an
expected value, check Emuera's source; when a check fails, decide from the
source whether the test or the engine is wrong (this suite found real
engine bugs: CURRENTALIGN's type, SORTCHARA's key, ADDVOIDCHARA's NO,
SPRITEDISPOSEALL, CALLEVENT, ONEINPUT's first character, PRINTN's wait).
`tools/erb_suite_export.sh` + `tools/erb_suite_compare.py` run the suite on
the real Emuera (Windows); `tools/erb_coverage.py` lists the built-ins no
test uses yet. Add every fixed bug as a check in its topic file.

## Testing beyond `cargo test`

Real games used for testing are big eraTW variants (AnonTW: ~3,500 files,
90,000 functions; pops-tw: 100,000 functions). Rules learnt the hard way:

- **Never touch the real game folders.** Games write saves and `global.sav`
  on start. Copy `ERB/` and `CSV/` (plus `sav/` if needed) to a scratch
  folder; `resources/` and `sound/` only when pictures or sound matter.
- **At most two game processes at once, each memory-capped**
  (`ulimit -v 4000000` or RLIMIT_AS): six parallel AnonTW runs once
  crashed a 7 GB machine.
- Don't use `pkill -f` / `pgrep -f` with a pattern: it matches the shell
  running it. Track PIDs.

Tools in `tools/` (Python 3; the screen-reading ones need `python3-pyte`):

- `tools/game_smoke.sh GAME_DIR [STEPS] [SEED]`: random play of a real
  game in a throwaway copy (memory-capped), failing on script errors.

- `tools/gamed.py` + `tools/g.sh`: keep a game running in a pseudo-terminal
  and drive it command by command (type, key, click a label), reading the
  settled screen back. For anything the headless mode can't reach (mouse,
  INPUTMOUSEKEY menus, rendering). pops-tw's title is an INPUTMOUSEKEY
  picture menu, so it can only be played through this, not `--headless`.
- `tools/savdiff.py`: compare two binary saves: payload byte identity,
  differing variables, entry order. Its docstring has the round-trip
  recipe: an `@SYSTEM_LOADEND` hook that `SAVEDATA`s and `QUIT`s right
  after loading an Emuera-made save.
- `tools/console_mouse.py`: the Linux-console mouse pointer under a fake
  gpm (`EMUERA_GPMCTL`, `EMUERA_GPM_VC`), printing reversed cells with their
  colours.

What only a human can check: the real Linux console (its font's glyph
map via GIO_UNIMAP works only on the VT emuera-tty runs on; framebuffer
pictures; 16 colours) and real terminals' image protocols. To test in a
real Konsole without disturbing the desktop, run it on a hidden X server:
`Xvfb :99 &`, then `env -u WAYLAND_DISPLAY DISPLAY=:99 QT_QPA_PLATFORM=xcb
dbus-run-session konsole --nofork --separate -e …`, drive it with
`xdotool` and capture with `import -window root shot.png` (without
those variables Konsole opens on the user's Wayland session).

## Reference sources

Two C# sources are expected as sibling checkouts of this repo (paths
relative to its root). Neither is part of the repo; if one is missing,
ask before cloning it. Read them for behaviour only and don't copy code:
the README promises none is included. Their comments are mostly Japanese.

### Emuera-EM/EE: `../emuera.em`

Clone of https://gitlab.com/EvilMask/emuera.em, branch `master`. Check it
before implementing or fixing a command, the save format, config handling
or game flow. Paths are under `../emuera.em/Emuera/`; `Runtime/Script/` is
shortened to `Script/`.

| Topic | C# | here |
|---|---|---|
| Command table: every statement name, its argument type and flags | `Script/Statements/FunctionIdentifier.cs`, codes in `BuiltInFunctionCode.cs`, arg types in `FunctionArgType.cs` | `COMMANDS` in `vm/builtins.rs` |
| How each command's arguments are parsed | `Script/Statements/ArgumentBuilder.cs` (and `Argument.cs`) | `parser.rs` |
| Statement implementations (one `*_Instruction` class each) | `Script/Statements/Instraction.Child.cs` (sic) | `Vm::command` |
| Statements handled inline by the interpreter (PRINT_ABL…, PRINTBUTTON, QUIT, SAVEDATA, GETTIME, character add/delete…) | `Script/Process.ScriptProc.cs` (a big `switch` on `FunctionCode`) | `Vm::command` |
| Expression functions: the name table (incl. `MAP_*`, `DT_*`, `XML_*`) | `Script/Statements/Function/Creator.cs` | `FUNCTIONS`, `Vm::builtin_fn` |
| Expression function implementations (one `*Method` class each) | `Script/Statements/Function/Creator.Method.cs` | `Vm::builtin_fn`, `map_fn`, `dt_fn` |
| The DataTable stand-in, styled `<div>` boxes | `Runtime/Utils/EvilMask/Utils.cs` | `vm/datatable.rs`, `html.rs` |
| Operators, expression parsing | `Script/Statements/Expression/` (`ExpressionParser.cs`, `OperatorMethod.cs`) | `expr.rs` |
| Tokenising, FORM strings, line joining | `Script/Parser/LexicalAnalyzer.cs`, `LogicalLineParser.cs`, `Script/Data/StrForm.cs` | `expr.rs`, `parser.rs` |
| ERB loading, file order, `[SKIPSTART]` / `[IF]`, label resolution | `Script/Loader/ErbLoader.cs`, `Script/Data/LabelDictionary.cs` | `loader.rs`, `parser.rs` |
| ERH: `#DEFINE`, `#DIM` (creation order), `.ERD` files | `Script/Loader/ErhLoader.cs`, `Script/Data/UserDefinedVariable.cs`, `IdentifierDictionary.cs` | `parser.rs`, `vm/mod.rs` |
| CSV loading, name tables, VariableSize, GAMEBASE | `Script/Data/ConstantData.cs`, `GameBase.cs` | `csvdata.rs` |
| Built-in variables, their codes and default sizes | `Script/Statements/Variable/VariableCode.cs`, `VariableData.cs`, `CharacterData.cs` | `vm/vars.rs` |
| Variable reads/writes and many variable-level operations (character ops, array ops) | `Script/Statements/Variable/VariableEvaluator.cs` | `vm/mod.rs`, `vm/builtins.rs` |
| Game flow state machine (TITLE / SHOP / TRAIN…), BEGIN, SAVEGAME/LOADGAME menus | `Script/Process.SystemProc.cs`, `Process.State.cs` | `vm/system.rs` |
| CALL / JUMP / TRY / RETURN, event functions (#PRI/#LATER/#SINGLE/#ONLY) | `Script/Process.cs`, `Process.CalledFunction.cs` | `vm/mod.rs` |
| What goes into a save (`SaveToStream*`, `SaveChara`, `SaveGlobal`, `CheckData`) | `Script/Statements/Variable/VariableEvaluator.cs` | `vm/save.rs` |
| Binary save format | `Runtime/Utils/EraBinaryDataWriter.cs`, `EraBinaryDataReader.cs` | `vm/erasav.rs` |
| Text save format | `Runtime/Utils/EraDataStream.cs` | `vm/textsav.rs` |
| Config keys (Japanese + English), defaults, file precedence | `Runtime/Config/ConfigData.cs`, `Config.cs`, `ConfigCode.cs` | `config_names.rs`, `emuconfig.rs` |
| The settings window (tabs, fields) | `UI/Framework/Forms/ConfigDialog.cs` (+ `.Designer.cs` for layout) | `tty/gamecfg.rs` |
| Console model: lines, buttons, auto `[N]` buttons, wrapping, alignment | `UI/Game/EmueraConsole.cs`, `EmueraConsole.Print.cs`, `PrintStringBuffer.cs`, `ButtonStringCreator.cs`, `ConsoleDisplayLine.cs` | `console.rs`, `tty.rs` |
| HTML_PRINT tags and attributes | `UI/Game/HtmlManager.cs` | `html.rs` |
| Graphics buffers (`G*`), sprites, resources CSVs | `UI/Game/Image/GraphicsImage.cs`, `AppContents.cs`, `CroppedImage.cs` | `resources.rs`, `gfx/` |
| Input requests and timeouts (INPUT/TINPUT/ONEINPUT…) | `Runtime/InputRequest.cs`, `UI/Game/EmueraConsole.cs` | `frontend.rs` |
| Sound | `Runtime/Utils/Sound.NAudio.cs` | `sound.rs` |
| Regex (pattern cache) | `Runtime/Utils/RegexFactory.cs` | `regex.rs` |
| RNG (`MTRandom`, used by `VariableEvaluator`) | `Runtime/Utils/SFMT.cs` | — |
| Emuera's own UI / error message strings in several languages | `Runtime/Utils/EvilMask/Lang.cs`, `LangManager.cs` | — |

Feature docs and changelogs for Emuera, EM, EE and Anchor are in
`../emuera.em/Readme/` (`Emuera.EM_readme.txt`, `EmueraEE_readme*.txt`,
`readme_Translation(Anchor).txt`). `QuickStart/` is an alternative
fast-start loader and `PluginSystem/` a .NET plugin API; neither applies
here. Emuera-Anchor's translation system isn't in this tree.

### Emuera-Anchor: `../emuEra`

Clone of https://gitgud.io/Bepsi/emuEra. It is vanilla Emuera 1821 plus
Anchor's translation system, so use it for translation only and
`../emuera.em` for everything else.

**`doc/anchor.md`** records what Anchor does that EM/EE don't, with
file:line references, and what emuera-tty does about each. Read it before
touching `translate.rs` or anything translation-related. It also notes a
trap: Anchor's main loop runs with translation *on* (`Process.cs:252`), so
string assignments store translated text.

Key files, under `../emuEra/Emuera/`:

| Topic | C# |
|---|---|
| TR tables, the memory, `translateALL`, TRCHARA loading | `GameData/Variable/Translation.cs` |
| `TR_NAME` | `GameData/Function/TRCommands.cs` |
| `%…%` translation and fallbacks | `GameData/StrForm.cs` (`FormatPercent`) |
| Which statements pass the flag | `GameProc/Function/Instraction.Child.cs` (PRINT, HTML_PRINT, SET, RETURNF), `GameProc/Process.ScriptProc.cs` (PRINTPLAIN, shop) |
| Literal translation by line text | `GameData/Expression/Term.cs` (`SingleTerm`) |
| Name reads | `GameData/Variable/VariableToken.cs`, `VariableData.cs` (token registration) |
| PARAM.CSV, TRCHARA applied to characters | `GameData/ConstantData.cs`, `GameData/Expression/ExpressionParser.cs:256` |

## OpenSpec

Planned work lives in `openspec/` (OpenSpec CLI, `spec-driven` schema; set
`OPENSPEC_TELEMETRY=0`). `openspec list` shows active changes, and
`openspec instructions <artifact> --change <id>` gives the format for each
artifact. Validate with `openspec validate <id> --strict`. Current change:
`add-anchor-translation-parity` (Anchor's translation rules, TRCHARA,
PARAM.CSV). Its `design.md` records the decisions agreed with the
maintainer; don't re-open them without asking.
