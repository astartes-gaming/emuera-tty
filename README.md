# emuera-tty

> **⚠️ Read this first: this is a vibecoded proof of concept.**
>
> This project was made by a self-described dumbass who doesn't really know
> what they're doing, by asking an AI (Claude) to write it. Nearly all of the
> code was AI-generated, reviewed only by running the game and seeing what
> breaks. It is not a serious or complete Emuera port, nobody has audited it,
> and it will mis-render, misbehave or crash on plenty of scripts.
>
> - Expect bugs. emuera-tty keeps a history of every save it overwrites
>   (see *Managing saves*), but keep your own copy of saves you care about.
> - It is not affiliated with Emuera, Emuera-EM/EE, eratohoTW or AnonTW.
>   Where behaviour was copied, it was by reading
>   [EvilMask's emuera.em](https://gitlab.com/EvilMask/emuera.em) source for
>   reference; no code from it is included.
> - No game files are included. You need your own copy of the game.
> - MIT licensed (see [LICENSE](LICENSE)).
>
> If it works for you anyway: nice. If it doesn't: that's the expected
> outcome of a proof of concept.

## Contents

- [Guide](#guide)
  - [1. What you need](#1-what-you-need)
  - [2. Install](#2-install)
    - [Build status](#build-status)
    - [Other systems](#other-systems)
    - [Other CPUs](#other-cpus)
    - [Small machines (Raspberry Pi Zero and the like)](#small-machines-raspberry-pi-zero-and-the-like)
  - [3. First run: choose your game](#3-first-run-choose-your-game)
  - [4. Playing](#4-playing)
    - [Settings](#settings)
    - [Game settings (Emuera's settings window)](#game-settings-emueras-settings-window)
    - [Copying text to the clipboard](#copying-text-to-the-clipboard)
    - [Translated games (Emuera-Anchor)](#translated-games-emuera-anchor)
    - [Managing saves](#managing-saves)
  - [5. Picking a terminal](#5-picking-a-terminal)
  - [6. Updating and removing](#6-updating-and-removing)
  - [7. When something goes wrong](#7-when-something-goes-wrong)
- [Reference](#reference)
  - [Command-line options](#command-line-options)
    - [Memory](#memory)
    - [Mouse](#mouse)
  - [Graphics backends](#graphics-backends)
  - [Layout](#layout)
  - [Implemented](#implemented)
  - [Roadmap](#roadmap)

A from-scratch reimplementation of the **Emuera** ERB engine in Rust that
plays era games in a terminal: graphics-capable terminals (kitty, Konsole,
WezTerm, Ghostty, foot, iTerm2, xterm with sixel…), plain ones (pictures are
left out, text and colours stay), the Linux framebuffer console, and even old
serial terminals.

Status: **proof of concept**, but it plays. Tested mostly with **AnonTW**
(eratohoTW in English), **pops-tw** (eraNAS) and **eraTohoK**: new games,
days, training, shops, maps and saves work, and automated play runs
thousands of random choices through them without errors. What it has
(details under *Implemented*):

- Emuera / Emuera-EM / EE scripts, HTML_PRINT with `<div>` boxes, pictures,
  sprites and graphics buffers, tooltips, the mouse (hover, click, right-click
  skip), and the games' own settings window.
- Saves in Emuera's own format, in both directions.
- Sound through mpv / ffplay / other players, and live **translation** of
  Japanese or English games into other languages (the dialog or all text)
  with a translator on your computer (or DeepL / translate-shell if you
  choose).
- A game library with archives and `git pull` updates, a settings page,
  save management, and readable script errors with the call stack.
- Linux, and builds for macOS, Windows, the BSDs, Termux and more, down to a
  Raspberry Pi Zero.

Expect rough edges in parts of the games nobody has played through yet
(see *Roadmap*).

# Guide

## 1. What you need

- **Linux**. It also builds for **macOS**, the **BSDs** (FreeBSD, NetBSD,
  OpenBSD, DragonFly), **Windows 10/11** (works in either the base `cmd` or
  WezTerm), **Termux** on Android (having an OTG mouse + keyboard on hand
  and using the device in landscape mode is recommended), **illumos /
  Solaris**, **Haiku** and **Redox**, but **only Linux is tested: every
  other system is compile-checked only and untested.** Reports are
  welcome. See [Other systems](#other-systems) for the differences,
  [Other CPUs](#other-cpus) for ARM, RISC-V, PowerPC, MIPS and the rest,
  and [Small machines](#small-machines-raspberry-pi-zero-and-the-like) for
  the Raspberry Pi Zero and other 512 MB boards.
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
- Optional: **mpv** (or ffplay) for the games' sound and music, and a
  translator such as **Ollama** or **Argos Translate** to read a game in
  another language (see [Playing](#4-playing)).

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

### Build status

| OS/Architecture | Status | Notes |
|---|---|---|
| Linux x86-64 | Build passed | Most common platform, under constant testing |
| Linux x86-64 (static) | Build passed | Untested, but should work the same as above |
| Windows x86-64 | Build passed | Tested launching and basic playthrough, but nothing further than that yet |
| Windows ARM64 | Build passed | Untested |
| macOS ARM64 (Apple silicon) | Build passed | Untested |
| macOS x86-64 | Build passed | Untested |
| Linux i586 (static) | Build passed | Untested |
| Linux ARMv5 (static) | Build passed | Untested |
| Linux ARMv6 (static) | Build passed | Untested |
| Linux ARMv7 (static) | Build passed | Untested |
| Linux AArch64 (static) | Build passed | Untested |
| Linux RISC-V 64 (static) | Build passed | Untested |
| Linux PowerPC 64, big-endian (static) | Build passed | Untested |
| Linux PowerPC 64, little-endian (static) | Build passed | Untested |
| Linux LoongArch 64 (static) | Build passed | Untested |
| FreeBSD x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| NetBSD x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| OpenBSD x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| DragonFly BSD x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| illumos x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Solaris x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Haiku x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Redox x86-64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Android (Termux, AArch64) | Build success | Tested on actual devices. Sparsely tested, works under Termux, should also work in any terminal environment, and does run under Android Recovery (TWRP)... somehow |
| Linux RISC-V 32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux PowerPC 32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux IBM Z (s390x) | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux SPARC 32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux SPARC 64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 32, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 32, little-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 64, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 64, little-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux Motorola 68000 | Build failed (under actions), check failed | Fails under Github Actions, needs testing on emulated/real hardware instead. Very likely might not compile at all |
| Linux Hexagon | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux C-SKY | Build failed (under actions), check failed | Fails under Github Actions, needs testing on emulated/real hardware instead. Very likely might not compile at all |
| Linux ARM, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux AArch64, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux x32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |

### Other systems

> **Untested.** emuera-tty is developed and played on Linux. Everything
> below is checked only by compiling for that system (`cargo check
> --target …`); none of it has been run there yet.

- **macOS / BSD**: the same steps. On macOS, `~/.local/bin` is usually not on
  PATH, so `--install` picks a writable PATH folder (Homebrew's
  `/usr/local/bin`) or tells you how to add one. Settings live in
  `~/.config/emuera-tty`. The Linux-console extras (framebuffer pictures,
  gpm / `/dev/input/mice` mouse) are Linux-only; everything that goes
  through the terminal works the same. Fonts for picture text come from
  fontconfig when it's installed, else the system's Japanese fonts
  (Hiragino on macOS).
- **Windows**: install Rust with `rustup-init.exe` (the MSVC toolchain, as
  rustup suggests), then in PowerShell:
  `git clone …`, `cd emuera-tty`, `cargo build --release`,
  `.\target\release\emuera-tty.exe --install`. Run it in **Windows Terminal**
  (mouse, true colour and sixel pictures in recent versions); the old
  console window works too but without pictures. Settings go to
  `%APPDATA%\emuera-tty`, extracted games to `%LOCALAPPDATA%\emuera-tty`;
  game saves stay in the game's `sav\` as with Emuera, so you can switch
  between Emuera.exe and emuera-tty. `--serial COM3` drives a terminal on a
  COM port. Picture text uses MS Gothic / Meiryo / Yu Gothic from
  `C:\Windows\Fonts`.
- **Termux** (Android): `pkg install rust git`, then the same steps as on
  Linux; `--install` puts the program in Termux's `$PREFIX/bin`. Pictures
  show if Termux's terminal answers that it can draw sixels (auto
  detection), else placeholders.
- **Haiku**, **Redox**, **illumos / Solaris**, **NetBSD / OpenBSD /
  DragonFly**: the Unix code path (termios, `poll`), as on Linux minus the
  Linux-console extras. Haiku's Terminal and Redox's console show text
  only; on Haiku the fastest serial speeds (460800, 921600) aren't
  offered, on Redox there is no RTS/CTS flow control.

### Other CPUs

Everything above is about x86-64 PCs. emuera-tty is plain Rust with no
CPU-specific code, so it builds for most processors Linux runs on. Saves
are Emuera's little-endian format on every CPU: a save made on a
big-endian machine is byte for byte the one a PC makes. **Tried only
under emulation (`qemu-user`), not on real machines.**

| | CPUs |
|---|---|
| **Run under emulation**: the sample game's self-test, a full 800-input AnonTW session with the same output as on a PC, saves loaded across | x86 32-bit (i586), ARMv5 / v6 / v7, AArch64, RISC-V 64, PowerPC 64 (big- and little-endian), LoongArch 64 |
| **Compile-checked** (no C library here to link against) | RISC-V 32, PowerPC 32, IBM Z (s390x), SPARC 32 / 64, MIPS 32 / 64 (both byte orders), Motorola 68000, Hexagon, C-SKY, big-endian ARM / AArch64, x32 |
| **Not possible**: Rust has no code generator for them | DEC Alpha, PA-RISC (HPPA), SuperH (SH-4), Itanium, OpenRISC, MicroBlaze, Xtensa |

The emulated builds are static musl programs, made on a PC with Rust's
own linker (no cross compiler needed): add the target with `rustup target
add`, then build with
`RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static" cargo build --release --target …`
for `i586-unknown-linux-musl`, `arm-unknown-linux-musleabihf`,
`armv5te-unknown-linux-musleabi`, `armv7-unknown-linux-musleabihf`,
`aarch64-unknown-linux-musl`, `riscv64gc-unknown-linux-musl`,
`powerpc64-unknown-linux-musl`, `powerpc64le-unknown-linux-musl` or
`loongarch64-unknown-linux-musl`. The compile-checked ones need nightly
Rust (`cargo +nightly build -Z build-std=std,panic_abort --target …`)
for most, and the target's C library to link. On the machine itself,
`cargo build --release` works wherever Rust is installed.

### Small machines (Raspberry Pi Zero and the like)

emuera-tty keeps its memory low enough for a 512 MB board (see
[Memory](#memory)): AnonTW stays under 160 MB, pops-tw under 350 MB.
It runs on 32-bit ARM (Pi Zero / Zero W, ARMv6) and 64-bit ARM (Pi Zero 2,
Pi 3 and later). **Not tried on a real board yet**: the 32-bit ARM build
runs AnonTW under emulation (`qemu-arm`) with the same output as on a PC,
and both ARM builds compile.

Building on the board itself works (`cargo build --release`) but takes a
long time on a Zero; building on a PC and copying the program over is
quicker. For a Pi Zero (no cross compiler needed):

```bash
rustup target add arm-unknown-linux-musleabihf
```

```bash
RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static" cargo build --release --target arm-unknown-linux-musleabihf
```

then copy `target/arm-unknown-linux-musleabihf/release/emuera-tty` to the
Pi (use `aarch64-unknown-linux-musl` for a 64-bit Pi OS) and run
`./emuera-tty --install` there. On the Linux console with no X, pictures
go to the framebuffer; over SSH, use a terminal with sixel or kitty
graphics, or `--gfx none` for text only. A big game takes some seconds to
load on a Zero (it reads every script once).

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
straight away.

**Several games** (eraTW, eraMegaten, eraPoPs, any Emuera game): every game
you play or add goes into your **library**. Once there are two or more,
`emuera-tty` opens the launcher instead:

| Key | Action |
|---|---|
| Enter (or click twice) | play the selected game |
| `a` / **＋ Add a game…** | open the folder picker above to add a folder or archive (it only joins the list, marked gold **(NEW)** until you first play it; Enter then plays it) |
| **↓ Clone a game…** | ask for a git repository, the folder to keep it in and (optionally) a branch and a history depth, `git clone` it (Esc cancels) and add it to the list, marked gold **(NEW)** until you first play it; it doesn't start |
| `u` | update a game kept in a git checkout (`git pull --ff-only`; marked **· git** with its branch and short commit hash) |
| `U` (Shift-U) | update all of them, each checkout once, then sum up what changed |
| `g` | right after an update (`u` / `U`): show git's full output in a scrollable view (`g` is "go to top" otherwise) |
| `d` / Delete | take the game off the list (its files and saves stay) |
| `s` / **⚙ Settings** | the settings page (see [Settings](#settings)) |
| `q` / Esc | quit |

**Cloning from the launcher.** **↓ Clone a game…** (below *Add a game…*)
asks for the repository (URL or path), the folder to store it in (the
repository gets its own subfolder there, named like `git clone` would;
Enter takes the folder next to your last game) and, if you want them, the
branch to check out straight away (Enter for the repository's default) and
a history depth for big repositories (`1` fetches only the latest commit,
Enter the full history; a shallow clone still updates with `u`). It shows
git's progress. Each field starts from what you gave last time, shown in
[brackets]: Enter on an empty field reuses it, and **Tab** puts it in the
field for editing (clear it to leave the field empty, e.g. to drop a
remembered branch). **Esc** or **Ctrl-C** cancels the clone (git cleans up
after itself; if a half-cloned folder is left, it asks before removing it).
Otherwise it adds the game to the list without starting it,
marked **(NEW)** in gold until you first play it (games added with *Add a game…* get the same mark). `g` shows git's full output
of the clone, also when it failed. A game kept in a subfolder of the
repository (up to two levels down) is found too.

**Games cloned with git** (`git clone` of a game's repository) update with
`u` in the launcher: it pulls the new commits and says how many came in,
with the commit it moved from and to (`abc1234 -> def5678`). Each such game
is listed with the branch and short commit hash it is on
(`· git master @ abc1234`), and `g` shows git's full output of the update
just done. It only fast-forwards and never asks for a password, so a game
you have edited, or a repository that needs a login, reports why it
couldn't update instead (commit or stash your edits, or run `git pull` yourself).
Saves in the game folder that the repository ignores are left alone.
Games that keep their pictures in Git LFS (pops-tw) also get `git lfs
pull`. Without [git-lfs](https://git-lfs.com) a clone holds small pointer
files instead of the pictures, and scripts that measure a picture fail
(pops-tw's title: a division by zero in `@TITLE_NAS`). Install git-lfs and
run `git lfs pull` in the game folder (or `u` again); the script error
screen says so when it finds such pointers.

Inside a game, **Ctrl-O** → *Switch game…* goes back to the
launcher. Other ways to start:

```sh
emuera-tty --choose                 # open the launcher even with one game
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
  under the mouse are highlighted; a picture that is a button can be
  clicked anywhere on it. Resting the mouse on a button with a tooltip
  shows it after half a second (or the game's delay).
- **Continue** (when the game waits after text, `▼ (Enter)` at the
  bottom): Enter, any key, or a click.
- **Skip to the next choice**: right-click (or Tab) where the game waits:
  the text that follows runs by without stopping until the game asks for
  a number or a choice, as Emuera's right click does. Right-clicking a
  choice picks it and skips the text after it. (A `FORCEWAIT` still
  stops, and the EE `INPUT default, mouse, canskip` takes its default.)
- **Typing**: ← / → move in the line being typed, Home / End go to its
  start and end, Delete and Backspace erase, Ctrl-U or Esc clear it.
  ↑ / ↓ go through your last 100 inputs, as in Emuera: typed ones and
  buttons clicked alike. One brought back is shown selected, so typing
  replaces it and ← / → edit it.
- **Macros** (Emuera's keyboard macros): one typed line can answer
  several questions. `\n` separates the answers, `\e` after one skips the
  text that follows it (as a right click), and `(…)*n` repeats:
  `(0\e\n)*3` answers `0` three times, skipping after each. Esc stops a
  macro that is running. Shift+F1…F12 store the line being typed under
  that key, and F1…F12 put it back (at a `▼` wait, the key gives it at
  once). Alt+0…9 (also Ctrl+0…9 in terminals that report it, such as
  kitty) switches between ten groups of twelve keys, and the group's name
  shows for a moment. They are kept in the game folder's `macro.txt`,
  the file Emuera.exe uses, so macros made in either work in both. On the
  Linux console Shift+F1 / F2 are F11 / F12, and Shift+F11 / F12 don't
  exist. The game setting *Use keyboard macros* (on by default, as in
  Emuera) gives the F keys to the macros, but F3 and F5 stay the settings
  and reload keys as long as no macro is stored on them (where Emuera
  would only empty the line); with it off they always are. Typed text is
  expanded either way, as in Emuera.
- **Scroll back** through earlier text: PgUp / PgDn, Ctrl+↑ / ↓ (or
  Alt+↑ / ↓) a line at a time, or the mouse wheel. ↑ / ↓ scroll too
  where the game waits (`▼`), and Home / End while nothing is typed. A
  `[scroll -N]` tag shows you're looking at history; End jumps back.
- **Settings**: F3 or Ctrl-O opens them over the game (see below); F3
  not while a keyboard macro is stored on it.
- **Back to the title**: Ctrl-T. **Restart** the game: Ctrl-R (both ask
  first; unsaved progress is lost). **Reload the ERB files** after editing
  them: Ctrl-E or F5 (unless a macro is stored on F5): the game carries on where it was
  with the new code (Emuera-Anchor's hotkeys, as in Emuera-EE; also in
  the settings).
- **Redraw** a garbled screen: Ctrl-L. **Quit**: Ctrl-C (save first!).
  Ctrl-C also works while the game is busy computing, so a script stuck
  in a loop can always be stopped; the screen keeps updating meanwhile.
- **The bottom row** shows, on the right, the number the button under the
  mouse sends (`→ 105`, also for buttons that don't show it, like map
  squares) and the function asking for input. While a script computes
  for more than a moment it says `… working @FUNCTION`. (*Bottom bar
  details* in the settings turns this off.) The memory emuera-tty uses is
  on the settings page's title, and in the bottom row too with *Show memory*
  (Linux, Android and Windows); both also show the highest use seen this
  session once it is above the current one (`123 MB (peak 150 MB)`).

**Sound**: PLAYSOUND / PLAYBGM play the game's `sound/` files through a
player program the system has: `mpv` or `ffplay` (any format, including
the `.m4a` many games use), `mplayer`, `afplay` on macOS, or for WAV
only `pw-play`, `paplay`, `aplay` and PowerShell on Windows. Install mpv
for full sound (`pkg install mpv` on Termux). Ten effect channels and a
looping BGM, as Emuera-EE; volume changes apply to sounds started after
them. *Sound* in the settings turns it off and on while you play (the BGM picks
up again); it is off over SSH unless set to `on` (the sound would play on
the remote machine). `EMUERA_SOUND=on|off` and `EMUERA_SOUND_PLAYER=mpv`
override.

**MIDI music** plays through [FluidSynth](https://www.fluidsynth.org),
found in `PATH` like the other players, with the game's `soundfont.sf2`
if it has one, else the system's:

- Linux: `sudo apt install fluidsynth fluid-soundfont-gm` (Fedora:
  `sudo dnf install fluidsynth fluid-soundfont-gm`).
- Windows: download the Windows zip from
  [FluidSynth's releases](https://github.com/FluidSynth/fluidsynth/releases),
  extract it, and add its `bin` folder to your `PATH` (*Edit environment
  variables for your account* → `Path` → *New*); open a new terminal
  afterwards.
- macOS: `brew install fluidsynth` (untested).

Games that don't bring a SoundFont need one on the system; on Windows and
macOS emuera-tty finds none by itself, so MIDI there plays only with a
game's own `soundfont.sf2`. Games that come with
**eraMidi** (`eramidi.exe`, as eratohoReverse does) get their music
without it: eraMidi watches Emuera-Anchor's clipboard for a line with a ♪
(the song title the game shows) and plays `media/reverse/<title>.mid`;
emuera-tty reads its own printed lines the same way, so no clipboard
setting is needed. The song loops until another one starts; one that is
already playing isn't restarted.

**Translating the game**: in the settings, *Translate to* picks the language to
show the game in (en, es, pt, fr, de, it, ru, ja, zh, ko), *Translate
from* the game's own (auto: Japanese lines from Japanese, English ones from
English, so an English translation like AnonTW can be read in Spanish), and
*Translate what* either the characters' **dialog** or **all** text (menus,
buttons, labels too); *auto* (the default) is dialog when the game is shown
in English and all text in any other language. Text is translated as it
appears. Translation is off while *Translator* is *off* (the default). While it is off, *Translate to / from / what* and *Test the translator…* are hidden from the settings. With *Translator* on *auto* it uses whatever translator is there,
the ones **on this computer** first (nothing is sent online then): the
first it finds of

- [Ollama](https://ollama.com) running a model (`ollama pull qwen2.5:7b`
  translates Japanese well; `EMUERA_TRANSLATE_MODEL` picks the model),
- a [LibreTranslate](https://libretranslate.com) server on port 5000
  (`EMUERA_LIBRETRANSLATE=host:port` for another),
- `translateLocally` (Bergamot, `EMUERA_TRANSLATE_MODEL`, default
  `ja-en-tiny`), or Argos Translate (`pipx install argostranslate`, then
  `argospm install translate-ja_en`; one Argos process is kept running,
  so only the first line waits for the model to load),
- or any command in `EMUERA_TRANSLATE_CMD`: it gets a line on stdin and
  prints its translation (`EMUERA_TRANSLATE_FROM` and `EMUERA_TRANSLATE_TO`
  hold the languages).

and, when none of those is there, the online ones below. *Translator*
can also name one (`local` for this computer's only, `argos`), or an
**online** service through its command-line tool (these send the text over
the internet):

- `translate-shell`: the `trans` command (Google Translate by default;
  `EMUERA_TRANS_ENGINE=bing` and so on for another of its engines);
- `deepl`: DeepL, through `curl`. With your API key in `DEEPL_AUTH_KEY`
  (a free key works) it uses DeepL's API; without one, the free keyless
  endpoint DeepL's browser extension uses, asked the way
  [egg-translator](https://gitgud.io/Shakes4cc/egg-translator) does. That
  one is unofficial: it limits how fast you can go and may change; the
  settings say so when it refuses. (Without curl, DeepL's own `deepl` tool and the
  key do.)

*Dialog* is the lines printed by a character's dialogue code (the `口上`
files, `KOJO` functions) and lines in quotes (「」 『』, or "" in English);
menus, buttons and the rest stay as they are. *All* translates every piece
of text, keeping its colours and buttons, and sends only the words: the
`[100]` numbers, brackets and figures around them stay put, and buttons
still send the same numbers. **Menus keep their layout**: where a piece
sits in a column (two spaces or more before the next one), its
translation keeps its width, padded when shorter, using the spaces after
it when longer, and cut with `…` only past those; at the end of a line or
before ordinary text it takes the room it needs.

Dialog goes to the translator **a click at a time**: the lines a
character says before the next wait (a speech over several lines) are
sent together, so a sentence split over lines is translated whole and
with its context. The translation comes back line by line when the
translator keeps the lines, and is spread over them by length when it
doesn't (Argos). The log has one entry per batch, its lines joined by
spaces.

**Original or translation**: middle-click a translated line (anywhere,
also at a wait) to see the original text; middle-click again for the
translation. It is remembered per line for the session.

Maps and box drawings are never translated, nor text already in the
target language, and the game itself still sees the text it printed. Text
shows untranslated until its translation comes in, which happens in the
background; text seen again is instant. If the translator fails, the settings say
why on that row's help line.

**Test the translator…** in the settings translates a sample line with the chosen
translator and shows the result and how long it took, or why it failed.
**Clear the translation cache** forgets every translation made this
session (they are kept in memory only), so what is on screen is
translated again: after changing translator or fixing one that failed.

**A log for translators**: `EMUERA_TRANSLATE_LOG=translations.tsv
emuera-tty` appends every new translation to that file, one per line:
the ERB file and line that printed the text, the languages, the original
and the translation, separated by tabs, ready to find and fix in the
game's scripts:

```text
ERB/口上/K9.ERB:3	ja>en	「おはようございます、お嬢様」	"Good morning, my lady."
```

Dialog sent to the translator several lines at a time (a speech over
several lines) is logged one row per line, each with its own file and line,
when the translation comes back with as many lines; if it merged or split
them, the batch is one row.

**Saving** uses the game's own save menu, and the files are Emuera's own
(binary) format: `save00.sav`, `save01.sav`, … and `global.sav`, plus
`dat/chara_*.dat` for SAVECHARA. Saves move both ways between emuera-tty
and Emuera on Windows: loading an Emuera save and saving it again gives
the same bytes Emuera writes (the variables in Emuera's order, character
names never set left out as Emuera does), checked on AnonTW saves of
day 43 and day 1336. They go where Emuera puts them: in `sav/` when
the game's config says so (`セーブデータをsavフォルダ内に作成する`, as
AnonTW's does), else next to the game; compressed when it asks for that
(`セーブデータを圧縮して保存する`). Emuera-EM maps, XML documents and data
tables named in `VarExt.csv` are saved too, and the autosave runs on
entering the shop, like Emuera's. Older emuera-tty saves (`sav/*.rsav`)
still load, and the next save of that slot writes a `.sav`. Saves in
Emuera's text format (games whose config turns the binary option off)
load too; saving always writes the binary format, which Emuera reads
whatever its config says. After a load, `@SYSTEM_LOADEND` runs and then
`@EVENTLOAD`, as in Emuera.

ERB and ERH files load in Emuera's order: each folder's files before its
subfolders, sorted the way Emuera sorts them when the config's
`読み込み順をファイル名順にソートする` is on (else the order Windows lists
them in), and folders whose name has a `#` first (Emuera-EE). Subfolders
are read only when `サブディレクトリを検索する` is on, as in Emuera; a
note at startup says how many files that leaves out. The order decides
which of two functions with the same name wins, the order event
functions run in, and the order variables are saved in.

### Settings

F3 or Ctrl-O in a game (F3 unless a keyboard macro is stored on it), or *⚙ Settings* in the launcher. ↑ / ↓ choose,
← / → or Enter change, Esc closes; changes are saved at once in
`~/.config/emuera-tty/config` and, except where marked, apply to the
running game:

| Setting | Values | |
|---|---|---|
| Images | auto, kitty, iterm, sixel, fb, none | how pictures are drawn; `none`: pictures are never decoded or scaled (and those already held are freed) until another choice is made |
| Colours | auto, truecolor, 256, 16 | colour depth sent to the terminal |
| Readable colours | normal, high, off | lighten coloured text that is too dark for its background |
| Wide symbols | auto, wide, narrow, single, osc66 | how symbols get Emuera's two columns |
| Copy to clipboard | game, on, off | printed text to the clipboard (below); `game`: as emuera.config says |
| Bottom bar details | on, off | hovered button's value, running function |
| Keyboard pointer | auto, on, off | cursor keys move a pointer, Insert / Shift+Enter click |
| Show memory | off, on | memory in use in the bottom row too (always on the settings title) |
| Translator | off, auto, local, argos, translate-shell, deepl | off: no translation; auto: whichever is installed, this computer's first |
| Translate to | en, es, pt, fr, de, it, ru, ja, zh, ko | show the game in that language (see above) |
| Translate from | auto, ja, en | the game's language; auto: per line |
| Translate what | auto, dialog, all | characters' dialog, or all text; auto: all unless into English |
| Sound | auto, on, off | PLAYSOUND / PLAYBGM; `auto`: not over SSH |
| Wheel scrolls | 3, 1, 5, 10 | lines per wheel step |
| Console mouse | auto, off | gpm / `/dev/input/mice` (next start) |
| Display | auto, ansi, retro | `retro`: the ASCII terminfo frontend (next start) |

The overlay shows the *Random seed* in use: selectable for its description,
but read-only (start with `EMUERA_SEED=N` to choose it; the game's
`RANDOMIZE` / `INITRAND` change it). *Environment variables…* (shown only when one is
set) lists every `EMUERA_*` variable emuera-tty reads with the value it sees (● set, ○ not
set; Enter says what one does; read-only, as they are read at start). It also
has *Game settings…* and *Manage saves…* (below),
*Clear the picture cache* (frees the decoded sprites whatever *Images* says;
they load again when shown), *Recolor…* (draws a colour of the game's as another,
written `from:to` in hex, comma separated: `01183c:111d3b` turns AnonTW's time-stop
navy into a softer blue; the game still sees its own colours; also `EMUERA_RECOLOR`),
*Switch game…* and *Quit*. Command-line options and `EMUERA_*` variables
override saved settings.

### Game settings (Emuera's settings window)

*Game settings…* in the settings overlay is a copy of Emuera's own settings
window, with the settings that do something in a terminal and can't
break a game, on Emuera's tabs:

| Tab | Settings |
|---|---|
| Environment | make autosaves, lines in history log, number of save slots (20–80: the save menu's pages, `SAVENOS`) |
| Display | PRINTC items per line and width, don't break lines inside a button |
| Font | text colour, selected button colour, the font for text drawn into pictures |
| System2 | compress save data (Emuera reads compressed saves too) |
| Clipboard | all of the clipboard options (see below) |

Left out: what only a window has (size, position, FPS, menu bar,
keyboard macros), what changes how the scripts are read (the System,
Compatibility and Debug tabs), the font size and line height (games work
out where pictures go from them), the save folder (existing saves would
seem to vanish), the background and history colours (the terminal's own
are used) and Rikai.

← / → (or Tab, or a click) switch tabs, ↑ / ↓ choose, Enter or a click
toggles a check box or asks for a number, a colour (`R,G,B` or `#RRGGBB`)
or text, `+` / `-` step a number, `d` puts back the game's value. Each
setting shows where its value comes from: nothing for Emuera's default,
*game* for the game's (`CSV/_default.config`), *changed* for yours,
*fixed* when `CSV/_fixed.config` doesn't let it change. Changes go into
the game's `emuera.config` as Emuera's settings window writes them
(UTF-8, Japanese or English keys as the game's files use, other lines
kept), so Emuera.exe sees them too; the running game takes them when the
page closes.

### Copying text to the clipboard

For translation tools and text hookers, as Emuera-EE's clipboard feature
(from Emuera-Anchor): with `表示したテキストをクリップボードにコピーする:YES`
in `emuera.config` (or *Copy to clipboard: on*), the lines the game printed
since the last copy go to the clipboard when the game waits for input (and,
as configured, on WAIT, a left, middle or double click). The other
emuera.config options apply too: new lines only or the last *N*
(`クリップボードに貼り付ける行数`), the buffer size, the minimum time between
copies, and replacing pictures with a string (`テキスト中の<>タグを無視する`,
`<>を次の文で置き換える`). The copy is an OSC 52 escape, which kitty, WezTerm,
foot, Windows Terminal, iTerm2 and tmux (`set -g set-clipboard on`) pass to
the system clipboard; Konsole and some others need it allowed in their
settings.

### Translated games (Emuera-Anchor)

Games translated with Emuera-Anchor's system play translated here too:

- `Abl_TR.csv`, `Talent_TR.csv`, `Item_TR.csv`… (any CSV name table, also
  `Palam`/`Param`, `Str`, `CSTR`, `Flag`…) in the `CSV/` folder itself
  (not its subfolders, as Anchor): `original,translation` lines, `;`
  starts a comment. As in Anchor, only spaces before the original are
  dropped: `技巧, Technique` translates to " Technique", which some games
  use for alignment. Lines from a `{` line to a `}` line are read as
  one, as Anchor's file reader does. A line without exactly one comma, a
  misplaced `{` or `}`, or a name given twice, is shown as a warning when
  the game loads (with the file and line) and skipped; Anchor refuses to
  start the game instead.
- When a name comes out translated, as in Anchor: when a script prints
  it (`PRINTV TALENTNAME:LOCAL`, `PRINTFORM %TALENTNAME:LOCAL%`, PRINTS,
  PRINTDATA, HTML_PRINT), stores it in a string (`LOCALS '=
  TALENTNAME:LOCAL`, `LOCALS = [%TALENTNAME:LOCAL%]`, `+=`), or returns
  it with RETURNF from a `#FUNCTIONS` that is being printed or stored.
  Joining strings with `+` keeps the translation. Everything else sees
  the original name, so the scripts keep working: conditions (`IF
  TALENTNAME:LOCAL == "処女"`), CALL arguments, PRINTBUTTON's text, the
  branches of `? #`, and the arguments of built-in functions other than
  TOUPPER, TOLOWER, TOHALF, TOFULL, REPLACE and STRFORM. The values of
  STR, TSTR, SAVESTR, RESULTS and GLOBALS translate the same way, through
  `Str_TR.csv`. Because stored text is translated, saves made in a
  translated game hold the translations, as Anchor's do (Emuera loads
  them like any other save; saves from older emuera-tty versions hold the
  originals). PRINT_ABL and the like, the TRAIN command list, the shop
  list and PRINTDATA are translated too, though Anchor prints those
  untranslated.
- A name with a fixed index (`TALENTNAME:5`) follows Anchor's own rule:
  inside a comparison, a calculation, a function call or a `%…%` it is
  translated even in a condition (`IF TALENTNAME:5 == "処女"` is false in
  a translated game, as in Anchor), while stored or returned on its own
  (`LOCALS '= TALENTNAME:5`) it stays the original. Quoted text printed
  with PRINTS, PRINTV or PRINTPLAIN (`PRINTS "男"`) is looked up in
  `ALL_TR.csv`, or in the table of the first `XXXNAME` on the same line.
  STRLENS and STRLENSU measure the translated text, so layouts built on
  them fit the translation.
- `ALL_TR.csv`: also looked up for a whole `%…%` value when it is
  printed or stored (or has a width, `%X,10%`). Failing that, as in
  Anchor, a name the script read untranslated shortly before (Anchor
  remembers the last 19) is translated inside the value, and then the
  value without its digits, `LV` and brackets is tried (`技巧LV3` →
  `TechniqueLV3`); a `%…%` in a RETURNF line gets none of this.
- `TRCHARA*.csv` (any case, anywhere under `CSV/`): character names
  (`NAME`, `CALLNAME`, `NICKNAME`, `MASTERNAME`, `CSTR,n,text`) in blocks
  from `NO,n` (or `番号,n`) to `ENDCHR` or `ENDCHAR`; characters added in
  a new game come out translated, while those in a save keep their saved
  names. As in Anchor, keys are matched in any case, a value is the field
  up to the next comma, and a field replaces only what the character's
  own `Chara*.csv` gives (a TRCHARA CALLNAME for a character whose CSV
  has none is ignored). Problems (an unknown key, data before `NO`, a
  missing `ENDCHR`…) are shown as warnings when the game loads, with the
  file and line.
- `TR_NAME(ABL, 25)` / `TR_NAME("TALENT", "処女")`: a translated name for
  scripts, the original when there is none. As in Anchor, the table is a
  bare variable or its name in capitals (`ABL`, `"ABLNAME"`…), and the key
  an index that has a name or a name in that CSV; anything else is a
  script error ("Invalid first argument" / "Invalid second argument").

### Managing saves

Ctrl-O → *Manage saves…* lists the game's saves with their date and the
comment Emuera's save menu shows. Pick one to **copy** or **move** it to
another slot, **export** it to a file (by default
`~/emuera-saves/<game>-saveNN-<date>.sav`) or **delete** it. For the whole
folder: **back up all saves** (into
`~/.local/share/emuera-tty/backups/<game>/<date>/`), **restore a backup**
(the current saves are backed up first) and **import a save file** into a
slot. Nothing is lost by mistake: a deleted or overwritten save is copied
to the backups first. Load a restored or imported save from the game's
own load menu. From a shell: `emuera-tty GAME --saves` lists the saves
and `emuera-tty GAME --backup-saves` backs them up.

**Save history.** Saves the game writes (save slots, autosaves,
`global.sav`, SAVECHARA files) are protected too, with no setting to turn
on:

- **The version replaced is kept**, in the same backup folder, as
  `<date>-auto-save01/`, so *restore a backup* can bring back any of them.
  Copies are thinned: the last 10 of each file, then one a day for 30 days,
  then one a week for 26 weeks. Saves are small (AnonTW's are under 1 MB),
  so this stays at a few MB per game.
- **A save emuera-tty didn't write** (one from Emuera.exe, or from before
  this history) is kept as `<date>-original-save01/` the first time it is
  replaced, and never thinned.
- **Each save is written safely**: to a temporary file, flushed to disk,
  read back and checked to load, and only then put in place. A crash,
  power cut or full disk leaves the previous save; a save that wouldn't
  load is reported at once and the previous one kept.
- **`save-log.tsv`** in the backup folder records each save written: when,
  which file, a fingerprint, its size, and the emuera-tty version and
  commit that wrote it. If a bug turns up in some build, the log tells
  which saves it wrote and which backup came before.

`EMUERA_SAVE_HISTORY=0` turns the history and the log off (saves are still
written safely). Nothing is added inside the saves: Emuera reads them as
before.

**Readable colours:** games are drawn for Emuera's black (or game-set)
background, and some colours there are hard to read, like pure blue, or
AnonTW's blue text on the dark navy background of time stop. emuera-tty
lightens coloured text until it reaches a minimum contrast (WCAG 3:1, or
4.5:1 on *high*), keeping its hue; greys are left alone, since games use
them for shading. The window background set by `SETBGCOLOR` covers the
whole screen, as in Emuera.

## 5. Picking a terminal

Text works everywhere. Images and Japanese/Chinese text depend on the
terminal:

| Terminal | Images | Notes |
|---|---|---|
| **kitty**, **Ghostty**, Konsole | yes (kitty graphics) | best choice |
| **iTerm2** (macOS), **WezTerm** | yes (iTerm2 inline images) | WezTerm's kitty graphics flicker, so auto uses iTerm images there |
| **foot**, xterm (`-ti vt340`), mlterm, yaft, Windows Terminal 1.22+ | yes (sixel) | |
| macOS Terminal.app, GNOME Terminal, Alacritty, others | `░` placeholders | fine for playing |
| Linux console under **fbterm** | yes (framebuffer) | needs the `video` group |
| bare Linux console (no fbterm) | yes (framebuffer) | can't show Japanese/Chinese text |
| real VT100 / VT220 / VT52 / ADM-3A / Wyse…, local or on a **serial line** | no | ASCII, see below |
| **tmux** inside kitty or Ghostty | yes (kitty Unicode placeholders) | needs `set -g allow-passthrough all` |
| **tmux** inside a sixel terminal | yes (tmux draws the sixel) | tmux built with sixel |
| Termux (Android) | no | `iterm`/`sixel` floods the terminal with data, `kitty` renders nothing, Termux in general doesn't support in-terminal image rendering |
| Android to other device (ADB) | depends on terminal used, conditions above | `fb` not supported for framebuffer consoles, has an issue that floods the input bar when moving the mouse around |
| Android to other device (SSH) | depends on terminal used, conditions above | `fb` not supported for framebuffer consoles |
| Android recovery shell (builtins in TWRP, OrangeFox, etc...) | no | images never supported, JP/CN text untested, very finicky in general (and why are you even running it there of all places?) |

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
- `--gfx none` turns images off; `--gfx kitty|iterm|sixel|fb` forces a backend
  if auto-detection guesses wrong.
- **tmux**: with kitty or Ghostty outside it, put `set -g allow-passthrough
  all` in `~/.tmux.conf` (then `tmux source ~/.tmux.conf`). (`on` works
  too, but tmux then drops a picture sent while the pane isn't showing,
  say during a loading screen while you are in another window, and it
  stays blank until the game draws a new one.) Pictures are
  then written as kitty's Unicode placeholders: text tmux keeps in its
  pane, so they follow splits, window switches and redraws. A picture
  meant to sit behind text isn't shown there, and crops are to whole
  cells. Without the setting there are no pictures (the startup message
  says so). byobu runs on tmux by default, so the same applies; GNU
  screen can't pass pictures through, so there are none there.

**On the Linux console** (Ctrl-Alt-F3 etc.):

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
  keeps menus and maps lined up (`EMUERA_AMBIGUOUS=single` forces this).
- Its font has a few hundred glyphs, and the rest show as a diamond.
  emuera-tty reads which characters the font has and draws the missing
  ones as the nearest it has: `【】「」` as `[ ]`, full-width `（Ａ１）` as
  `(A1)`, `ー` as `─`, `★※` as `*`, and so on, in the same width. Kana and
  kanji have no stand-in; use fbterm for those.
- The console has 16 colours. Dim colours keep their hue (dark teal
  walls stay cyan, brown floors brown) and greys step through black, dark
  grey, light grey and white, so coloured maps stay readable.

**On a retro or dumb terminal** (real hardware, or an emulator like
cool-retro-term or `xterm -ti vt52`):

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

**On a serial line**, from the computer the terminal is wired to:

```sh
emuera-tty /path/to/game --serial /dev/ttyUSB0 --term vt220 --baud 9600
```

`--serial-format 7E1` (default `8N1`) and `--flow rts` or `none`
(default XON/XOFF) match the terminal's setup; `--size 132x24` if it isn't
the terminfo size. The serial port needs the `dialout` group
(`sudo usermod -aG dialout $USER`). Messages and warnings stay on the
computer's own screen; the game quits when the line hangs up.

## 6. Updating and removing

```sh
emuera-tty --upgrade      # download the latest version, rebuild, reinstall
emuera-tty --reset-data   # forget settings and the game list (saves are kept)
emuera-tty --uninstall    # remove the installed program
emuera-tty --manual       # the manual (man emuera-tty works once installed)
```

`--install` also puts the manual page (`doc/emuera-tty.1`) in the
`share/man/man1` beside the program's folder (`~/.local/share/man/man1`),
where `man` finds it.

`--upgrade` keeps its own copy of the source in
`~/.local/share/emuera-tty/src` and needs `git` and `cargo`. Settings live
in `~/.config/emuera-tty/`; delete that folder (or use `--reset-data`) to
start fresh. Games extracted from archives (and their saves) are in
`~/.local/share/emuera-tty/games/`; neither `--reset-data` nor
`--uninstall` touches them.

## 7. When something goes wrong

This is a proof of concept, so it will happen.

- **A red `*** script error ***` screen** means the game hit something
  emuera-tty doesn't support yet (or a bug of the game's). As in Emuera
  it shows the file and line, the line itself, the function, and the
  call stack that led there. Press Enter to quit; your last save is safe.
- **`warning: … not implemented (ignored)`** lines in the terminal after
  quitting are commands emuera-tty skipped; the game usually carries on.
- **Check a game without playing it:** `emuera-tty /path/to/game --check`
  lists every line that fails to parse, and, as Emuera does, every
  statement a `#FUNCTION(S)` may not use (input, CALL/JUMP, BEGIN, QUIT,
  RETURN, PRINTW…: it is an error if it runs).
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
| `--gfx auto\|kitty\|iterm\|sixel\|fb\|none` | image backend (also `EMUERA_GFX`) |
| `--check` | parse the game, list errors, exit |
| `--headless` | plain stdin/stdout mode (scripts, tests); `--echo` shows typed input |
| `--strict` | unimplemented commands are errors instead of warnings |
| `--debug` | enable `[IF_DEBUG]` blocks and `;#;` lines |
| `--retro[=line]` | terminfo-driven ASCII frontend for retro terminals (auto for non-ANSI `$TERM`) |
| `--term NAME` | terminfo name of the terminal (default `$TERM`, `vt100` with `--serial`) |
| `--serial DEV` | play on a serial terminal; `--baud N`, `--serial-format 8N1`, `--flow xon\|rts\|none` |
| `--size COLSxROWS` | screen size when the terminal can't report it |
| `--saves` / `--backup-saves` | list the game's saves / back them all up |
| `--install` / `--upgrade` / `--uninstall` / `--reset-data` | see the guide |
| `--manual` | the full manual page |

Environment: `FRAMEBUFFER=/dev/fbN` picks another framebuffer,
`EMUERA_MOUSE=mice|off|term` overrides the console-mouse choice,
`EMUERA_AMBIGUOUS=wide|narrow|osc66` overrides how box-drawing and symbol
characters are fitted to Emuera's two columns (detected at startup:
terminals that draw them wide need nothing, others are padded; `osc66`
tries kitty's text-sizing protocol instead, which some may prefer).
At startup emuera-tty also measures how wide the terminal really draws
those characters (terminals' width tables differ), which keeps maps
aligned in Konsole, kitty and others; `EMUERA_MEASURE=0` skips that,
`EMUERA_TRACE` as above. Game scripts are read on every CPU core (AnonTW
loads in 0.3 s); `EMUERA_THREADS=N` caps the thread count, and
`EMUERA_TIMING=1` prints how long each loading phase took and the memory
in use after it. Running the script itself is one
thread by nature (every line may read what the previous one wrote), but
pictures a screen needs are decoded and scaled on all cores. The
interpreter resolves each variable once (a reference remembers its global
or per-function slot), jumps straight to the matching CASE of a
SELECTCASE made of constants, and avoids copying strings when comparing,
calling functions or querying DataTables: on AnonTW's shop screens it runs
about three times faster than the first versions. `EMUERA_SEED=N` fixes the random
number seed, so a run can be repeated exactly (tests, bug reports).

### Memory

A big game's scripts are tens of thousands of functions (AnonTW 90000,
pops-tw 100000), and a session runs a few percent of them. So at load
emuera-tty reads only each function's `@` line and attributes (`#FUNCTION`,
`#PRI`…) and where it is in its file; the statements are compiled the
first time the function is called, read again from the file (nothing is
kept in memory meanwhile). A parse error in a function body is reported
when the function first runs; `--check` compiles everything and lists
them all (`EMUERA_PARSE=all` compiles everything at load for a normal
run too). A script file edited while the game runs is read as it is
then, for functions not run yet; the reload key (Ctrl-E) reloads everything.

Two things are kept within a share of the machine's memory, so a 512 MB
board copes and a PC with memory to spare does exactly what it did:

- **Compiled functions** beyond 1/16 of the memory (`EMUERA_CODE_MB=n`
  sets it): the bodies called longest ago are dropped and compiled again
  if needed. pops-tw runs a 170000-line function once at the title.
- **Graphics buffers** (`GCREATE`) beyond 1/8 (`EMUERA_GBUF_MB=n`): the
  least recently used are kept packed (run-length coded, exactly) until
  drawn or shown again. pops-tw keeps three portraits per character in
  buffers, 270 MB unpacked, 40 MB packed.

| | before | now (PC) | with a 512 MB machine's budgets |
|---|---|---|---|
| AnonTW, 800-input session | 654 MB | 157 MB | 157 MB |
| pops-tw, a new game | 1204 MB | 636 MB | 346 MB |

`EMUERA_MEMSTATS=<file>` writes what the memory holds (functions
compiled, the biggest variables, characters, picture buffers…) to
`<file>` at every input.

`EMUERA_LINT=1` with `--check` lists compiled statements that look like
the parser split their line in the wrong place (a function name holding
`(`, `,` or spaces; FORM text still holding `%`, `{` or `}`), for hunting
parser bugs across a whole game.

For development: `cargo test` runs the unit tests and the `sample/` game's
self-test; `cargo build --profile fastbuild --features profile` makes a
quick-to-build optimised binary whose `EMUERA_PPROF=<file>` writes a
sampling profile of the engine itself (time per Rust function and caller
→ callee pairs); `emuera-tty sample --headless --echo` plays the sample in plain
text.

### Mouse

| Where | How |
|---|---|
| xterm, kitty, WezTerm, foot, Ghostty, Alacritty, GNOME/KDE terminals, tmux | SGR (1006) mouse reports |
| urxvt and other 1015 terminals | urxvt (1015) reports |
| older terminals | legacy X10 reports |
| Linux console / fbterm with **gpm** running | connects to gpm (`/dev/gpmctl`) as a client (the console's own mouse reports stay off, or gpm would send them clicks only), draws its own pointer |
| Linux console / fbterm **without** gpm | reads `/dev/input/mice` directly (wheel included), draws its own pointer |

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

**No mouse?** *Keyboard pointer* in the settings (`auto` turns it on by itself on the
Linux console when no mouse can be found) puts a pointer on the screen
that the **cursor keys** move a cell at a time; **Insert** clicks under it,
and so does **Shift+Enter** where the terminal sends it apart from Enter
(Konsole, xterm; not the Linux console). Enter keeps its own job, and
PgUp / PgDn still scroll. Hovering shows tooltips and the button's number
as with a mouse, and picture menus (pops-tw's title) take the clicks too.
(A key held alone, such as right Shift, can't be used: terminals send
nothing for it.)

## Graphics backends

`--gfx auto` picks, in order:

| Environment | Backend | How |
|---|---|---|
| `TERM=linux` / `fbterm*` (VT) | **fb** | mmaps `/dev/fb0`, alpha-blends sprites onto the console after each frame. Needs read/write access to the device (`video` group). Cell size = fb resolution ÷ cols/rows. |
| `TERM_PROGRAM=iTerm.app` / `WezTerm`, `LC_TERMINAL=iTerm2` | **iterm** | PNG per sprite in `OSC 1337 File=`, sized in whole cells (padded, so never stretched), cached. |
| kitty graphics reply `_Gi=31;OK` | **kitty** | zlib-compressed RGBA (`o=z`) transmitted once per sprite+size, `a=p` placements (a replaced picture is placed before the old one is taken off), source-rect crop at the screen edge. |
| inside tmux with `allow-passthrough on`, and a kitty graphics reply or `GHOSTTY_RESOURCES_DIR` / `KITTY_WINDOW_ID` | **kitty-tmux** | the same through tmux's passthrough, placed with kitty's Unicode placeholders (U+10EEEE, the image id as the 256-colour foreground, row/column diacritics), which tmux handles as text. |
| DA1 reply contains `4` | **sixel** | median-cut 255-colour quantiser, transparent background, cached per sprite. |
| otherwise | none | images shown as `░` placeholders of the right size. |

Games that draw everything into a window-sized canvas every frame
(eraFlappyTengu, eraSort) are limited by how fast that canvas reaches the
terminal. Each frame is sent as one synchronized update (DEC mode 2026);
kitty pictures go compressed (a mostly empty canvas shrinks a
hundredfold), and scaling (`GDRAWSPRITE`, the canvas to the screen) uses
a fixed-point version of the Triangle filter on several cores. Off-screen
pictures stay in the terminal only up to 24 MB, and freed pictures'
ids are given to new ones: Konsole (25.04) never frees a picture's data
when told to, only when another picture comes with the same id, so
fresh ids for every frame would fill its memory within a minute. At a
1440×900 window, eraFlappyTengu runs at about 85 frames per second, using
about 4 ms of CPU per frame.

Images are sized in Emuera's coordinate space (font-size–relative, as in
`HTML_PRINT <img height=…>`) and scaled by `terminal cell height / Emuera
line height`, so layouts keep their proportions. `emuera.config` font size /
line height / colours are honoured.

Pictures that overlap, like a portrait's layers (body, face, blush, a
frame, the tint while time is stopped, each printed on its own line and
moved onto the first with `ypos`), are blended into one picture before
they are sent, back to front as Emuera draws them. Terminals stack
separate pictures by rules of their own (kitty by image id, Konsole by
which was placed last), so a re-sent layer could land on top of the
others; one picture can't. It also helps sixel and iTerm2, which can't
stack transparent pictures at all.

In WezTerm, Windows Terminal and other terminals where sixel and iTerm2
pictures belong to the cells under them, and inside tmux, where kitty
pictures are placeholder text, writing text there erases the picture:
after redrawing a hovered button over a picture, emuera-tty sends that
picture again. Inside tmux a tooltip simply covers the placeholders,
rather than being cut out of the picture (which would mean sending a new
picture through tmux).

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
  vm/builtins.rs every Emuera / Emuera-EM command and function
  vm/system.rs   TITLE / FIRST / SHOP / TRAIN / ABLUP / AFTERTRAIN / TURNEND flow
  vm/save.rs     save/load: what goes into Emuera's .sav / .dat files
  vm/erasav.rs   Emuera's binary save format (reader / writer)
  console.rs     Emuera console model: styled segments, buttons, auto `[N]` buttons
  html.rs        HTML_PRINT: font/b/i/u/s, p, br, button/nonbutton, clearbutton, img, shape, space, div
  tty.rs         raw-mode TUI, layout/wrapping/alignment, mouse, colour downgrade (24-bit/256/16)
  retro.rs       retro / dumb / serial terminals: terminfo-driven, ASCII, changed-cells-only redraw
  terminfo.rs    compiled terminfo reader, tparm, delay padding
  settings.rs    saved settings and the game library (~/.config/emuera-tty/config)
  picker.rs      launcher: game library, folder / archive picker, settings page
  sys.rs         platform layer: raw mode, size, input polling (termios / Windows console)
  gfx/{kitty,iterm,sixel,fb}.rs
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
  `{ }` line joining, `;!;` / `;^;`, `_Rename.csv`.
- Control flow: IF/ELSEIF/ELSE, SIF, SELECTCASE (`TO`, `IS`), FOR/NEXT,
  REPEAT/REND, WHILE/WEND, DO/LOOP, BREAK/CONTINUE, GOTO/TRYGOTO,
  CALL/JUMP/TRY*/TRYC*…CATCH, *FORM variants, TRY*LIST, RETURN/RETURNF,
  BEGIN, RESTART, THROW, QUIT.
- PRINT family (V/S/FORM/FORMS × K/D × L/W/C/LC, SINGLE, PLAIN), PRINTDATA,
  STRDATA, PRINTBUTTON, PRINT_IMG, HTML_PRINT, DRAWLINE, colours/styles,
  ALIGNMENT, REUSELASTLINE, CLEARLINE.
- INPUT/INPUTS/TINPUT/ONEINPUT/WAIT/TWAIT/INPUTMOUSEKEY (approx.); INPUTS'
  default answer is FORM text, as in Emuera.
- Sound: PLAYSOUND / PLAYBGM / STOPSOUND / STOPBGM / SETSOUNDVOLUME /
  SETBGMVOLUME / EXISTSOUND (see *Sound* above); GETMEMORYUSAGE.
- Characters: ADD/DEL/SWAP/COPY/SORT/PICKUP, CSV* lookups, FINDCHARA.
- Strings, math, arrays (VARSET, ARRAYSHIFT/REMOVE/SORT/COPY, SPLIT, …).
  `TOSTR` takes .NET format strings exactly as Emuera does: standard
  (`D2`, `X6`, `N0`, `F2`, `E3`, `P0`, `C`) and custom (`#,0`, `0.0`,
  `00;-00`, `0,.###`, `0%`, quoted text, `E+0`). `TOINT` / `ISNUMERIC`
  read `0x` hex, `0b` binary and exponents; FORM strings and literals
  know `\n \t \s \S`.
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
- `#LOCALSIZE` / `#LOCALSSIZE`; `LOCAL@FUNC` (and `LOCALS@`, `ARG@`,
  `ARGS@`) for another function's own.
- TRAIN as in Emuera: commands numbered by their position in TRAIN.csv
  (other numbers go to `@USERCOM`), NEXTCOM, the WAIT after a command,
  and the per-step resets (UP / DOWN / LOSEBASE, SOURCE, NOWEX, STAIN to
  `_Replace.csv`'s defaults). PRINT_SHOPITEM, MONEYSTR and MONEYLABEL use
  `_Replace.csv`'s currency unit and its position. `CALLTRAIN` /
  `STOPCALLTRAIN` / `@CALLTRAINEND` (continuous training; pops-tw forces
  commands with it). `DOTRAIN` as in Emuera: the event it is in (only
  @EVENTTRAIN, @SHOW_STATUS, @SHOW_USERCOM, @EVENTCOMEND, @USERCOM) runs to
  its end, then the command runs.
- Graphics buffers and sprites (`GCREATE`, `GDRAW*`, `GSETFONT`/text,
  `SPRITECREATE`, `SPRITE*`), shown through the image backends.
- Client background layers (`CBGSETG`, `CBGSETSPRITE`, `CBGSETBUTTONSPRITE`,
  button maps) and the mouse in pixels (`MOUSEX` / `MOUSEY`,
  `INPUTMOUSEKEY` clicks, wheel, keys, timeouts), so menus drawn as
  pictures work, like pops-tw's title. Terminals without images get the
  text that was drawn into the picture, at its place on the screen.
- `REPLACE` / `REGEXPMATCH` / `FINDELEMENT` use .NET-style regular
  expressions as in Emuera (groups, `$1`, lookaround).
- EMEE `.als` name aliases.
- `DAY.csv` / `TIME.csv` / `MONEY.csv` names (`DAYNAME`, `TIMENAME`).
- Config files with Emuera's Japanese or English keys (`ウィンドウ幅` or
  `WINDOW WIDTH`), including PRINTC items per line and width, the font,
  and button wrapping (a button that doesn't fit moves to the next line
  whole, as in Emuera).
- SAVEGAME/LOADGAME menus, SAVEDATA/LOADDATA, SAVEGLOBAL/LOADGLOBAL, @SAVEINFO.
- Emuera variable lifetimes: private `#DIM` static unless `DYNAMIC`,
  per-function `LOCAL`/`ARG`, `#DIM REF` parameters.
- Emuera-Anchor: the `*_TR.csv` / `ALL_TR.csv` / `TRCHARA` translation
  files and `TR_NAME`, `PARAM.CSV` (read in place of `PALAM.CSV`, with
  `PARAM` usable for `PALAM` in scripts), the Ctrl-T / Ctrl-R / Ctrl-E
  hotkeys, the clipboard.
- Keyboard macros: the `\n` / `\e` / `(…)*n` input syntax, F1…F12 in ten
  groups, the game's `macro.txt` read and written as Emuera does.
- Emuera-EM/EE: `MAP_*` string maps, `DT_*` data tables (incl. Select
  filters and XML), `XML_*` documents (XPath subset: steps, `//`, `*`,
  `text()`, `@attr`, predicates), `VARSETEX`, `LOADTEXT`/`SAVETEXT`,
  `HTML_TAGSPLIT`, `HTML_POPPRINTINGSTR`, and `AWAIT` with the window
  still live: typing goes into the input line (`GETTEXTBOX`), the button
  under the pointer is `MOUSEB()`, and clicks and keys reach
  `GETKEY`/`GETKEYTRIGGERED` (AnonTW's dish search by name).

Debugging: `EMUERA_TRACE=<part of a function name>` logs matching calls and
their return values to stderr, with timestamps. `EMUERA_PROFILE=<file>`
samples which ERB function is running every millisecond (time spent waiting
for the player not counted) and keeps `<file>` updated with the top 60,
under a first line telling which input the script is waiting for and the
call stack that asked for it (handy when a game seems stuck).
`EMUERA_INJECT="STATEMENT||STATEMENT"` with `EMUERA_INJECT_AT=<n>` runs those
statements as a function `@__INJECT` just before the n-th input.
`EMUERA_LIST_BUTTONS=1` makes `--headless` print the values of the buttons
on screen before each input (`[buttons: 1001, 1002]`), hidden ones included,
for scripted play.

## Tests

`cargo test --release` runs the unit tests, the sample game's self-test and
the **ERB test suite**: small Emuera games that check the engine against
what Emuera does.

- `tests/erb/*.erb`: one file per topic (strings and regular expressions,
  what RESULT holds, indices by CSV name, control flow, characters, saves,
  graphics, XML and data tables, input, the system flow…). Each is a
  `@TEST_MAIN` of `CALL CHECK, "what", condition` lines; `;@input` lines are
  typed in. A bug fixed gets its check here.
- `tests/golden/*.erb` print text that must match the `.out` beside them
  exactly (`UPDATE_GOLDEN=1 cargo test --release --test erb_suite` rewrites
  them after a deliberate change).
- `tests/save/`: a fixed game state whose `save00.sav` / `global.sav` must
  match byte for byte (Emuera's binary format).

The same files run on the **real Emuera**, which is what makes the suite an
independent check: `tools/erb_suite_export.sh DIR` builds them as ordinary
game folders; with Emuera.exe run in each (each writes `emuera.log`),
`tools/erb_suite_compare.py DIR` reports any check that fails there, golden
output that differs and saves that aren't byte-identical.
`tools/erb_coverage.py` lists which of Emuera's 558 built-in functions and
statements no test uses yet (about 90% are used).

`tools/game_smoke.sh GAME_DIR [STEPS] [SEED]` plays a real game with random
menu choices in a throwaway copy and reports script errors.


## Roadmap

1. Run it on the other systems (macOS, Windows, the BSDs, Termux, Haiku,
   Redox) and on a real Raspberry Pi Zero: they compile (the Pi build
   also runs under emulation), nobody has played on them yet.
2. A good way to play on Android phones, through Termux: players keep
   asking for an up-to-date eraBASIC player there, and saves would move
   between phone and PC. Nothing is known to be missing, but nobody has
   tried it on a phone. What needs checking: a phone's narrow screen
   (games are laid out for a wide window), taps arriving as clicks for
   buttons and picture menus, function keys and Ctrl keys from Termux's extra-keys
   row, Japanese text widths in Termux's font, whether its terminal draws
   sixel pictures, and games kept in shared storage (`/sdcard`). A
   separate Android app would come later, if at all: the engine never
   talks to the terminal directly, so another frontend could be added.
3. Play deeper into AnonTW, pops-tw and eraTohoK, and more games.
   AnonTW's 5 remaining parse errors and pops-tw's 16 are mistakes in the
   games' own scripts (Emuera reports them too); eraTohoK has none.
4. Dialog translation and sound with real translators and players on
   other systems: tested here with stand-ins and mpv only.
5. Speed of script-heavy games: pops-tw's new game takes about 14
   seconds of CPU (its own object system looks names up in 1000-entry
   arrays hundreds of thousands of times, through a million statements);
   compiling variables to slots ahead of time would be the next step.
