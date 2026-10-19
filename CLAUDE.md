# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

emuera-tty is a Rust reimplementation of Emuera (the Windows/.NET engine for
eraBasic "ERB" games) that runs in a terminal. Fidelity to Emuera is the
goal: script semantics, layout, and save files that move both ways between
emuera-tty and Emuera.

## Dependencies

Package names are Debian/Ubuntu's.

- **Building:** Rust from rustup (stable; developed on 1.99) and git. Crate
  dependencies come through cargo; there are no system libraries to
  install.
- **The test scripts in `tools/`:** Python 3 and pyte (`python3-pyte`,
  or `pip install pyte`) for `gamed.py` and `console_mouse.py`;
  `savdiff.py` needs only Python 3. `gamed.py` reads `/proc`, so it runs
  on Linux only.
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
  C# source rather than guessing: Emuera.em (EM + EE),
  https://gitlab.com/EvilMask/emuera.em. Comments here often name the
  Emuera function they mirror (`beginDataLoaded`, `SaveToStreamBinary`…).
  An offline copy of the Emuera-EM documentation (command reference,
  eramaker spec) is in the EraSlop repository on gitgud.io
  (`era-games/EraSlop`, `.agents/eradoc/docs`); where it disagrees with
  the source, the source wins. Emuera itself can't run on Linux here (no
  .NET/Wine), so compatibility is checked against its source and against
  files it wrote.
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
  Runnable builds: a musl target with
  `RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static"`,
  then `qemu-<arch> ./emuera-tty …` (without an address-space cap, or qemu
  can't map the guest). The README's "Other CPUs" section has the list.
- **Docs track behaviour.** User-visible changes update `README.md` and the
  man page `doc/emuera-tty.1` (`--manual` prints it).
- **The maintainer's round:** fix, look for optimisations, update the
  README, test, commit, push to `origin master`, then
  `./target/release/emuera-tty --install`. Commit messages: a one-line
  summary, then a short paragraph on why.

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
