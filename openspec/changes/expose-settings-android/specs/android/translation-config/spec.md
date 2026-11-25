# Spec Delta

## MODIFIED Requirements

### Requirement: Translation engine selection
The translation screen SHALL display the translator choices emuera-tty has and allow choosing one, SHALL keep the choice in the config file, and SHALL say that the translator does not run on Android yet.

#### Scenario: Choose a translator
- **WHEN** user chooses "Ollama" from the translator list
- **THEN** the choice is kept in the config file, and the screen says the translator is not applied on Android yet

#### Scenario: Select Ollama as translator
- **WHEN** user chooses "Ollama" from the translator list
- **THEN** the choice is kept in the config file, and the screen says the translator is not applied on Android yet

#### Scenario: Select translate-shell (online)
- **WHEN** user chooses "translate-shell"
- **THEN** the choice is kept in the config file, and the screen says the translator is not applied on Android yet

#### Scenario: Select DeepL with API key
- **WHEN** user chooses "DeepL"
- **THEN** the choice is kept in the config file, and the screen says the translator is not applied on Android yet

#### Scenario: Select file-based translation only
- **WHEN** user chooses "file" (no translator)
- **THEN** the choice is kept in the config file, and the screen says the translator is not applied on Android yet

#### Scenario: The screen says what is not working
- **WHEN** user opens the translation screen
- **THEN** it says that the choices are kept for later and that the game is not translated on Android yet

### Requirement: Translate to language selection
The translation screen SHALL allow selecting the target language from: English, Spanish, Portuguese, French, German, Italian, Russian, Japanese, Chinese, Korean, and SHALL keep it in the config file.

#### Scenario: Change target language to Spanish
- **WHEN** user sets "Translate to" to "es"
- **THEN** the choice is kept in the config file, and the screen says the game is not translated on Android yet

### Requirement: Translate from language selection
The translation screen SHALL allow selecting the source language detection mode: auto (detect Japanese/English), Japanese only, English only, and SHALL keep it in the config file.

#### Scenario: Set source to Japanese only
- **WHEN** user sets "Translate from" to "ja"
- **THEN** the choice is kept in the config file, and the screen says the game is not translated on Android yet

### Requirement: Translate what selection
The translation screen SHALL allow selecting what text gets translated: auto (dialog unless target is English), dialog only (character speech), or all (menus and labels too, never maps), and SHALL keep it in the config file.

#### Scenario: Translate all text
- **WHEN** user sets "Translate what" to "all"
- **THEN** the choice is kept in the config file, and the screen says the game is not translated on Android yet

## REMOVED Requirements

### Requirement: Translation filter files
**Reason**: the filter files act on a running translator, which the Android frontend does not have yet.

**Migration**: the filter files are still read by the desktop program; the Android app keeps the setting once the translator is wired in.

### Requirement: Translation log file
**Reason**: the log is written by a running translator, which the Android frontend does not have yet.

**Migration**: the desktop program still writes the log; the Android app keeps the setting once the translator is wired in.

### Requirement: Translation load file
**Reason**: the loaded translations are used by a running translator, which the Android frontend does not have yet.

**Migration**: the desktop program still loads them; the Android app keeps the setting once the translator is wired in.

### Requirement: Translation storage file
**Reason**: the storage file is both loaded and appended to by a running translator, which the Android frontend does not have yet.

**Migration**: the desktop program still uses it; the Android app keeps the setting once the translator is wired in.

### Requirement: Test translation
**Reason**: there is no translator on Android to test.

**Migration**: test the translator on the desktop program, or wait for the change that wires the translator into the Android app.

### Requirement: Show translator status
**Reason**: no translator runs on Android, so there is no status to show.

**Migration**: the translator's status is shown by the desktop program.

### Requirement: Translation cache export
**Reason**: there is no translation cache on Android, as nothing is translated.

**Migration**: export the cache from the desktop program.
