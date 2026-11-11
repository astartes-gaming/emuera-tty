# Spec Delta

## Purpose

Emuera's debug features: the `@` commands typed at the game's input, and the debug window's variable watch, stack trace and console.

## ADDED Requirements

### Requirement: @ commands at the input
A typed input line of more than one character starting with `@`, at an input that is not ONEINPUT, SHALL run as Emuera's system command instead of being given to the script: `@REBOOT`, `@OUTPUT` / `@OUTPUTLOG`, `@QUIT` / `@EXIT`, `@CONFIG`, `@DEBUG` (debug mode only); any other text runs as a debug command when デバッグコマンドを使用する is on, else prints Emuera's error.

#### Scenario: A debug command
- **WHEN** debug commands are allowed and the player types `@FLAG:3 = 5`
- **THEN** FLAG:3 becomes 5, the line is echoed, and the input request is still waiting

#### Scenario: An expression
- **WHEN** debug commands are allowed and the player types `@FLAG:3 * 2`
- **THEN** its value is printed

#### Scenario: Debug commands not allowed
- **WHEN** デバッグコマンドを使用する is NO and the player types `@FLAG:3 = 5`
- **THEN** Emuera's "debug commands cannot be used" error is printed and FLAG:3 is unchanged

#### Scenario: A forbidden statement
- **WHEN** the player types `@WAIT` or `@GOTO X` or `@SAVEDATA 0, "x"`
- **THEN** Emuera's error for that statement is printed and nothing runs

### Requirement: Debug panel
In debug mode, a debug panel SHALL show the variable watch list (expressions and their current values, editable), the stack trace of the running script, and the debug console (DEBUGPRINT output, with a line to run debug commands and its history); the watch list and console log SHALL be saved in the game's `debug/` folder as Emuera saves them.

#### Scenario: Watch list kept between sessions
- **WHEN** the player adds `FLAG:3` to the watch list and quits
- **THEN** `debug/watchlist.csv` holds `FLAG:3`, and the next session (in emuera-tty or Emuera) shows it again

#### Scenario: Not in debug mode
- **WHEN** the game runs without `--game-debug` and the player types `@DEBUG`
- **THEN** Emuera's "debug window cannot be used" error is printed
