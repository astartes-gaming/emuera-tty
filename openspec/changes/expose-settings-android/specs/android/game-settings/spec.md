# Spec Delta

## Purpose

The per-game settings screen of the Android app: every setting of Emuera's settings window, kept in the game's `emuera.config` (which Emuera.exe reads too), with the widget its kind needs, where its value comes from, and when it takes effect.

## ADDED Requirements

### Requirement: Every Emuera setting is listed
The screen SHALL list every setting of Emuera's settings window, grouped by Emuera's tabs, including the ones only Emuera.exe uses.

#### Scenario: A game with its own settings files
- **WHEN** the player opens the settings of a game that has `CSV/_default.config` and `emuera.config`
- **THEN** every setting of Emuera's settings window is listed, tab by tab, with its value in effect

#### Scenario: A setting only Emuera.exe uses
- **WHEN** the list holds a setting Emuera.exe uses and the phone has no use for (window geometry, the font for text drawn into pictures)
- **THEN** it is listed and marked as one only Emuera.exe uses

### Requirement: A setting is edited with the widget its kind needs
The screen SHALL offer a toggle for a yes/no setting, a number field with its range for a number, a colour field for a colour, a text field for text, and a list of the options for a setting Emuera has a drop-down for.

#### Scenario: Turn a yes/no setting off
- **WHEN** the player turns off "Make autosaves"
- **THEN** the setting's value becomes NO

#### Scenario: Choose from Emuera's drop-down
- **WHEN** the player opens the drop-down of "Warn about a function that is not found"
- **THEN** the options Emuera offers (ignore, total count only, once per file, show) are listed and one is chosen

#### Scenario: A number out of range
- **WHEN** the player types a number outside the setting's range
- **THEN** the value is refused and the range is shown

#### Scenario: A colour
- **WHEN** the player edits "Text colour"
- **THEN** the colour is written as Emuera writes it, R,G,B or #RRGGBB

### Requirement: Where a value comes from is shown
The screen SHALL show whether each value is Emuera's default, the game's, the player's, or one the game does not allow changing.

#### Scenario: A value the player set
- **WHEN** the player opens the settings of a game whose `emuera.config` holds a value
- **THEN** that setting is marked as the player's

#### Scenario: A value the game locks
- **WHEN** the game's `CSV/_fixed.config` holds a value
- **THEN** the control is disabled and says the game does not allow changing it

### Requirement: When a setting takes effect is shown
The screen SHALL say whether a setting takes effect at once, at the next start of the game, or only in Emuera.exe.

#### Scenario: A setting read while the game loads
- **WHEN** the player reads "Ignore case"
- **THEN** it is marked as one that takes effect at the next start

#### Scenario: An Emuera-only setting
- **WHEN** the player reads a setting emuera-tty does not use
- **THEN** it is marked as one only Emuera.exe uses (and that scripts can read with GETCONFIG)

### Requirement: A setting can be put back to the game's or Emuera's value
The screen SHALL let the player take one value out of `emuera.config`, and take out all the player's values at once.

#### Scenario: Put one value back
- **WHEN** the player puts one setting back to the game's value
- **THEN** that line is taken out of the game's `emuera.config` and the value in effect is shown again

#### Scenario: Put all the player's values back
- **WHEN** the player chooses "the game's defaults"
- **THEN** every value the player had set is taken out of `emuera.config`, and the ones the game fixed stay

### Requirement: The settings are kept in the game's emuera.config
The screen SHALL write the values into the game's `emuera.config`, in Emuera's format, so that Emuera.exe reads them too.

#### Scenario: Saving a value
- **WHEN** the player saves a changed value
- **THEN** the game's `emuera.config` holds the new value under Emuera's own key, and the other lines of the file are unchanged

#### Scenario: A value Emuera cannot read
- **WHEN** the game's `emuera.config` holds a value emuera-tty does not understand
- **THEN** the setting shows the game's or Emuera's default, as Emuera shows it

### Requirement: A running game takes the new settings at its next input
The screen SHALL give a running game its new settings without a restart, as the desktop does when the settings page closes.

#### Scenario: Change a setting while the game runs
- **WHEN** the player saves a setting from the game's menu and the game is running
- **THEN** the running game takes the new value at its next input

#### Scenario: Change a setting from the game list
- **WHEN** the player changes a setting from the list, with no game running
- **THEN** the next start of that game uses the new value
