# Spec Delta

## Purpose

Run the game replays in the terminals players use (Ghostty and kitty, each directly and inside tmux; the Windows console) on a schedule in CI, and fail when the terminal itself breaks.

## ADDED Requirements

### Requirement: When the matrix runs
The terminal matrix SHALL run:
- nightly on the upstream repository's default branch;
- on every release tag;
- on demand, for a chosen branch.

It SHALL NOT run on every push: the per-push `cargo test` job stays as it is.

#### Scenario: A push to a branch
- **WHEN** someone pushes a commit
- **THEN** only the existing per-push jobs run, and the matrix doesn't start

#### Scenario: A release tag
- **WHEN** a release tag is pushed
- **THEN** the full matrix runs against that tag's release build

#### Scenario: On demand
- **WHEN** a maintainer starts the matrix by hand for a branch
- **THEN** it runs against that branch, optionally limited to chosen games and terminals

### Requirement: Terminals covered
Each replay SHALL run in each of these terminals:
- Ghostty, directly;
- Ghostty inside tmux;
- kitty, directly;
- kitty inside tmux;
- the Windows console through ConPTY.

Each terminal's version SHALL be pinned and reported with the results.

#### Scenario: A full nightly run
- **WHEN** the nightly matrix runs with all three games available
- **THEN** each game is replayed in all five terminals, and the summary shows one result per game and terminal, with the terminal versions

### Requirement: The terminal dying fails the run
A replay in a real terminal SHALL fail when the terminal process exits or crashes before the replay ends, even if emuera-tty itself reported nothing wrong. A crash's signal, and its backtrace where the system keeps one, SHALL be kept with the result.

#### Scenario: The terminal crashes on a picture
- **WHEN** Ghostty dies from a signal while a replay shows a picture
- **THEN** that game-and-terminal result fails, naming the signal and the last replay step sent

### Requirement: A picture missing fails the run
At steps the script marks as showing a picture, the terminal's window SHALL be captured. The run SHALL fail when the area where the picture belongs is a single flat colour.

#### Scenario: A blank title
- **WHEN** a step marked as showing pops-tw's title is captured and the title area is one flat colour
- **THEN** the result fails and keeps the screenshot

### Requirement: Results kept
Each run SHALL keep, per game and terminal, as downloadable results:
- the replay log;
- emuera-tty's error output;
- the terminal's own log;
- the screenshots.

They SHALL be kept for at least 14 days.

#### Scenario: Looking into a failure
- **WHEN** a nightly result fails
- **THEN** its log, error output, terminal log and screenshots can be downloaded from the run

### Requirement: Private games through a secret
A game that isn't publicly readable SHALL be fetched with a read-only credential stored as a repository secret. Where the secret isn't available (forks, pull requests from forks), that game's results SHALL be reported as skipped, and the rest of the matrix SHALL run.

#### Scenario: A run in a fork
- **WHEN** the matrix runs in a fork without the eraTohoK secret
- **THEN** eraTohoK's results show as skipped, and pops-tw and AnonTW run as usual

#### Scenario: The secret never shown
- **WHEN** the matrix runs with the secret
- **THEN** the credential doesn't appear in any log or kept result

### Requirement: One result doesn't stop the others
A failing game or terminal SHALL NOT cancel the other results of the same run. The run as a whole SHALL fail when any result fails.

#### Scenario: Ghostty fails, kitty passes
- **WHEN** a replay fails in Ghostty and passes in every other terminal
- **THEN** all results are reported, and the run is marked failed
