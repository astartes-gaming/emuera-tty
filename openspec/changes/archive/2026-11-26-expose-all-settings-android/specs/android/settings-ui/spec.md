# Spec Delta

## Purpose

Provides a comprehensive settings screen in the Android app that exposes all emuera-tty configuration options, organized by category (Language, Display, Input, Translation, Advanced), with live updates where supported.

## ADDED Requirements

### Requirement: Settings screen accessible from game menu
The Android app SHALL provide a "Settings" entry in the in-game menu that opens a full-screen settings activity.

#### Scenario: Open settings from game menu
- **WHEN** user taps the gear icon in the game screen and selects "Settings"
- **THEN** a settings activity opens showing all emuera-tty settings categories

### Requirement: Language settings exposed
The settings screen SHALL display and allow changing the interface language, readable colours, wide symbols handling, backslash-as-yen, and Greek/Cyrillic rendering.

#### Scenario: Change interface language
- **WHEN** user selects "Language" and chooses "ja"
- **THEN** the app's menus and messages switch to Japanese immediately

### Requirement: Image rendering toggle exposed
The settings screen SHALL display and allow toggling image rendering on/off (enabled by default; disabling hides all pictures and sprites).

#### Scenario: Disable image rendering
- **WHEN** user turns off "Disable image rendering"
- **THEN** the engine stops rendering pictures and sprites, showing only text

#### Scenario: Enable image rendering
- **WHEN** user turns on "Disable image rendering" (re-enabling images)
- **THEN** the engine resumes rendering pictures and sprites

### Requirement: Input settings exposed
The settings screen SHALL display and allow changing clipboard behavior, status bar details, memory display, keyboard pointer, gamepad support, wheel scroll lines, tab completion, and scroll-to-end behavior.

#### Scenario: Enable keyboard pointer
- **WHEN** user enables "Keyboard pointer"
- **THEN** cursor keys move a pointer on screen and Insert/Shift+Enter click

#### Scenario: Change wheel scroll lines
- **WHEN** user sets "Wheel scrolls" to 10
- **THEN** mouse wheel scrolls 10 lines per step

### Requirement: Translation settings category exposed
The settings screen SHALL provide a "Translation" category that opens the translation configuration sub-screen.

#### Scenario: Open translation settings
- **WHEN** user taps "Translation" category
- **THEN** the translation configuration screen opens

### Requirement: Advanced settings exposed
The settings screen SHALL display and allow changing warmup (decode last pictures), console mouse, frontend display mode, and target EE version.

#### Scenario: Change target EE version
- **WHEN** user selects "Target EE version" and chooses "EEv57"
- **THEN** the engine emulates Emuera-EE v57 compatibility

### Requirement: Settings persist across sessions
All settings changes SHALL be written to the emuera-tty config file and survive app restarts.

#### Scenario: Settings persist after restart
- **WHEN** user changes a setting and restarts the app
- **THEN** the setting retains the changed value

### Requirement: Live settings take effect immediately
Settings marked as live in emuera-tty SHALL apply to the running game without restart.

#### Scenario: Live setting applies immediately
- **WHEN** user changes "Readable colours" from "normal" to "high" while a game runs
- **THEN** the game's text colours update immediately without restart

### Requirement: Game-specific settings accessible
The settings screen SHALL provide access to the game's own Emuera settings (emuera.config) organized by tabs (Environment, Display, Font, System2, Clipboard).

#### Scenario: Open game settings
- **WHEN** user taps "Game settings" in the settings screen
- **THEN** the game's Emuera settings dialog opens with all tabs