# Spec Delta

## Purpose

Replay a real game's play-through against a release build in a pseudo-terminal, failing on script errors, crashes, unexpected screens or malformed terminal output, the same way on a developer's machine and in CI.

## ADDED Requirements

### Requirement: Games pinned to a commit
Each game a replay uses SHALL be listed with its repository URL, the exact commit to check out, and whether it needs a credential. A replay SHALL run against that commit, never against whatever the repository's branch holds.

#### Scenario: A game moves on
- **WHEN** a game's repository gains new commits after its pin was set
- **THEN** replays still run against the pinned commit, and updating the pin is a deliberate change reviewed like code

#### Scenario: A game that needs a credential, without it
- **WHEN** a replay's game needs a credential and none is available
- **THEN** that replay is reported as skipped, not failed, and the other replays still run

### Requirement: Game folders never modified
A replay SHALL run on a throwaway copy of the game: scripts and CSV files copied, pictures and sound linked or read in place, settings and save data in their own temporary folders. The cloned or local game folder SHALL be unchanged afterwards.

#### Scenario: A replay that saves
- **WHEN** a replay's script saves the game
- **THEN** the save is written in the throwaway copy, and the game folder holds no new or changed file

### Requirement: Replay script steps
A replay script SHALL be a plain text file with one step per line, covering:
- typing text with Enter;
- pressing a key;
- clicking the last place a label shows on screen;
- clicking a screen cell;
- waiting until the game is idle or for a time;
- expecting text on screen within a time limit;
- taking a named screenshot.

Lines starting with `#` SHALL be comments.

#### Scenario: Clicking a picture menu
- **WHEN** a step clicks a screen cell on pops-tw's title, which is a picture menu with no text labels
- **THEN** the game receives a mouse move and click at that cell, as from a real mouse

#### Scenario: An unknown step
- **WHEN** a script contains a step the runner doesn't know
- **THEN** the replay fails before starting the game, naming the file and line

### Requirement: Deterministic replays
A replay SHALL run the game with a fixed random seed, terminal size and settings, so the same build, game commit and script give the same screens on every run and machine.

#### Scenario: Two runs of the same replay
- **WHEN** a replay runs twice with the same build, game pin and script
- **THEN** every `expect` step finds its text both times

### Requirement: What fails a replay
A replay SHALL fail when:
- the game reports a script error;
- emuera-tty exits unexpectedly or is killed by a signal;
- an `expect` step's text doesn't appear in time;
- the game stays busy beyond the step's time limit.

The failure report SHALL name the script line, and keep the last screen and emuera-tty's error output.

#### Scenario: A script error mid-replay
- **WHEN** the game prints a script error while a replay runs
- **THEN** the replay fails at the current step, and the report holds the error text and the screen at that moment

#### Scenario: A crash
- **WHEN** emuera-tty dies from a signal during a replay
- **THEN** the replay fails and reports the signal and the last step sent

### Requirement: Terminal output checked
While a replay runs, every picture command emuera-tty writes (kitty graphics, including tmux-wrapped ones; sixel; iTerm2) SHALL be checked as well-formed: chunks complete, payload encoding valid, and compressed data decoding to the size the command declares. Any malformed command SHALL fail the replay.

#### Scenario: A corrupt compressed picture
- **WHEN** emuera-tty sends a compressed kitty picture whose data doesn't decode to its declared width × height × 4 bytes
- **THEN** the replay fails and names the picture id and the step that produced it

#### Scenario: A picture split across chunks
- **WHEN** a kitty picture arrives in several chunks
- **THEN** the chunks are joined before checking, and an unterminated sequence at the end of the replay fails it

### Requirement: Same runner on Linux and Windows
The replay runner SHALL run on Linux through a pseudo-terminal and on Windows through the console's pseudo-console (ConPTY), with the same scripts and the same pass/fail rules. Steps a platform can't perform SHALL be marked as such in the script and skipped there.

#### Scenario: A replay on Windows
- **WHEN** a replay script runs on Windows
- **THEN** emuera-tty runs in a ConPTY console, the steps are sent as console input, and the result is judged by the same rules as on Linux

### Requirement: Memory-capped, at most two at once
Each game process in a replay SHALL run under an address-space limit (4 GB by default). The runner SHALL run at most two replays at a time unless told otherwise.

#### Scenario: Several replays requested
- **WHEN** a developer asks for all replays at once
- **THEN** no more than two games run at the same time, each capped
