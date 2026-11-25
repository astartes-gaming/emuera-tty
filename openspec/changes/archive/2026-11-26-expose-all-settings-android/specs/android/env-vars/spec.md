# Spec Delta

## Purpose

Provides an environment variables reference screen in the Android app that lists all EMUERA_* environment variables with their descriptions, allowing advanced users to understand and configure engine behavior through environment variables.

## ADDED Requirements

### Requirement: Environment variables list displayed
The env-vars screen SHALL display all EMUERA_* environment variables recognized by emuera-tty, each with its name and description.

#### Scenario: View environment variables list
- **WHEN** user opens the "Environment Variables" screen
- **THEN** a scrollable list shows all EMUERA_* variables with descriptions

### Requirement: Variables organized by category
The env-vars screen SHALL group variables logically (e.g., Display, Translation, Debugging, Performance, Input, Compatibility).

#### Scenario: Variables grouped by function
- **WHEN** user scrolls the env-vars list
- **THEN** variables are grouped under category headers like "Display", "Translation", "Debugging"

### Requirement: Variable descriptions match Rust source
Each variable's description SHALL match the authoritative description from `src/settings.rs` ENV_VARS.

#### Scenario: EMUERA_COLORS description accurate
- **WHEN** user views EMUERA_COLORS entry
- **THEN** description reads "truecolor, 256 or 16: the colour depth, when detection guesses wrong"

### Requirement: Copy variable name to clipboard
The env-vars screen SHALL allow long-pressing a variable to copy its name (e.g., "EMUERA_COLORS") to the clipboard.

#### Scenario: Copy variable name
- **WHEN** user long-presses "EMUERA_SEED"
- **THEN** "EMUERA_SEED" is copied to clipboard and a toast confirms

### Requirement: Search/filter variables
The env-vars screen SHALL provide a search field to filter the list by variable name or description.

#### Scenario: Search for translation variables
- **WHEN** user types "translate" in the search field
- **THEN** only EMUERA_TRANSLATE_* and EMUERA_TRANS_* variables are shown

### Requirement: Show which variables are currently set
The env-vars screen SHALL indicate which EMUERA_* variables currently have values in the process environment.

#### Scenario: Show set variables
- **WHEN** user sets EMUERA_SEED=42 before launching
- **THEN** EMUERA_SEED entry shows "Current: 42" or similar indicator