# Proposal

## Why

The Android app for emuera-tty currently exposes only a small subset of emuera-tty's settings and features. The in-game menu offers just 5 options (Game settings, Manage saves, Back to title, Restart game, Reload ERB files), and the game settings dialog only shows the game's own `emuera.config` settings. Critical features like the inline translator (with multiple engine choices), display settings (images, colors, contrast, wide symbols), input settings (keyboard pointer, gamepad, tab completion), and many others are completely unavailable on Android. This forces Android users to either use environment variables (which are awkward on Android) or miss out on functionality that desktop users have.

## What Changes

- Add a comprehensive **Settings** screen accessible from the game menu that exposes all emuera-tty settings (from `src/settings.rs` `ITEMS`), with the "images" protocol selector simplified to a boolean "Disable image rendering" toggle (Android uses a fixed rendering backend)
- Add a **Translation** sub-screen for configuring the inline translator (engine, target language, source language, what to translate, translation filter files, translation log/load/storage paths)
- Expose the **game's Emuera settings** (from `src/emuconfig.rs` `ITEMS`) - already partially done via GameSettings.kt but needs better organization
- Add **Environment Variables** page for advanced users (from `src/settings.rs` `ENV_VARS`)
- Wire up the Rust settings to the Android UI through JNI (`Native` object)
- Ensure settings persist across sessions via the existing config file mechanism

## Capabilities

### New Capabilities

- `android/settings-ui`: Full settings screen exposing all emuera-tty configuration options (language, display, input, translation, advanced)
- `android/translation-config`: Translation engine selection and configuration (Ollama, LibreTranslate, Bergamot, Argos, translate-shell, DeepL, custom command, file-based)
- `android/env-vars`: Environment variables reference page for advanced configuration

### Modified Capabilities

None - this is a new Android UI feature with no existing spec.

## Impact

- **Android Kotlin files**: `GameActivity.kt`, `GameSettings.kt`, new `SettingsActivity.kt`, new `TranslationSettings.kt`, new `EnvVarsActivity.kt`
- **JNI bridge**: `Native.kt` needs new external functions for reading/writing all settings
- **Rust side**: `src/settings.rs` already has all settings defined; `src/mt.rs` has translation engines; need to expose getter/setter via JNI
- **Build**: No new dependencies; uses existing config file mechanism