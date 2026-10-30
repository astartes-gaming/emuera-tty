# Spec Delta

## Purpose

Shows emuera-tty's own interface (settings, launcher, save manager, prompts,
its own messages) in the player's language, and lets translators add a
language without changing code.

## ADDED Requirements

### Requirement: Interface text is looked up in the selected language

Every piece of text emuera-tty itself shows SHALL be looked up in the
catalogue of the selected language, and SHALL be shown in English when the
catalogue has no translation for it. Text coming from the game (its prints,
CSV names, Emuera-Anchor translations) SHALL NOT go through this lookup.

#### Scenario: A translated message

- **WHEN** the language is Japanese and the Japanese catalogue translates "Restart the game"
- **THEN** the settings overlay shows the Japanese text in its place

#### Scenario: An untranslated message falls back to English

- **WHEN** the language is Korean and the Korean catalogue has no entry for a message
- **THEN** that message is shown in English, and nothing else changes

#### Scenario: Game text is left alone

- **WHEN** a game prints a line whose text equals an English interface message
- **THEN** the line is printed exactly as the game wrote it

### Requirement: Values are inserted into messages by name

A message that includes values (a slot number, a file name, a count) SHALL
mark each value with a name, so a translation can put the values in any
order.

#### Scenario: A translation reorders the values

- **WHEN** a translation puts `{file}` before `{slot}` where the English has them the other way round
- **THEN** the shown text has each value in the place the translation gives it

### Requirement: The language is chosen by a setting or the system locale

A **Language** setting SHALL offer `auto`, English, Japanese, Simplified
Chinese, Traditional Chinese and Korean, each named in its own language. With
`auto`, the language SHALL follow the system locale, falling back to English
when the locale names none of them.

#### Scenario: auto follows the locale

- **WHEN** Language is `auto` and the locale is `ja_JP.UTF-8`
- **THEN** the interface is in Japanese

#### Scenario: Chinese locales pick the right script

- **WHEN** Language is `auto` and the locale is `zh_TW` or `zh_HK`
- **THEN** Traditional Chinese is used; for `zh_CN` or `zh_SG`, Simplified Chinese

#### Scenario: An unsupported locale

- **WHEN** Language is `auto` and the locale is `de_DE.UTF-8`
- **THEN** the interface is in English

#### Scenario: The setting wins over the locale

- **WHEN** Language is set to Korean and the locale is Japanese
- **THEN** the interface is in Korean

### Requirement: A language change applies at once

Changing the Language setting SHALL change the interface text from the next
screen drawn, without a restart.

#### Scenario: Switching in the settings overlay

- **WHEN** the player changes Language from English to Japanese in the overlay
- **THEN** the overlay redraws in Japanese

### Requirement: Translators work from standard catalogue files

Each language's translations SHALL live in a gettext `.po` file in the
repository, and a template SHALL list every interface message. A message
added to the code without being in the template SHALL fail the test suite.

#### Scenario: A new message without a template entry

- **WHEN** a developer adds an interface message and doesn't update the template
- **THEN** `cargo test` fails and names the missing message

#### Scenario: A translator adds a language's translations

- **WHEN** a translator fills in `ja.po` and emuera-tty is rebuilt
- **THEN** the Japanese interface uses those translations, with no code changed
