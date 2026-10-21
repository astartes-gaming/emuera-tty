# Spec Delta

## Purpose

Supports Emuera-Anchor's `PARAM.CSV`, the English-friendly name for the parameter table, which Anchor reads in place of `PALAM.CSV` and lets scripts refer to as `PARAM`.

## ADDED Requirements

### Requirement: PARAM.CSV replaces PALAM.CSV
When the CSV folder holds `PARAM.CSV`, matched case-insensitively, it SHALL be read as the parameter name table and `PALAM.CSV` SHALL NOT be read. Messages about the table SHALL name `PARAM.CSV`.

#### Scenario: Both files present
- **WHEN** the game has `PARAM.CSV` with `0,Pleasure C` and `PALAM.CSV` with `0,快Ｃ`
- **THEN** `PALAMNAME:0` is `Pleasure C`

#### Scenario: Only PALAM.CSV
- **WHEN** the game has only `PALAM.CSV`
- **THEN** it is read as before

### Requirement: PARAM identifier renaming
When the game has `PARAM.CSV` or any `*_TR.csv` / `TRCHARA*.csv` file, an identifier SHALL have each `PARAM` replaced by `PALAM` when the result is a built-in variable, and a variable not found SHALL be looked for with that replacement; so SHALL VARSIZE's string argument when it is exactly `PARAM`. Other games' identifiers SHALL be left as written. (Anchor renames every identifier in every game.)

#### Scenario: Built-in variables
- **WHEN** a game with PARAM.CSV runs `PARAM:TARGET:0 += 10` and `PRINTFORML %PARAMNAME:0%`
- **THEN** PALAM:TARGET:0 increases by 10 and `Pleasure C` is printed

#### Scenario: User variable with PARAM in its name
- **WHEN** the game declares `#DIM MYPARAM` and uses `MYPARAM`, and declares `#DIM OTHERPALAM` and uses `OTHERPARAM`
- **THEN** `MYPARAM` is its own variable (in Anchor it is an error), and `OTHERPARAM` is `OTHERPALAM`, as in Anchor

#### Scenario: Variable-name string
- **WHEN** a game with PARAM.CSV evaluates `VARSIZE("PARAM")`
- **THEN** the result is PALAM's size

#### Scenario: Translated game with PALAM.CSV
- **WHEN** a game with `Talent_TR.csv` and only `PALAM.CSV` runs `PARAM:TARGET:0 += 10`
- **THEN** PALAM:TARGET:0 increases by 10

#### Scenario: Untranslated game without PARAM.CSV
- **WHEN** a game with no PARAM.CSV and no TR files declares `#DIM PARAMX` and `#DIM PALAMX`
- **THEN** they are two separate variables

### Requirement: Param_TR
When `PARAM.CSV` was read, the parameter names SHALL be translated by `Param_TR.csv`, and `Palam_TR.csv` SHALL NOT be used. Otherwise `Palam_TR.csv` SHALL be used.

#### Scenario: PRINT_PALAM with Param_TR
- **WHEN** a game with PARAM.CSV and translation files has a `Param_TR.csv` entry for a name and shows PRINT_PALAM
- **THEN** the Param_TR translation is shown
