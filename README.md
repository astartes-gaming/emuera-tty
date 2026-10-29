# emuera-tty

> - Written with an AI (Claude) and tested by playing games, not audited.
>   Some scripts will still mis-render or misbehave. emuera-tty keeps a history
>   of every save it overwrites (see [Managing saves](https://github.com/astartes-gaming/emuera-tty/wiki/Playing#managing-saves)), but keep your own copy
>   of saves you care about.
> - It is not affiliated with Emuera, Emuera-EM/EE, eratohoTW or AnonTW.
>   Where behaviour was copied, it was by reading
>   [EvilMask's emuera.em](https://gitlab.com/EvilMask/emuera.em) source for
>   reference; no code from it is included.
> - No game files are included. You need your own copy of the game.
> - MIT licensed (see [LICENSE](LICENSE)).

A from-scratch reimplementation of the **Emuera** ERB engine in Rust that
plays era games in a terminal: graphics-capable terminals (kitty, Konsole,
WezTerm, Ghostty, foot, iTerm2, xterm with sixel…), plain ones (pictures are
left out, text and colours stay), the Linux framebuffer console, and even old
serial terminals.

Tested mostly with **AnonTW** (eratohoTW in English), **pops-tw** (eraNAS)
and **eraTohoK**: new games, days, training, shops, maps and saves work, and automated play runs
thousands of random choices through them without errors. What it has
(details under [Implemented](https://github.com/astartes-gaming/emuera-tty/wiki/Implemented)):

- Emuera / Emuera-EM / EE scripts, HTML_PRINT with `<div>` boxes, pictures,
  sprites and graphics buffers, tooltips, the mouse (hover, click, right-click
  skip), and the games' own settings window.
- Saves in Emuera's own format, in both directions.
- Sound through mpv / ffplay / other players, and live **translation** of
  Japanese or English games into other languages (the dialog or all text)
  with a translator on your computer (or DeepL / translate-shell if you
  choose).
- A game library with archives and `git pull` updates, a **game
  downloader** (the games of the eraGames wiki), a settings page, save
  management, and readable script errors with the call stack.
- **Gamepads** on Linux, the **Steam Deck** included (see
  [Gamepads and the Steam Deck](https://github.com/astartes-gaming/emuera-tty/wiki/Gamepads-and-Steam-Deck)).
- Linux, and builds for macOS, Windows, the BSDs, Termux and more, down to a
  Raspberry Pi Zero.

Expect rough edges in parts of the games nobody has played through yet
(see the [Roadmap](https://github.com/astartes-gaming/emuera-tty/wiki/Roadmap)).

**Documentation: [the wiki](https://github.com/astartes-gaming/emuera-tty/wiki/)** (its source is in
[`doc/wiki/`](doc/wiki), and `emuera-tty --manual` prints the manual page).

## Install

You need **git**, **gcc** and **Rust 1.88 or newer**
([rustup](https://rustup.rs) if your distribution's Rust is older; see
[Installation](https://github.com/astartes-gaming/emuera-tty/wiki/Installation#installing-on-your-distribution) for
the package line of each distribution and for **SteamOS / Steam Deck**).
Then:

```sh
git clone https://github.com/astartes-gaming/emuera-tty.git
cd emuera-tty
cargo build --release
./target/release/emuera-tty --install
```

`--install` puts `emuera-tty` in a folder on your `PATH` (usually
`~/.local/bin`), no root needed. `emuera-tty --upgrade` later fetches,
rebuilds and reinstalls the latest version. Windows, Android (Termux),
macOS, the BSDs and other systems: see
[Other systems and CPUs](https://github.com/astartes-gaming/emuera-tty/wiki/Other-Systems-and-CPUs).

## Play

```sh
emuera-tty                 # the launcher (a folder picker the first time)
emuera-tty ~/Games/eraTW   # a game folder (the one with CSV/ and ERB/), or its archive
```

Type a choice's number and Enter, or click it; F3 opens the settings.
Saves are in Emuera's format and where Emuera puts them, so you can
switch between Emuera.exe and emuera-tty. More in
[Choosing a game](https://github.com/astartes-gaming/emuera-tty/wiki/Choosing-a-Game) and [Playing](https://github.com/astartes-gaming/emuera-tty/wiki/Playing);
for pictures and Japanese text, [pick a terminal](https://github.com/astartes-gaming/emuera-tty/wiki/Terminals).

## Documentation

| Guide | Reference |
|---|---|
| [Installation](https://github.com/astartes-gaming/emuera-tty/wiki/Installation) | [Command line and environment](https://github.com/astartes-gaming/emuera-tty/wiki/Command-Line-and-Environment) |
| [Other systems and CPUs](https://github.com/astartes-gaming/emuera-tty/wiki/Other-Systems-and-CPUs) | [Memory](https://github.com/astartes-gaming/emuera-tty/wiki/Memory) |
| [Choosing a game](https://github.com/astartes-gaming/emuera-tty/wiki/Choosing-a-Game) | [Pictures and graphics](https://github.com/astartes-gaming/emuera-tty/wiki/Pictures-and-Graphics) |
| [Playing](https://github.com/astartes-gaming/emuera-tty/wiki/Playing) | [Compatibility](https://github.com/astartes-gaming/emuera-tty/wiki/Compatibility) |
| [Settings](https://github.com/astartes-gaming/emuera-tty/wiki/Settings) | [Implemented](https://github.com/astartes-gaming/emuera-tty/wiki/Implemented) |
| [Translation](https://github.com/astartes-gaming/emuera-tty/wiki/Translation) | [Debugging](https://github.com/astartes-gaming/emuera-tty/wiki/Debugging) |
| [Gamepads and the Steam Deck](https://github.com/astartes-gaming/emuera-tty/wiki/Gamepads-and-Steam-Deck) | [Development](https://github.com/astartes-gaming/emuera-tty/wiki/Development) |
| [Terminals](https://github.com/astartes-gaming/emuera-tty/wiki/Terminals) | [Roadmap](https://github.com/astartes-gaming/emuera-tty/wiki/Roadmap) |
| [Troubleshooting](https://github.com/astartes-gaming/emuera-tty/wiki/Troubleshooting) | |

## Development

```sh
cargo test --release     # unit tests, the sample game's self-test, the ERB suite
cargo clippy --release
cargo fmt
```

Behaviour follows Emuera's source; deliberate differences are listed in
[`doc/differences.md`](doc/differences.md) (Emuera-EM/EE) and
[`doc/anchor.md`](doc/anchor.md) (Emuera-Anchor). See
[Development](https://github.com/astartes-gaming/emuera-tty/wiki/Development) for the test suites. Documentation
changes go in `doc/wiki/`; `tools/wiki_sync.sh` publishes them to the wiki.
