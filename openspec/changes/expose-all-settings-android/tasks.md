# Tasks

## 1. Rust JNI Bridge Extensions

- [ ] 1.1 Add `get_all_settings()` JNI function returning TSV of all settings (key, label, choices, current, default, live, help) — verify with `cargo test --release` and manual JNI call
- [ ] 1.2 Add `set_setting(key, value)` JNI function that validates and writes to config — verify setting persists in config file
- [ ] 1.3 Add `get_translation_engines()` JNI function returning available engines with descriptions — verify returns all 8 engine types
- [ ] 1.4 Add `set_translation_engine(engine)` JNI function — verify engine selection persists
- [ ] 1.5 Add `get_translation_config()` and `set_translation_config(key, value)` JNI functions — verify all translation settings readable/writable
- [ ] 1.6 Add `test_translation(text)` JNI function using current engine — verify returns translated text or error
- [ ] 1.7 Add `get_env_vars()` JNI function returning all EMUERA_* vars with descriptions — verify matches `ENV_VARS` in settings.rs
- [ ] 1.8 Add `reload_config()` JNI function (already exists as `Native.reloadConfig()`) — verify triggers game config reload

## 2. SettingsActivity — Main Settings Screen

- [ ] 2.1 Create `SettingsActivity.kt` with RecyclerView and category tabs — verify activity launches from game menu
- [ ] 2.2 Implement settings list adapter reading from `Native.getAllSettings()` — verify all 30+ settings display with correct labels
- [ ] 2.3 Implement setting value editors: spinner for choices, EditText for int/color/text, checkbox for boolean (images: disable-only) — verify each setting type edits correctly
- [ ] 2.4 Wire "Save" to call `Native.setSetting()` and show success/error — verify config file updates and toast shows
- [ ] 2.5 Implement live setting detection: call `Native.reloadConfig()` for live settings — verify game updates immediately (e.g., contrast change)
- [ ] 2.6 Add "Game settings" entry opening existing `GameSettings.kt` dialog — verify game's emuera.config settings accessible
- [ ] 2.7 Add "Translation" category entry launching `TranslationSettingsActivity` — verify navigation works
- [ ] 2.8 Add "Environment Variables" entry launching `EnvVarsActivity` — verify navigation works
- [ ] 2.9 Register `SettingsActivity` in `AndroidManifest.xml` — verify no manifest errors

## 3. TranslationSettingsActivity — Translation Configuration

- [ ] 3.1 Create `TranslationSettingsActivity.kt` with engine spinner, language pickers — verify activity launches
- [ ] 3.2 Populate engine spinner from `Native.getTranslationEngines()` — verify all available engines listed with descriptions
- [ ] 3.3 Implement target language spinner (10 languages) — verify selection persists
- [ ] 3.4 Implement source language spinner (auto, ja, en) — verify selection persists
- [ ] 3.5 Implement "Translate what" spinner (auto, dialog, all) — verify selection persists
- [ ] 3.6 Add file pickers for filter, log, load, storage paths using `ACTION_OPEN_DOCUMENT`/`ACTION_CREATE_DOCUMENT` — verify URIs convertible to paths for Rust
- [ ] 3.7 Implement "Test translation" button calling `Native.testTranslation()` — verify sample text translates and result displays
- [ ] 3.8 Show current translator description and last error from `Native.getTranslationConfig()` — verify error displays when engine fails
- [ ] 3.9 Add "Export cache" button calling new JNI `export_translation_cache(path)` — verify TSV file created with translations
- [ ] 3.10 Register `TranslationSettingsActivity` in `AndroidManifest.xml` — verify no manifest errors

## 4. EnvVarsActivity — Environment Variables Reference

- [ ] 4.1 Create `EnvVarsActivity.kt` with searchable RecyclerView — verify activity launches
- [ ] 4.2 Populate list from `Native.getEnvVars()` grouped by category — verify all ~50 EMUERA_* variables display
- [ ] 4.3 Implement search/filter by variable name or description — verify filtering works in real-time
- [ ] 4.4 Implement long-press to copy variable name to clipboard — verify toast confirms copy
- [ ] 4.5 Show current value indicator for variables set in process environment — verify EMUERA_SEED=42 shows "Current: 42"
- [ ] 4.6 Register `EnvVarsActivity` in `AndroidManifest.xml` — verify no manifest errors

## 5. Game Menu Integration

- [ ] 5.1 Add "Settings" entry to `GameActivity.showMenu()` list — verify appears in gear menu
- [ ] 5.2 Launch `SettingsActivity` from menu entry — verify settings screen opens over game
- [ ] 5.3 Ensure game continues running in background while settings open — verify game state preserved

## 6. Build and Integration Testing

- [ ] 6.1 Build Android project (`./gradlew assembleDebug`) — verify compiles without errors
- [ ] 6.2 Run on device/emulator: open settings, change each setting category — verify all settings editable
- [ ] 6.3 Test translation config: select each engine, test translation — verify engines work or show clear errors
- [ ] 6.4 Test persistence: change settings, kill app, restart — verify settings retained
- [ ] 6.5 Test live settings: run game, change contrast/disable-image-rendering/wide — verify game updates without restart
- [ ] 6.6 Test game settings (emuera.config) still work from both menus — verify no regression
- [ ] 6.7 Test env-vars screen search and copy — verify all variables accessible

## 7. Documentation

- [ ] 7.1 Update Android README with new settings screens overview — verify docs reflect new features
- [ ] 7.2 Add inline code comments for JNI functions and Kotlin activities — verify maintainability

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.