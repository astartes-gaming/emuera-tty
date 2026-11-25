# Proposal

## Why

The Android app gives the player almost no control over the engine: emuera-tty's own settings (readable colours, the target EE version, the translator, warmup, …) are read from `<files>/cfg/emuera-tty/config` but nothing on the phone shows or changes them, and the game's own `emuera.config` screen shows only the items emuera-tty happens to use, with a text field where Emuera has a drop-down. The desktop has both (the settings overlay and Emuera's settings window); the phone should have them too, with the same rule the desktop follows: every setting is shown, and the ones this frontend cannot use say so.

## What Changes

- **A global settings screen** in the Android app (the list's menu and the running game's menu), listing every item of `settings::ITEMS` with its choices, current value, help text, and whether it applies to Android. Live ones (colour contrast, the yen, wide symbols, Greek/Cyrillic width, memory in the bottom row, scroll-to-end) reach the running game at its next input; the rest are marked as taking effect at the next start.
- **The engine honours those settings on Android.** Today `main.rs` applies emuera-tty's settings only in the terminal frontend's branch, so on the phone the file is read for the language alone. The Android frontend gets the same settings and applies the ones that mean something on a touch screen; the ones that cannot (terminal colour depth, the console mouse, ANSI/retro output, the gpm mouse) are shown as such.
- **The per-game screen renews `GameSettings.kt`**: every item of Emuera's settings window, tab by tab, with a widget per kind (toggle, number field, colour, text, drop-down), where the value comes from (Emuera's default, the game, the player, or locked by `_fixed.config`), when it takes effect (now, at the next start, or only Emuera.exe uses it), and a per-item reset.
- **The translator's settings are shown and saved**, marked as not yet applied on Android: wiring `mt.rs` into the Android frontend's layout is a separate change.
- **The config file is written safely.** The list process and the game process both write it; the write is atomic and changes one key at a time, so neither can lose the other's changes.
- The app's own strings stay English: the "language" setting changes the engine's messages (load warnings, script errors, emuera-tty's own menus), not the phone's chrome.

## Capabilities

### New Capabilities

- `android/game-settings`: the per-game screen over Emuera's settings window (the game's `emuera.config`): every item, its kind, where its value comes from, and when it takes effect.

### Modified Capabilities

- `android/settings-ui`: the global settings screen is renewed: every item of `settings::ITEMS` (the list in the spec is out of date), grouped by category from Rust rather than hardcoded in Kotlin, with the items that do not apply on Android marked instead of hidden, and the per-game screen moved to `android/game-settings`.
- `android/translation-config`: the translation settings are saved and shown, but marked as not yet applied on Android, and the screen no longer promises that a chosen engine translates the game.

## Impact

- `src/settings.rs`: a group per item, an atomic write, and a way to read and change one setting from outside.
- `src/android.rs`: the JNI bridge for the global settings (`globalSettings`, `setGlobalSetting`), the Android frontend applying them, and the per-game line format gaining the effect, the choices and the store.
- `src/tty.rs`, `src/picker.rs`: the settings pages use the new group field (no behaviour change).
- `android/kotlin/`: a new settings activity, a rewritten `GameSettings.kt`, the list and game menus, and the bottom bar for the memory and scroll settings.
- Docs: `doc/wiki/Android-App.md` (the settings section, the settings' effects on Android, the translator's not-yet state).
- Saves are unaffected; the config file's format is unchanged.
