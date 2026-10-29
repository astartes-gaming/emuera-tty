# Tasks

## 1. Shared pty driver

- [ ] 1.1 Move gamed.py's pty start, pyte screen and CPU-based idle detection into `tools/ptydrive.py`, and make `tools/gamed.py` import them. Verify: `tools/gamed.py` + `tools/g.sh` still drive AnonTW to the first menu, and the screen text is unchanged.

## 2. Game pins and throwaway copies

- [ ] 2.1 Add `tests/replay/games.toml` with pops-tw and AnonTW (`Legis1998/anon-tw`) pinned to commits, pops-tw's LFS paths, and the copy/link rules. Verify: `python3 -c "import tomllib; tomllib.load(open('tests/replay/games.toml','rb'))"` loads it, and a unit check rejects an entry without a commit.
- [ ] 2.2 Add eraTohoK with `secret = "GITGUD_ERATOHOK_TOKEN"`. Verify: with the variable unset, `tools/replay.py --fetch eratohok` reports "skipped", not an error.
- [ ] 2.3 Fetch a pinned game (blob-less clone, checkout, LFS pull of the named paths), using a `GIT_ASKPASS` helper for secrets, and make the throwaway copy. Verify: the token doesn't appear in `ps` output or in the log, and the source folder's `git status` is clean after a replay that saves.
- [ ] 2.4 Document the pin format and how to update a pin in `tests/replay/README.md`.

## 3. Replay scripts and the plain runner

- [ ] 3.1 Implement the script parser: `type`, `key`, `click LABEL`, `click ROW COL`, `wait idle|SECS`, `expect TEXT [SECS]`, `shot NAME [picture ROW COL ROWS COLS]`, `skip-on`, `#` comments. Verify: unit tests, run with `python3 -m unittest`, cover each step, and an unknown step fails with file:line before the game starts.
- [ ] 3.2 Run a script in a pty against a release build, with a fixed seed, size and settings, under a 4 GB address-space cap, at most two at once. Verify: running the same script twice passes both times, and `--jobs 3` still runs only two games at once unless `--max-jobs` raises it.
- [ ] 3.3 Fail on script errors, unexpected exit or signal, a missed `expect`, or a busy timeout. Keep the last screen and stderr. Verify: a deliberately broken script (an `expect` for text that never appears) and a game with an ERB that THROWs both fail, with the step line in the report.
- [ ] 3.4 Write the first replays (`tests/replay/pops-tw.txt`, `anon-tw.txt`, `eratohok.txt`): title, new game, a few in-game days, save, load, with `shot … picture` on pops-tw's title and loading screen. Verify: each passes locally twice against the current release build.
- [ ] 3.5 Document running replays locally (README *Testing* section and `tests/replay/README.md`). Verify: following the README from a fresh checkout runs the AnonTW replay.

## 4. Output checks

- [ ] 4.1 Parse APC/DCS sequences from the raw output: unwrap tmux passthrough, join kitty chunks, and decode base64 and `o=z`. Verify: unit tests with captured output from today's pops-tw run pass, and a payload with one corrupted byte fails with the picture id.
- [ ] 4.2 Check sixel and iTerm2 sequences for termination, and fail on any sequence left unterminated at the end. Verify: unit tests with a truncated sixel and a truncated kitty chunk.
- [ ] 4.3 Run the checks during every replay. Verify: the pops-tw replay under `--gfx kitty` passes, and a build with compression deliberately broken fails at the title step.

## 5. Shim mode and real terminals (local)

- [ ] 5.1 Add `replay.py --shim`, which runs emuera-tty in its own pty inside the terminal: output copied to the terminal and to the checks, terminal replies passed back, steps injected. Verify: inside a local Ghostty, the AnonTW replay passes and the window shows the game.
- [ ] 5.2 Add pinned terminal configs (`tests/replay/terminals/`: Ghostty, kitty, tmux.conf) and a launcher that starts Xvfb, the terminal (optionally with tmux) and the shim, waits on the terminal, and records its exit status or signal. Verify: running the pops-tw replay in Ghostty under Xvfb passes, and a build from before `f835bfd` (compression at level 1) fails with Ghostty's SIGSEGV reported.
- [ ] 5.3 Capture screenshots with `import`, and implement the flat-area picture check using the pinned cell size. Verify: pops-tw's title shot passes, and the same shot with `--gfx none` fails as blank.
- [ ] 5.4 Collect backtraces from `coredumpctl` when present, and keep the terminal's log. Verify: the pre-`f835bfd` Ghostty failure includes the `compress.flate.Decompress` frame.

## 6. Windows console

- [ ] 6.1 Add ConPTY mode with `pywinpty` (added to `tools/requirements.txt` for Windows only), with the same steps and checks, and steps marked `skip-on windows` skipped. Verify: on a Windows machine, the AnonTW replay passes in the console, and a THROW in the game fails it.

## 7. Workflow

- [ ] 7.1 Add `.github/workflows/replay.yml` with schedule, release-tag and `workflow_dispatch` (games, terminals) triggers, and a release build job for Ubuntu and Windows that uploads artifacts. Verify: a manual run on the fork builds both binaries.
- [ ] 7.2 Add the games job: cache keyed by pin, fetch on a miss, eraTohoK through the secret, and "skipped" when the secret is absent. Verify: a run in the fork (no secret) shows eraTohoK skipped, and a second run restores the games from the cache.
- [ ] 7.3 Add the cached terminal builds: kitty's release tarball, and Ghostty built from its pinned tag with its Zig (falling back to a pinned Fedora container if Ubuntu's GTK is too old). Verify: the second run takes both from the cache, and the summary shows their versions.
- [ ] 7.4 Add the Linux replay matrix (game × ghostty, ghostty-tmux, kitty, kitty-tmux) with `fail-fast: false`, systemd-coredump and Japanese fonts installed, and results kept for 14 days. Verify: a manual run on the fork completes all jobs and their results download.
- [ ] 7.5 Add the Windows replay matrix (game × console). Verify: a manual run on the fork completes the Windows jobs.
- [ ] 7.6 Add the summary job: a table of results with terminal versions in the run summary, failing when any result failed. Verify: a run with one deliberately failing script shows that cell failed, the others passed, and the run marked failed.
- [ ] 7.7 Document the workflow, the secret and the triggers in the README and in `tests/replay/README.md`. Verify: the docs name the secret, what happens without it, and how to start a run by hand.

## 8. Integration check

- [ ] 8.1 Run the full matrix on the fork against the current release, and against a build from before `f835bfd`. Verify: the current build passes everywhere (eraTohoK skipped without the secret), and the old build fails exactly the Ghostty cells, with the backtrace in the results.

## Workflow follow-up

- The upstream maintainer adds a read-only gitgud deploy token for eraTohoK as `GITGUD_ERATOHOK_TOKEN` and enables the workflow upstream.
- After a week of stable nightly runs, decide whether release tags wait on the matrix.
- Archive the change with `openspec archive add-release-integration-tests` once merged upstream.
