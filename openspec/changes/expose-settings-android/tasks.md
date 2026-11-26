# Tasks

## 1. The settings model in Rust

- [x] 1.1 Add a `group` to `settings::Item` ("Language", "Display", "Input", "Translation", "Advanced") for every item in `ITEMS`, keeping the desktop's pages as they are; verify with `cargo test --release settings` and by opening the desktop's settings overlay, which still lists every item.
- [x] 1.2 Say per item whether it applies on Android and why not when it does not (a field on `settings::Item`, decided in Rust); verify with a unit test that the items the design's table lists as applying are the ones flagged.
- [x] 1.3 Make `Settings::save` write a temporary file and rename it over the config, and add a way to change one setting of the file (re-read, change the key, save); verify with a unit test that two changes in a row keep both, and that a `config.tmp` left over is ignored.
- [x] 1.4 Add the settings the phone's own screens need to the model (the picture switch the phone draws with, the scroll lines, the tab completion) if they are not one of the items already; verify that every item the phone shows comes from `ITEMS`.

## 2. The engine applies the settings on Android

- [x] 2.1 Give `AndroidFe` the settings: read them when the game starts, and re-read them at each input when the config file has changed; verify with a test that a changed setting is in effect at the next input, and that the file is not read again when it has not changed.
- [x] 2.2 Apply the settings the design's table lists as applying on Android in the engine: the language (`i18n::select`), the readable colours (the renderer lifts a colour too dark to read, as the terminal frontend does), the yen, the Greek/Cyrillic and ambiguous widths in the cell model, and the picture switch; verify each with a unit test where the code allows it, and with a replay of a game that prints coloured text.
- [x] 2.3 Keep the engine-level settings working on Android as they do now (the picture warmup, the EE version through `EMUERA_EE_VERSION`); verify with a test that the warmup and the EE version follow the setting.

## 3. The JNI bridge for the global settings

- [x] 3.1 Add `globalSettings()`: one TSV line per item (key, label, value, choices, live, applies on Android, group, help); verify with a unit test that the lines match `ITEMS` one for one, and that a line with a tab in it is escaped.
- [x] 3.2 Add `setGlobalSetting(key, value)`: changes one setting in the engine's config file and returns "" or what went wrong; verify with a unit test that the file holds the new value and that an unknown key is refused.

## 4. The global settings screen on the phone

- [x] 4.1 Add a settings activity listing the categories and the items `globalSettings()` gives, each with its choices as a picker, its help text, and the items that do not apply on Android marked and disabled; verify on a connected phone (`adb`) that every item of the engine appears, and that a marked item cannot be changed.
- [x] 4.2 Put the settings entry in the game list's menu and in the running game's menu, both opening the same activity; verify by opening it from each.
- [x] 4.3 Save a changed setting through `setGlobalSetting` and keep it across restarts; verify by restarting the app and checking the value is still there, and that the config file holds it in the engine's format.
- [x] 4.4 Put a setting back to its default; verify that the key is gone from the config file and the screen shows the default again.

## 5. The per-game screen

- [x] 5.1 Add the effect, the choices and the store to the line `Native.gameSettings()` returns, so the screen can show when a setting takes effect and offer Emuera's drop-down options; verify with a unit test that a choice item carries its options.
- [x] 5.2 Rewrite `GameSettings.kt` around that line: every item of Emuera's settings window, tab by tab (the `android_setting` filter goes), a widget per kind (toggle, number field, colour, text, drop-down), where the value came from, when it takes effect, and a reset per item; verify on a phone that a yes/no setting toggles, a choice opens Emuera's options, a number out of its range is refused, and an item the game fixes cannot be changed.
- [x] 5.3 Put a value back to the game's or Emuera's value, one at a time and all of the player's at once; verify that the line is taken out of the game's `emuera.config` and the value in effect is shown again.
- [x] 5.4 Give a running game its new settings at its next input (the game's menu already reloads the config); verify by changing "PRINTC items per line" in a running game and seeing it at the next input.

## 6. The settings the phone's own screens follow

- [x] 6.1 The picture switch stops the phone drawing pictures and sprites; verify on a phone that the pictures are gone and come back when the setting is put back.
- [x] 6.2 The memory in the bottom row, the scroll to the end on a key press, the scroll lines of a fling and the tab completion in the input box follow their settings; verify each on a phone.

## 7. Tests and docs

- [x] 7.1 The Rust tests for the new model functions and the JNI lines pass (`cargo test --release`), and the desktop's settings pages still work (`cargo test --release --test erb_suite`).
- [x] 7.2 `android/test-game.sh` gains a check that the settings screen opens from the game's menu, that a setting can be changed, and that a running game takes it up.
- [x] 7.3 `doc/wiki/Android-App.md` describes the two screens, what each setting does on Android (the table of the design), and that the translator's settings are kept but not applied yet; verify the page reads as the app behaves.
