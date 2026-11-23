# Spec Delta

## Purpose

What games written for Emuera.NET get here: its built-ins, its HTML_PRINT dialect and its setting.json.

## ADDED Requirements

### Requirement: Emuera.NET built-ins a game uses
The Emuera.NET built-ins used by the local games written for it SHALL behave as in Emuera.NET.

#### Scenario: A CSV number by nickname
- **WHEN** a script calls `GETCSVNOBYNICKNAME("Miko")` and characters 5 and 7 have that nickname
- **THEN** it returns 5; with no such character, -1

#### Scenario: MATCHALL
- **WHEN** a script runs `MATCHALL ARR, 3` on an array holding 3 at indices 0, 2 and 4
- **THEN** RESULT is 3 and RESULT:1…3 are 0, 2, 4

### Requirement: Emuera.NET's HTML dialect
HTML_PRINT SHALL accept Emuera.NET's `<div>` and `<img>` attributes, size a `<div>` without width or height to its content, and start its lines left-aligned whatever ALIGNMENT says.

#### Scenario: A div without a size
- **WHEN** a script prints `<div xpos='100px'><img src='LOGO' height='3000'></div>`
- **THEN** the div is as wide as the picture and placed 100 px from the line's start

#### Scenario: ALIGNMENT CENTER
- **WHEN** ALIGNMENT is CENTER and a script HTML_PRINTs a line without `<p align>`
- **THEN** the line is laid out from the left, and ALIGNMENT is still CENTER afterwards

### Requirement: setting.json
The switches of a game's `setting.json` that change how scripts run SHALL be honoured.

#### Scenario: Renames in character CSVs
- **WHEN** `setting.json` has `"UseRenameInCharaCSV":true` and a Chara CSV holds `[[name]]`
- **THEN** it is replaced from `_Rename.csv` as in ERB files
