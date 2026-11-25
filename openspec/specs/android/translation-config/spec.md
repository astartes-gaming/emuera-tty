# android/translation-config Specification

## Purpose

Provides a translation configuration screen in the Android app that lets users choose and configure the inline translation engine (Ollama, LibreTranslate, Bergamot, Argos, translate-shell, DeepL, custom command, or file-based), target language, source language, what to translate, and translation file paths.

## Requirements

### Requirement: Translation engine selection
The translation screen SHALL display all available translation engines and allow selecting one.

#### Scenario: Select Ollama as translator
- **WHEN** user chooses "Ollama" from the translator list
- **THEN** the app uses Ollama at localhost:11434 for translations

#### Scenario: Select translate-shell (online)
- **WHEN** user chooses "translate-shell" and the tool is installed
- **THEN** the app uses translate-shell with Google engine by default

#### Scenario: Select DeepL with API key
- **WHEN** user chooses "DeepL" and DEEPL_AUTH_KEY is set
- **THEN** the app uses DeepL's official API

#### Scenario: Select file-based translation only
- **WHEN** user chooses "file" (no translator)
- **THEN** only translations from loaded files are shown, no live translation occurs

### Requirement: Translate to language selection
The translation screen SHALL allow selecting the target language from: English, Spanish, Portuguese, French, German, Italian, Russian, Japanese, Chinese, Korean.

#### Scenario: Change target language to Spanish
- **WHEN** user sets "Translate to" to "es"
- **THEN** all translated text appears in Spanish

### Requirement: Translate from language selection
The translation screen SHALL allow selecting the source language detection mode: auto (detect Japanese/English), Japanese only, English only.

#### Scenario: Set source to Japanese only
- **WHEN** user sets "Translate from" to "ja"
- **THEN** only Japanese text is translated, English text is shown as-is

### Requirement: Translate what selection
The translation screen SHALL allow selecting what text gets translated: auto (dialog unless target is English), dialog only (character speech), or all (menus and labels too, never maps).

#### Scenario: Translate all text
- **WHEN** user sets "Translate what" to "all"
- **THEN** menus, labels, and dialog are all translated

### Requirement: Translation filter files
The translation screen SHALL allow specifying filter files (via --translation-filter) that exclude specific script files or text lines from translation.

#### Scenario: Add translation filter file
- **WHEN** user adds a filter file path
- **THEN** lines from listed scripts or matching listed text are not translated

### Requirement: Translation log file
The translation screen SHALL allow setting a log file path (--translate-log) where translations are appended with metadata.

#### Scenario: Enable translation logging
- **WHEN** user sets a translation log path
- **THEN** each translation is written to the log with source, target, engine, and location

### Requirement: Translation load file
The translation screen SHALL allow setting a load file path (--translate-load) to pre-load translations from a previous log.

#### Scenario: Load translations from file
- **WHEN** user sets a translation load path
- **THEN** translations from that file are used immediately without calling a translator

### Requirement: Translation storage file
The translation screen SHALL allow setting a storage file path (--translate-storage) that serves as both load and log.

#### Scenario: Use single file for load and log
- **WHEN** user sets a translation storage path
- **THEN** translations are loaded from and appended to the same file

### Requirement: Test translation
The translation screen SHALL provide a test button that translates a sample line using the current engine and settings.

#### Scenario: Test translation works
- **WHEN** user taps "Test translation" with Ollama configured
- **THEN** a sample Japanese line is translated and the result shown

### Requirement: Show translator status
The translation screen SHALL display the currently active translator description and any last error.

#### Scenario: Show translator error
- **WHEN** the translator fails (e.g., Ollama not running)
- **THEN** the error message is displayed in the translation screen

### Requirement: Translation cache export
The translation screen SHALL provide an option to export the current translation cache (loaded + session translations) to a file.

#### Scenario: Export translation cache
- **WHEN** user taps "Export cache"
- **THEN** a file is created with all translations in log format
