# Design

## Context

The engine has two settings models, and the phone uses neither of them properly.

- **emuera-tty's own settings** (`src/settings.rs`): 25 items, each a key with a list of choices, saved as `key=value` lines in `$XDG_CONFIG_HOME/emuera-tty/config`. On Android the engine sets `XDG_CONFIG_HOME` to `<files>/cfg`, so the file is `<files>/cfg/emuera-tty/config`. `main.rs` applies them in the terminal frontend's branch only (`t.contrast = …`, `t.yen_mode = …`, `t.set_translate(…)`); the Android frontend takes the `external` branch, so on the phone only the language is read (`i18n::select` in `android.rs`). `Settings::export_env` (the engine-level ones: `EMUERA_AMBIGUOUS`, `EMUERA_COLORS`, `EMUERA_WARMUP`, `EMUERA_EE_VERSION`, `EMUERA_MOUSE`) and the picture warmup do run on Android, because they are before the branch.
- **Emuera's settings window** (`src/emuconfig.rs`): 93 items with a kind (bool, int, colour, text, choice), a tab, a default, an effect (now / at the next start / only Emuera.exe) and a store (`emuera.config` or Emuera's `setting.json`). `GameSettings.kt` shows a filtered subset of it (`android_setting()`), with a plain text field where Emuera has a drop-down, and nothing about where a value came from or when it takes effect.

Two processes write to the app's files: the game list and the running game (its own `:game` process, kept alive by a foreground service).

The `add-emuera-settings-parity` change (in progress) owns the `emuconfig` model; this change only reads it, and does not change the config file formats.

## Goals / Non-Goals

**Goals:** one settings model per kind of setting, read by the app from the engine and never copied into Kotlin; every setting visible on the phone, with the ones Android cannot use marked; the settings that do apply reaching the running game at its next input; the config file shared with the desktop program, written safely by two processes.

**Non-Goals:** the app's own text in other languages (the phone's chrome stays English); the translator running on Android (the settings are kept, the translator is not wired); the environment variables reference screen (`android/env-vars`, its own capability); new settings or a new config file format; a Jetpack Compose rewrite.

## Decisions

### The models stay in Rust; the app reads them

The global settings gain a `group` field ("Language", "Display", "Input", "Translation", "Advanced") next to their key, label, choices and help: the categories are the engine's business, and the desktop's settings page can use the same groups later. The per-game settings already carry everything the screen needs (`Kind`, `Effect`, `Store`, `TABS`, `Source`).

*Alternative:* grouping the global settings in Kotlin — rejected, the list would drift from `ITEMS` the moment an item is added.

### Two batch getters and two setters over JNI

`globalSettings()` returns one TSV line per item (key, label, value, choices, live, applies on Android, group, help); `setGlobalSetting(key, value)` changes one and returns "" or the error. `gameSettings(game)` gains the effect, the choices of a `Kind::Choice` item and the store; `setGameSetting(game, key, value)` stays as it is.

*Alternatives:* one JNI function per setting (60-odd, all to be kept in step) — rejected; JSON over JNI (a JSON writer in Rust for no gain) — rejected; SharedPreferences on the phone — rejected, the desktop program would not read it.

### The config file is the engine's own, and the app never parses it

The file stays `<files>/cfg/emuera-tty/config`, in the format `settings.rs` writes. The app reads and writes it through Rust (`Settings::load`, `Settings::set`, `Settings::save`), so the phone and the desktop program agree, and a value the desktop program wrote is seen by the phone.

### Two processes, one file: read-modify-write, atomic save

`setGlobalSetting` re-reads the file, changes the one key and writes it back, so a change made in the game process is not lost by a later change in the list process. `Settings::save` writes a temporary file and renames it over the config, so a crash mid-write cannot leave a half-written file (the desktop benefits too).

The player interacts with one process at a time (the game's screen is in front, or the list's), so two writes do not overlap; the re-read closes the window where the second writer holds a stale copy.

### A running game takes the new settings at its next input

The Android frontend re-reads the config when it asks for input — a stat first, a read only when the file changed — and applies what applies. This is the same moment the game's own settings take effect (`Hotkey::Config` → `Vm::reload_config`), and it works whether the settings were changed in the game process or in the list process.

*Alternative:* the app tells the running game over a broadcast or a marker file — rejected, the game may be in another process and idle, and the input comes anyway.

### Every setting is shown; the ones Android cannot use are marked

The rule the desktop's game settings page already follows (`Effect::ExeOnly` → "Only Emuera.exe uses this"), extended to the global settings: each item says whether it applies on Android and why not when it does not. The decision lives in Rust, next to the item, not in Kotlin.

*Alternative:* hiding the settings that do not apply — rejected: the player would not know the setting exists, and could not set it for the desktop program from the phone.

What applies on Android, and how:

| Setting | On Android | How |
|---|---|---|
| language | yes | the engine's messages switch language; the app's own text stays English |
| images | partly | only "none" means anything: the app stops drawing pictures; the terminal protocols do not apply |
| colors | no | the colour depth sent to a terminal |
| contrast | yes | the Android renderer lifts a colour too dark to read, as the terminal frontend does |
| wide | yes | the app's cell width for ambiguous characters follows the setting |
| yen | yes | the app draws `\` as `¥` |
| letters | yes | Greek and Cyrillic take two cells in the app's layout |
| clipboard | no | OSC 52 in a terminal; the phone has its own clipboard, not wired to this |
| statusbar | no | it shows the hovered button's value; a touch screen has no hover |
| memory | yes | the memory in the bottom row |
| crashlog | no | the app reports crashes itself (engine.log) |
| translator, translate, translate_from, translate_what | not yet | kept and marked; the translator is not wired into the Android frontend |
| sound | no | the app plays sound with MediaPlayer, not with mpv or ffplay |
| kbdpointer | no | there is no pointer on a touch screen |
| gamepad | no | the app does not read gamepads |
| wheel | partly | the app's fling scrolls that many lines |
| tab_complete | yes | the app's input box completes to a button's value, from the buttons it already knows |
| scroll_end_on_key | yes | typing while scrolled back jumps to the end |
| warmup | yes | already: the engine decodes the last used pictures while the game loads, as the setting says |
| mouse | no | gpm and /dev/input/mice on the Linux console |
| frontend | no | ANSI and retro output for a terminal |
| ee_version | yes | the engine reads it while the game loads (EMUERA_EE_VERSION) |

### The language changes the engine's messages; the app's own text stays English

The phone's strings are hardcoded English in Kotlin; a translation layer for them is a separate change. The settings screen says what the language changes, so the player is not surprised by a game in Japanese and a menu in English.

### The translator's settings are kept, the translator is not wired

The translation screen stores the engine, the languages and what to translate, and says that the game is not translated on Android yet. Wiring `mt.rs` into the Android frontend (translating blocks, polling, re-laying out the lines as translations arrive) is a separate change; the settings are in place for it.

### The per-game screen is a renewal of `GameSettings.kt`

One dialog, built from `Native.gameSettings()`: a widget per kind (toggle, number field, colour field, text field, drop-down for a choice), where the value came from, when it takes effect, and a reset per item (plus the existing "the game's defaults" for all of them). The choice items need their options in the line, so the Rust side sends them.

## Risks / Trade-offs

- [The phone writes values the desktop program may not expect] → the file format and the keys are unchanged, and only the settings the phone offers are written; the desktop program reads the same values.
- [A setting that looks as if it works but does not] → every item says whether it applies on Android; the translator says so in its own screen.
- [Reading the config at every input] → the file is a few kilobytes; a stat first, a read only when it changed.
- [Two processes writing the config] → read-modify-write in one call, and an atomic save; the player writes from one process at a time.
- [The per-game screen shows 93 items, many of them Emuera-only] → they are grouped by Emuera's tabs and marked with the reason, as the desktop's page does.
- [The app's chrome stays English while the game speaks another language] → the settings screen says so; translating the chrome is a separate change.

## Migration Plan

No migration: the config file's format and the game's `emuera.config` are untouched, and the settings that already worked on the phone keep working.

Rollback: the new activity and the JNI functions can be removed; the values stay in the config file, where the desktop program reads them as it did before.

## Open Questions

- Whether the desktop's launcher settings page adopts the same groups as the phone's (a later change, no effect on this one).
- Whether the phone's per-game text size (a share of the game's window) becomes an emuera-tty setting of its own; today it is Android-only, and Emuera's own font size means something else.
