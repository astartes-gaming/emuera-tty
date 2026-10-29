## What you need

- **Linux**, the main tested system. It also runs on **Windows 10/11**,
  **Android** (Termux), **macOS**, the **BSDs**, **illumos / Solaris**,
  **Haiku**, **GNU/Hurd** and **Redox**, with different amounts of testing
  (see [Other systems](Other-Systems-and-CPUs#other-systems)), on most CPUs
  ([Other CPUs](Other-Systems-and-CPUs#other-cpus)) and on 512 MB boards like
  the Raspberry Pi Zero ([Small machines](Other-Systems-and-CPUs#small-machines-raspberry-pi-zero-and-the-like)).
  Reports from any of them are welcome.
- A **terminal**. Any modern one works for text; see
  [Picking a terminal](Terminals) for pictures and Japanese text.
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
  another language (see [Sound](Playing#sound) and [Translation](Translation)).

## Build and install

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

## Installing on your distribution

Building needs **git**, a **C linker** (gcc) and **Rust 1.88 or newer**
(`rustc --version` shows yours). Where the distribution's own Rust is
older, get Rust from rustup instead. After the packages, run the four
commands under [Build and install](#build-and-install).

| Distribution | Packages |
|---|---|
| **Debian** 13+, **Ubuntu** 24.04+, Linux Mint 22+, Pop!_OS, Raspberry Pi OS | `sudo apt install git gcc rustup`, then `rustup default stable` (their packaged `rustc` is too old) |
| Older Debian / Ubuntu | `sudo apt install git gcc curl`, then the rustup line under [What you need](#what-you-need) |
| **Fedora** | `sudo dnf install git gcc rust cargo` |
| **Arch**, Manjaro, EndeavourOS | `sudo pacman -S --needed git gcc rust` |
| **openSUSE** Tumbleweed | `sudo zypper install git gcc rust cargo` |
| openSUSE Leap | `sudo zypper install git gcc rustup`, then `rustup default stable` |
| **Alpine** 3.23+ | `doas apk add git build-base rust cargo` |
| **Void** | `sudo xbps-install -S git gcc rust cargo` |
| **Gentoo** | `sudo emerge --ask dev-vcs/git dev-lang/rust-bin` |
| **NixOS** | `nix-shell -p git gcc rustc cargo`, then build inside that shell |
| Fedora Silverblue, Bazzite, other read-only systems | build in a Distrobox, as for SteamOS below |

The Debian, Ubuntu, Fedora, Arch, openSUSE, Alpine and Void lines are
tested by building in their containers (October 2026); the Gentoo and
NixOS ones aren't yet.

### SteamOS (Steam Deck)

SteamOS has no compiler and its system folders are read-only, so build in
a **Distrobox** container (SteamOS 3.5 and later have `distrobox`; the
container shares your home folder). In **Desktop Mode** (Steam button →
Power → Switch to Desktop), open Konsole:

```sh
distrobox create -n build -i archlinux:latest
distrobox enter build
sudo pacman -S --needed git gcc rust
git clone https://github.com/astartes-gaming/emuera-tty.git
cd emuera-tty && cargo build --release
./target/release/emuera-tty --install
exit
```

The program lands in `~/.local/bin`, outside the container, and runs on
SteamOS itself: `emuera-tty` in Konsole. `emuera-tty --upgrade` needs the
container too (`distrobox enter build -- emuera-tty --upgrade`).
Alternatively, copy a **static build** made on another Linux PC (the
recipe under [Other CPUs](Other-Systems-and-CPUs#other-cpus) with `x86_64-unknown-linux-musl`)
into `~/.local/bin` and `chmod +x` it. To play from Game Mode with the
controller, see [Gamepads and the Steam Deck](Gamepads-and-Steam-Deck).

## Updating and removing

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
