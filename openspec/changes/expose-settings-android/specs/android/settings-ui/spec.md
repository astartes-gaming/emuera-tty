# Spec Delta

## MODIFIED Requirements

### Requirement: Settings screen accessible from game menu
The Android app SHALL provide a "Settings" entry in the in-game menu and in the game list's menu, both opening the same full-screen settings activity.

#### Scenario: Open settings from game menu
- **WHEN** user taps the gear icon in the game screen and selects "Settings"
- **THEN** a settings activity opens showing all emuera-tty settings categories

#### Scenario: Open settings from the game list
- **WHEN** user opens the menu of a game in the list and selects "Settings"
- **THEN** the same settings activity opens, showing the settings that apply to that app as a whole

### Requirement: Language settings exposed
The settings screen SHALL display and allow changing the interface language, readable colours, wide symbols handling, backslash-as-yen, and Greek/Cyrillic rendering, and SHALL say that the language changes the engine's own messages while the app's own text stays as it is.

#### Scenario: Change interface language
- **WHEN** user selects "Language" and chooses "ja"
- **THEN** the engine's messages (load warnings, script errors, emuera-tty's own menus) switch to Japanese immediately, and the app's own buttons stay in their own language

### Requirement: Image rendering toggle exposed
The settings screen SHALL display a toggle for image rendering (on by default; off hides all pictures and sprites), because the picture protocols of the terminals have no meaning on Android.

#### Scenario: Disable image rendering
- **WHEN** user turns off "Disable image rendering"
- **THEN** the engine stops rendering pictures and sprites, showing only text

#### Scenario: Enable image rendering
- **WHEN** user turns on "Disable image rendering" (re-enabling images)
- **THEN** the engine resumes rendering pictures and sprites

### Requirement: Input settings exposed
The settings screen SHALL display and allow changing the input settings that mean something on Android (scroll-to-end on a key press, the wheel's scroll lines, tab completion, the memory in the bottom row) and SHALL mark the ones that do not (the keyboard pointer, the gamepad, copying to the clipboard).

#### Scenario: Enable keyboard pointer
- **WHEN** user reads "Keyboard pointer"
- **THEN** it is marked as one that does nothing on Android, and says why

#### Scenario: Change wheel scroll lines
- **WHEN** user sets "Wheel scrolls" to 10
- **THEN** a scroll of the wheel's kind moves 10 lines per step

#### Scenario: A setting with no meaning on Android
- **WHEN** user reads "Keyboard pointer"
- **THEN** it is marked as one that does nothing on Android, and says why

### Requirement: Translation settings category exposed
The settings screen SHALL provide a "Translation" category that opens the translation configuration sub-screen, which says that the translator does not run on Android yet.

#### Scenario: Open translation settings
- **WHEN** user taps "Translation" category
- **THEN** the translation configuration screen opens, saying that the choices are kept but not applied yet

### Requirement: Advanced settings exposed
The settings screen SHALL display and allow changing the advanced settings that apply on Android (decoding the last used pictures, the target EE version), and SHALL mark the ones that do not (the console mouse, the display mode).

#### Scenario: Change target EE version
- **WHEN** user selects "Target EE version" and chooses "EEv57"
- **THEN** the next start of the game emulates Emuera-EE v57 compatibility

#### Scenario: A setting with no meaning on Android
- **WHEN** user reads "Console mouse"
- **THEN** it is marked as one that does nothing on Android, and says why

### Requirement: Settings persist across sessions
All settings changes SHALL be written to the emuera-tty config file the engine reads, and survive app restarts, so that the phone and the desktop program agree on them.

#### Scenario: Settings persist after restart
- **WHEN** user changes a setting and restarts the app
- **THEN** the setting retains the changed value

#### Scenario: The desktop program reads the same file
- **WHEN** user changes a setting on the phone and then runs emuera-tty on the same config file
- **THEN** the desktop program shows the same value

### Requirement: Live settings take effect immediately
Settings marked as live in emuera-tty SHALL apply to the running game at its next input, without a restart, and the settings that need a restart SHALL say so.

#### Scenario: Live setting applies immediately
- **WHEN** user changes "Readable colours" from "normal" to "high" while a game runs
- **THEN** the game's text colours update at the game's next input, without a restart

#### Scenario: A setting that needs a restart
- **WHEN** user changes a setting that emuera-tty reads while the game loads
- **THEN** the settings screen says it takes effect at the next start

## ADDED Requirements

### Requirement: Every setting comes from the engine's own model
The settings screen SHALL list the settings the engine has, with their labels, choices, values and help, without the app naming any setting of its own.

#### Scenario: The engine gains a setting
- **WHEN** a new setting is added to the engine
- **THEN** it appears in the app's settings screen with its own label and choices, and the app needs no change

### Requirement: Settings that do not apply on Android are shown and marked
The settings screen SHALL show every setting, including the ones Android cannot use, and SHALL mark each of those with the reason it does nothing here.

#### Scenario: A terminal-only setting
- **WHEN** user reads "Colours" (the colour depth sent to a terminal)
- **THEN** it is shown, marked as one that does nothing on Android, and says why

#### Scenario: The marking is the reason, not a blank
- **WHEN** a setting does not apply on Android
- **THEN** the screen says what it does elsewhere instead of leaving the player to guess

### Requirement: A setting can be put back to its default
The settings screen SHALL let the player take a setting back to the value it has when nothing was chosen.

#### Scenario: Put a setting back
- **WHEN** user puts "Readable colours" back to its default
- **THEN** the setting is removed from the config file and shows its default value again

## REMOVED Requirements

### Requirement: Game-specific settings accessible
**Reason**: the per-game screen (the game's `emuera.config`) has its own capability now, `android/game-settings`, because it is a different model with its own kinds, sources and effects.

**Migration**: the game's settings are reached from the game list's ⚙ and from the in-game menu, as before; the global settings screen links to the running game's settings.
