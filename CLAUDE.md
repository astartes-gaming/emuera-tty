# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

emuera-tty is a Rust reimplementation of Emuera (the Windows/.NET engine for
eraBasic "ERB" games) that runs in a terminal. Fidelity to Emuera is the
goal: script semantics, layout, and save files that move both ways between
emuera-tty and Emuera.

## Commands

```sh
cargo build --release                       # LTO "fat", 1 codegen unit: slow to link
cargo build --profile fastbuild             # release speed without LTO, quicker
cargo test --release                        # unit tests + tests/sample_selftest.rs
cargo test --release emuera_name_order      # one test by name
cargo test --release --test sample_selftest # only the sample-game integration tests
cargo clippy --release                      # lint config lives in Cargo.toml [lints.clippy]
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

Useful environment variables: `EMUERA_TIMING=1` (load phases + memory),
`EMUERA_PROFILE=<file>` (ERB-level profile, any build), `EMUERA_PPROF=<file>`
(native profile; needs `--features profile`, written only on a clean exit),
`EMUERA_PARSE=all` (compile everything at load), `EMUERA_THREADS=N`,
`EMUERA_TRACE`, `EMUERA_MOUSE_LOG=<file>`, `EMUERA_TRANSLATE_LOG=<file>`,
`EMUERA_AMBIGUOUS=wide|narrow|osc66|single`, `EMUERA_MOUSE=mice|off|term`.

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
  through places (`get_place` / `set_place`). Character variables are
  created lazily per character in `Chara::vars`. A user `CALL` is a Rust
  call; `BEGIN`, `QUIT` and errors unwind as `Err(Ctrl)` back to the system
  loop. `system.rs` is the TITLE/SHOP/TRAIN/… state machine,
  `builtins.rs` every Emuera / Emuera-EM / EE command and function.
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
  two-column width rules in `char_cells`), `retro.rs` (terminfo-driven, for
  retro/serial terminals). Images go through `gfx/` backends (kitty, iTerm,
  sixel, Linux framebuffer); on the framebuffer, pictures are blitted after
  the text, over it.
- **Linux console specifics.** Mouse via gpm or `/dev/input/mice`
  (`mice.rs`), pointer drawn by emuera-tty as a blinking inverted cell;
  characters missing from the console font are drawn as lookalikes
  (`console_lookalike`, font map read with GIO_UNIMAP).
- **Around the game.** `picker.rs` (game library/launcher), `settings.rs`
  (F1 settings, `~/.config/emuera-tty/config`), `emuconfig.rs` (Emuera's
  `emuera.config` / `_default.config` / `_fixed.config`), `mt.rs` +
  `translate.rs` (dialogue translation), `sound.rs` (external players).

## Conventions and constraints

- **Emuera is the reference.** When behaviour is in doubt, read Emuera's
  source (the user keeps a copy at `~/Documents/emuera.em`) rather than
  guessing; comments in this codebase often name the Emuera function they
  mirror. Saves must stay byte-compatible with Emuera's.
- **Low memory and odd CPUs.** It must run on a 512 MB Raspberry Pi Zero
  (ARMv6) and builds for RISC-V, big-endian PowerPC, LoongArch, MIPS,
  m68k, s390x. Use `crate::atomic64` instead of `AtomicU64`; the `tar`
  crate is used without default features (no xattr).
- **Docs track behaviour.** User-visible changes update `README.md` and the
  man page `doc/emuera-tty.1`.
- **Don't modify real game folders** when testing; work on a scratch copy
  (games write saves and `global.sav` on start). Big games use several GB
  of address space during parallel loading, so cap memory (`ulimit -v`)
  and don't run many at once.
- Commit messages: a one-line summary, then a short paragraph on why.
