# Design

## Context

The motivation is in proposal.md (Why); the requirements are in `specs/game-replay` and `specs/terminal-matrix-ci`. This document covers how to build it.

What exists today:
- `tools/gamed.py` drives a game in a pseudo-terminal from a FIFO. It reads the screen with pyte and judges "idle" by CPU use.
- `tools/game_smoke.sh` plays randomly, headless.
- `.github/workflows/build.yml` runs `cargo test` on every push.

All three bugs that motivate this change were reproduced by hand the same way: Xvfb with software OpenGL (Mesa llvmpipe), the real terminal, xdotool and ImageMagick's `import`. That setup works on a hosted Ubuntu runner.

Constraints that shape the approach:
- **Games:** the game repositories aren't ours, and pops-tw takes its pictures from Git LFS. AnonTW and pops-tw can be cloned anonymously; eraTohoK can't.
- **Runner sizes:** hosted runners for public repositories have 4 cores and 16 GB of memory, and a job may run up to 6 hours.
- **Windows:** hosted Windows runners have no interactive desktop.
- **Determinism:** replays must not depend on the terminal's own input handling (keyboard layouts, IMEs, focus), or they won't be deterministic.

## Goals / Non-Goals

**Goals:**
- One script format and one pass/fail rule set, used both by the plain pty runner and by the real-terminal matrix.
- Results a maintainer can act on from the run page alone: the failing step, screen, logs, screenshot, and the terminal's crash signal.
- Easy to add a game or a terminal: one pin entry or one matrix entry.

**Non-Goals:**
- Pixel-exact screenshot comparison. Fonts and anti-aliasing differ between terminal versions; we only check "not blank".
- macOS, Konsole, WezTerm, foot and Windows Terminal in this change. The design leaves room for them.
- Testing the terminals' own input handling: xdotool typing, IMEs.

## Decisions

### Runner: Python next to `gamed.py`
`tools/replay.py` reuses gamed.py's approach: a pty, pyte for the screen, CPU-based idle detection, and the same `type` / `key` / `click LABEL` vocabulary. It adds `click ROW COL`, `expect TEXT [SECS]`, `wait idle|SECS`, `shot NAME [picture ROW COL ROWS COLS]` and `skip-on windows|terminal`. The common pieces move into `tools/ptydrive.py`, and gamed.py imports them.

*Alternative:* a Rust integration test (`tests/replay.rs`). Rejected because the tests use only std, so there's no pty or screen emulator available, and the tools are already in Python.

### Inside a real terminal: a pass-through shim, not xdotool input
The terminal (or tmux) runs `replay.py --shim`. The shim starts emuera-tty in its own pty and does four things:
1. copies everything emuera-tty writes to the terminal, which draws it;
2. feeds the same bytes to pyte and to the output checker;
3. passes the terminal's replies (kitty graphics query, DA, OSC 11, mouse) back to emuera-tty;
4. injects the script's steps as input, with mouse steps sent as SGR mouse sequences.

So input is deterministic, the screen and output checks work exactly as in the plain runner, and the terminal still does the real rendering. That rendering is what crashed and flickered. Inside tmux, the shim sees emuera-tty's raw output before tmux rewrites it, so the output checks still apply.

*Alternative:* typing with xdotool into the terminal window. Rejected: it depends on window focus, keyboard layout and timing, and gives us no copy of the output to check.

### Output checks run on the raw output
An APC/DCS parser on emuera-tty's raw output does the checks:
- It joins kitty chunks (`m=1` … `m=0`), unwrapping tmux passthrough first.
- It decodes base64, then inflates `o=z` data.
- It compares the length with `s × v × 4` for `f=32` (or `× 3` for `f=24`).
- It checks that sixel and iTerm2 sequences are terminated.

This catches malformed output from emuera-tty. It would not have caught the Ghostty crash, whose data was valid. For that, the terminal-death check is the safety net.

### Terminal death and backtraces
The matrix job starts the terminal itself and waits on its process. If the process exits before the shim reports the end of the script, or dies by a signal, the result fails. Two sources supply the backtrace:
- `systemd-coredump`, installed on the runner and read with `coredumpctl info`;
- the terminal's own log, kept from its stderr.

### Picture checks: flat-area test on a screenshot
`shot NAME picture ROW COL ROWS COLS` captures the X screen with `import`, crops the cell rectangle using the pinned cell size, and fails when the crop's standard deviation is 0 (a single colour).

The cell size comes from each terminal's pinned configuration: font, size and window geometry in `tests/replay/terminals/`. The shim also logs the cell size emuera-tty reports, to cross-check.

### Pins: TOML read with `tomllib`
`tests/replay/games.toml` holds one table per game: `url`, `commit`, `lfs` (paths to pull, e.g. `resources/`), `secret` (environment variable name, optional), and `copy` / `link` rules for the throwaway copy. Python 3.11's `tomllib` reads it with no new dependency.

*Alternatives:* JSON (no comments) or a shell file (harder to share with Windows). Rejected.

### Workflow shape (`.github/workflows/replay.yml`)
- **Triggers:**
  - `schedule` (nightly, 03:00 UTC);
  - `push: tags: ['v*']`;
  - `workflow_dispatch` with optional `games` and `terminals` inputs.
- **`build` job:** `cargo build --release` on Ubuntu and Windows; the binaries are uploaded as artifacts.
- **`games` job:**
  - restores each game from `actions/cache`, keyed by its commit (about 4 GB in all);
  - on a cache miss, does a blob-less clone, checks out the pin, and runs `git lfs pull --include` for the paths the pin names;
  - eraTohoK uses the `GITGUD_ERATOHOK_TOKEN` secret through a `GIT_ASKPASS` helper, so the token never appears in a command line or URL;
  - without the secret, eraTohoK is marked skipped.
- **Linux `replay` matrix:** game × {ghostty, ghostty-tmux, kitty, kitty-tmux}, `fail-fast: false`, one game per job, so each VM runs one game process. Each job:
  1. installs Xvfb, Mesa, xdotool, ImageMagick, tmux, Japanese fonts and `systemd-coredump`;
  2. gets the terminal from its cache;
  3. starts Xvfb and the terminal running the shim (or tmux with a pinned config and the shim);
  4. uploads the results with 14-day retention.
- **Windows `replay` matrix:** game × {console}, with `replay.py` in ConPTY mode through `pywinpty`.
- **`summary` job:** a table of game × terminal results (pass, fail or skip) with terminal versions, written to the run summary. It fails if any result failed.

### Terminals: pinned builds, cached
- **kitty:** the official binary release tarball at a pinned version, unpacked into the cache.
- **Ghostty:** built from a pinned release tag with the Zig version that release names, `-Doptimize=ReleaseFast`, cached by tag and Zig version. The first build takes about 10 minutes; after that it comes from the cache.
- **tmux:** Ubuntu's package, with a pinned `tmux.conf`: `allow-passthrough all`, status bar on, the `RGB` and underline-style features.

## Risks / Trade-offs

- **[Ghostty needs newer GTK/libadwaita than Ubuntu ships]** → Use the newest Ubuntu runner image. If that's still too old, run the Ghostty jobs in a Fedora container, where Ghostty is packaged, and pin the container image.
- **[Scheduled workflows stop after 60 days without repository activity]** → The release-tag and on-demand triggers still work; the summary notes the last nightly date.
- **[Games change their screens, breaking `expect` lines]** → Games are pinned; updating a pin includes re-running the replays locally and fixing scripts in the same change.
- **[Software OpenGL renders differently from real GPUs]** → It catches crashes and blank pictures, not GPU-driver bugs. Driver issues stay a manual check, as noted in CLAUDE.md.
- **[A flaky timing-dependent step]** → Steps wait for idle, not fixed times. A step that fails is retried once and reported as flaky if the retry passes; it fails the run only if both attempts fail.
- **[Licensing of the games' content]** → Nothing from the games is committed or published except screenshots of a few screens in short-lived results. They're kept for 14 days and are visible to anyone who can see the run (the repository is public).
- **[The token leaking]** → It's a read-only deploy token scoped to one repository; it goes only through `GIT_ASKPASS`, is masked by Actions, and isn't available to pull requests from forks.

## Migration Plan

This is new tooling only; nothing existing changes. The rollout:
1. Land the runner and the scripts. Developers can use them locally right away.
2. The upstream maintainer adds the secret and enables the workflow.
3. Watch a week of nightly runs before making release tags depend on the result.

Rollback: disable the workflow. The local runner is unaffected.

## Open Questions

- How long each game's replay should run. Start with about 10 minutes per game (title, new game, a few in-game days, save and load) and lengthen once the runs are stable.
- Whether to keep screenshots from passing runs, or only from failures. Start with all, and drop them if the results get large.
