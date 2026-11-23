# Spec Delta

## Purpose

Fonts a game ships in its `font/` folder, and the characters only they have.

## ADDED Requirements

### Requirement: Game-folder fonts
Fonts in the game's `font/` folder SHALL be known by family and file name, and used where the game's settings or scripts name them.

#### Scenario: The configured font
- **WHEN** `emuera.config` sets `フォント名:EraMTFont` and `font/EraMTFont.ttf` holds that family
- **THEN** GETFONT returns `EraMTFont` while pictures can be shown, and pictures drawn with GDRAWTEXT use it

### Requirement: Characters only the game's font has
A private-use character the game's font has SHALL be drawn as a picture of its glyph, in the text's colour, where the terminal shows pictures; where it can't, the game SHALL be told its font isn't in use, so a script with a text fallback uses it.

#### Scenario: A dungeon tile with pictures
- **WHEN** eraMegatenP prints its map in a kitty-protocol terminal
- **THEN** each tile shows as EraMTFont's glyph in its colour, as in Emuera.NET

#### Scenario: No pictures
- **WHEN** the same game runs with `--gfx none`
- **THEN** GETFONT does not return `EraMTFont`, and the map is printed with the game's emoji tiles
