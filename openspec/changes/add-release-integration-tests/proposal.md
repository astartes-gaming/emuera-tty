# Proposal

## Why

`cargo test` runs tiny games headlessly, so it never sees what real games do in real terminals. In one week three bugs reached a release build that only showed up there:

- Ghostty crashed on every compressed picture.
- pops-tw's title flickered inside tmux.
- Pictures went missing when a tmux pane was hidden.

Each was found by hand, in the maintainer's own terminal. Long replays of real games, in the terminals players use, would catch this kind of bug before a release.

## What Changes

- **Replay scripts:** a game's play-through written as a script (keys, text, clicks on labels, waits and expected screen text), with the game pinned to a commit. The first scripts cover pops-tw, AnonTW and eraTohoK.
- **A local runner:**
  - runs a script against a release build in a pseudo-terminal on Linux, or a ConPTY console on Windows;
  - reports a failure when the game hits a script error, crashes, or stops matching the expected screen;
  - checks every picture command emuera-tty writes, for example that compressed payloads decode to the picture's size.
- **A terminal matrix:**
  - the same scripts replayed inside real terminals on a hidden X server: Ghostty and kitty, each directly and inside tmux;
  - on Windows, the console through ConPTY;
  - a failure when the terminal itself dies, or the screen stays blank where a picture belongs;
  - screenshots kept for review.
- **A scheduled GitHub Actions workflow in the upstream repository:**
  - runs nightly, on release tags and on demand, separately from the per-push `cargo test` job;
  - eraTohoK, which isn't public, comes in through a read-only deploy token stored as a secret, and its jobs are skipped where that secret is missing (forks, pull requests).
- No change to emuera-tty itself or to `cargo test`.

## Capabilities

### New Capabilities

- `game-replay`: replaying a real game from a script in a pseudo-terminal, what makes a replay pass or fail, and the checks on emuera-tty's terminal output. It runs the same way on a developer's machine and in CI.
- `terminal-matrix-ci`: the scheduled workflow, covering:
  - which games run in which terminals, when, and with which secrets;
  - what counts as the terminal failing;
  - what each run keeps (logs, screenshots).

### Modified Capabilities

None.

## Impact

- **New files:**
  - `tests/replay/`: scripts and the game pin list;
  - `tools/replay.py`: the runner, next to `tools/gamed.py`, which it reuses;
  - `.github/workflows/replay.yml`.
- **Dependencies:**
  - Python 3 with pyte, already used by `tools/`;
  - `pywinpty` on Windows only;
  - in CI only: Xvfb, Mesa, xdotool, ImageMagick, tmux, kitty, and a pinned Ghostty release built with its pinned Zig. No new Rust crates.
- **Upstream repository:**
  - a `GITGUD_ERATOHOK_TOKEN` secret, plus the maintainer's agreement to scheduled jobs;
  - cache space for the games (about 4 GB, under the 10 GB per-repository cache limit);
  - hosted-runner minutes, which are free for public repositories.
- **Game repositories:** cloned anonymously (pops-tw, AnonTW from `Legis1998/anon-tw`) or with the token (eraTohoK); never modified.
