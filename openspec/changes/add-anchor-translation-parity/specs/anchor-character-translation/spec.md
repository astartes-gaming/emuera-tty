# Spec Delta

## Purpose

Applies Emuera-Anchor's `TRCHARA*.csv` character translations to the game's character templates, so characters added in a new game carry the translated names and strings Anchor would give them.

## ADDED Requirements

### Requirement: TRCHARA files
Every file under the CSV folder whose name starts with `TRCHARA` and ends with `.csv`, both matched case-insensitively, SHALL be read as character translation data.

#### Scenario: Name with a suffix in a subfolder
- **WHEN** the game has `CSV/chara/TRCHARA(Reimu).csv`
- **THEN** its translations are applied

#### Scenario: Lower-case file name
- **WHEN** the file is named `trchara_main.CSV`
- **THEN** its translations are applied

### Requirement: Character blocks
A block SHALL start with `NO,n` or `番号,n` and end with `ENDCHR` or `ENDCHAR`, with or without a following number. Inside a block, the keys `NAME`/`名前`, `CALLNAME`/`呼び名`, `NICKNAME`/`あだ名` and `MASTERNAME`/`主人の呼び方` take the second field, and `CSTR,i,text` sets CSTR index `i` to the third field (text after a further comma is dropped, as in Anchor). Keys SHALL be matched case-insensitively; lines starting with `;` are comments.

#### Scenario: Full block
- **WHEN** a block reads `NO,1` / `NAME,Reimu Hakurei,` / `CALLNAME,Reimu,` / `CSTR,3,shrine, maiden` / `ENDCHAR,1,`
- **THEN** character 1's template has NAME `Reimu Hakurei`, CALLNAME `Reimu` and CSTR:3 `shrine`

### Requirement: Only given fields are replaced
A block SHALL replace only the fields it gives, and only those the character's own `Chara*.csv` gives (a name or CSTR index that CSV lacks stays unset, as in Anchor, which translates each field as it reads that line). Fields the block does not mention SHALL keep their `Chara*.csv` values. (Anchor blanks NAME, CALLNAME, NICKNAME and MASTERNAME when a block leaves them out; this is not reproduced.)

#### Scenario: Block with only CALLNAME
- **WHEN** character 2's block gives only `CALLNAME,Marisa,`
- **THEN** character 2's NAME is still the one from its Chara*.csv

#### Scenario: Field the Chara*.csv lacks
- **WHEN** a block gives NICKNAME for a character whose Chara*.csv has no NICKNAME
- **THEN** the character's NICKNAME stays unset

### Requirement: Translated templates
The translations SHALL apply to the character templates, so every character added from a template (ADDCHARA, ADDDEFCHARA, a new game) carries them. Characters already in a loaded save SHALL keep the values stored in the save.

#### Scenario: New game
- **WHEN** a new game adds character 1
- **THEN** its NAME is `Reimu Hakurei`

#### Scenario: Old save
- **WHEN** a save made before TRCHARA was added is loaded
- **THEN** its characters keep their saved names

### Requirement: TRCHARA warnings
These SHALL produce a load warning naming the file and line, and loading SHALL continue:
- a line with fewer than two fields, or one starting with a comma;
- a second `NO` before `ENDCHR`;
- data before any `NO`;
- a `NO` that isn't an integer, or names no character;
- an unknown key;
- a CSTR line without a third field;
- a CSTR index given twice in a block (the later one wins).

#### Scenario: Unknown key
- **WHEN** a block holds `TALENT,1,x`
- **THEN** a warning names the file and line, and the rest of the block applies

#### Scenario: Missing ENDCHR
- **WHEN** a `NO,2` line follows `NO,1` without an `ENDCHR` between them
- **THEN** a warning is shown for the second `NO`
