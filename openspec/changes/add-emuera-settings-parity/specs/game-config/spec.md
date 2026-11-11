# Spec Delta

## Purpose

Which of Emuera's settings emuera-tty reads, from which files, and what each one does here.

## ADDED Requirements

### Requirement: Every Emuera setting is read
emuera-tty SHALL read every key of Emuera's settings window and debug settings from `CSV/_default.config`, the game's `emuera.config` and `CSV/_fixed.config`, in that order with later files winning, under its Japanese or English name (English case-insensitively), and SHALL use Emuera's default for a key no file sets.

#### Scenario: An English key
- **WHEN** a game's `emuera.config` has `VALID EXTENSIONS FOR LOADTEXT AND SAVETEXT:xml`
- **THEN** LOADTEXT and SAVETEXT accept `.xml` files

#### Scenario: Fixed config wins
- **WHEN** `emuera.config` sets a key to YES and `CSV/_fixed.config` sets it to NO
- **THEN** the setting is NO

### Requirement: Script-behaviour settings are honoured
The settings of Emuera's System, Compatibility and Debug tabs SHALL change how scripts are loaded and run as they do in Emuera-EM/EE.

#### Scenario: Full-width space not whitespace
- **WHEN** a game sets `全角スペースをホワイトスペースに含める:NO` and a line separates tokens with U+3000
- **THEN** the line is read as Emuera reads it with that setting

#### Scenario: Automatic TOSTR for user-function arguments
- **WHEN** a game sets `ユーザー関数の引数に自動的にTOSTRを補完する:YES` and passes an integer to a string parameter of a user function
- **THEN** the call converts it as Emuera does instead of failing

#### Scenario: _Rename.csv off
- **WHEN** a game has `_Rename.csv` and sets `_Rename.csvを利用する:NO`
- **THEN** `[[name]]` in scripts is not replaced

### Requirement: Every setting is accounted for
Each setting SHALL be either honoured, given a terminal equivalent, or listed as not applicable with the reason in `doc/settings-parity.md`.

#### Scenario: A window-only setting
- **WHEN** a game sets `ウィンドウ位置X:200`
- **THEN** nothing changes, and the parity table says why

### Requirement: The game settings page shows Emuera's settings
The game settings page SHALL show Emuera's tabs and items; an item that has no effect in a terminal SHALL be shown as such, and an item locked by `_fixed.config` SHALL not be editable.

#### Scenario: A locked item
- **WHEN** `_fixed.config` sets an item and the player selects it on the page
- **THEN** the page says the game fixes it and does not change it
