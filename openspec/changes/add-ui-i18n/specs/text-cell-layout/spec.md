# Spec Delta

## Purpose

Defines how text of any script occupies terminal cells, for game text and
emuera-tty's own interface alike, so that lines, boxes and menus stay
aligned and no part of a character is lost.

## ADDED Requirements

### Requirement: Interface layout measures display width

Wherever emuera-tty lays out its own interface (box widths, padding,
truncation, the bottom row), it SHALL measure text in terminal columns, not
in characters: two columns for wide (CJK) characters, none for combining
marks.

#### Scenario: A box with CJK text

- **WHEN** the settings overlay shows a row in Japanese
- **THEN** the row's right border is in the same column as every other row's

#### Scenario: Truncating a long label

- **WHEN** a translated label is wider than its space
- **THEN** it is cut to fit the space in columns, and never in the middle of a wide character

### Requirement: Combining marks stay with their base character

A combining mark (zero width, such as a Devanagari vowel sign or virama, or
an accent written as a separate character) SHALL be drawn together with the
character before it, in game text and interface text alike, and SHALL NOT
be dropped or given a cell of its own.

#### Scenario: Hindi text

- **WHEN** a line contains "हिन्दी"
- **THEN** every code point of it reaches the terminal, in order, and the line takes the columns its grapheme clusters occupy

#### Scenario: A decomposed accent

- **WHEN** a line contains "e" followed by U+0301 COMBINING ACUTE ACCENT
- **THEN** both are written to the same cell and the line is one column wide there

#### Scenario: Wrapping keeps a cluster whole

- **WHEN** a line wraps just after a base character that is followed by a combining mark
- **THEN** the mark stays with the base character on the first row

### Requirement: Characters beyond the Basic Multilingual Plane are laid out

Characters outside the Basic Multilingual Plane (U+10000 and above, such as
Old Avestan or emoji) SHALL be laid out with their display width like any
other character, and SHALL NOT shift what follows on the line.

#### Scenario: Old Avestan

- **WHEN** a line contains Old Avestan letters (U+10B00–U+10B35)
- **THEN** each takes one column and the text after them starts where their widths add up to

### Requirement: Right-to-left text is written in logical order

Right-to-left text (Arabic, Old Avestan) SHALL be written to the terminal in
logical (storage) order, with its display width, leaving any reordering to
the terminal. emuera-tty SHALL NOT reorder it itself.

#### Scenario: An Arabic word inside a line

- **WHEN** a line contains an Arabic word between two English words
- **THEN** the characters are written in the order they are stored, and the English word after it starts in the column given by the widths

### Requirement: Layout is checked in several scripts

The test suite SHALL render the interface in pseudo-translations written in
CJK, Arabic, Cyrillic, Devanagari and Old Avestan scripts, and SHALL fail if
a box border is out of line, a label overflows its space, or any code point
of a message is missing from the output.

#### Scenario: A layout regression

- **WHEN** a change makes a settings row one column too wide in the Devanagari pseudo-translation
- **THEN** `cargo test` fails and names the row and the script
