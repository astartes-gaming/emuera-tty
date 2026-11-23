## Android App

**Emuera-Android** is a native Android app that embeds emuera-tty's engine. The same Rust engine runs on a background thread, and a Kotlin UI draws it on a canvas. The app is not on Google Play; you build the APK and install it with `adb`.

More or less the unofficial successor of **uEmuera**, which hasn't had any updates as far back as 2023, and can no longer run some popular Era games. (https://github.com/xerysherry/uEmuera)

**Work in progress.** This is not a complete implementation. Some features are still missing and some TUI behavior is slightly finicky. Read below for extra details.

> **OpenSpec tracking:** This app's settings exposure is tracked in the `expose-all-settings-android` change ([design](https://github.com/astartes-gaming/emuera-tty/tree/master/openspec/changes/expose-all-settings-android/design.md), [tasks](https://github.com/astartes-gaming/emuera-tty/tree/master/openspec/changes/expose-all-settings-android/tasks.md)). The sections below note what's implemented, what's planned in that change, and what's intentionally out of scope.

### What works (implemented)

- The full engine: every command, function, save format, and game flow that the desktop build has.
- Saves move both ways between the app and Emuera.exe on Windows.
- Pictures: sprites, `G*` buffers, `CBG*` layers, `<div>` boxes (background only, no border or rounded corners yet).
- Sound: SE and BGM through Android's `MediaPlayer`, paused when the game is hidden.
- Text resize: **A-** / **A+** in the bottom bar changes the text size instantly; the choice is kept across restarts and game updates.
- Game settings (`emuera.config`): the same keys Emuera uses, from the list or the game's menu (`GameSettings.kt`).
- Save manager: copy/move slots, export/import files, back up all saves, restore a backup.
- Running in the background: a foreground service keeps the game alive with a notification to come back.
- Archive import: zip, tar, tar.gz (7z and rar not yet — unpack them first and import the folder).
- Files app integration: games appear under **Emuera-Android** in the system's file manager; copy saves to another device or add files to a game there.
- Tablet rotation: the text follows the minimum size on the new width; pictures among text can be cut off when upright.
- Crash reports: the engine's log (`engine.log` and `engine.log.1`) is shown with a share button.

### What's in the `expose-all-settings-android` change (planned, not yet implemented)

The OpenSpec change `expose-all-settings-android` adds a complete settings UI for the 30+ global emuera-tty settings (from `src/settings.rs`) and translation configuration. It does **not** change the Rust settings system or add new settings — it only exposes what's already there.

**Planned additions (from the change's tasks):**

- **`SettingsActivity`** — main settings screen with categorized list (Language, Input, Translation, Advanced), reading all settings from Rust via a batch JNI getter (`getAllSettings()` returning TSV).
- **`TranslationSettingsActivity`** — translation engine selection (all 8 engines from `mt.rs`: Ollama, LibreTranslate, Bergamot, Argos, Command, Translate-shell, DeepL curl, DeepL CLI), target/source language pickers, "translate what" (auto/dialog/all), file pickers for filter/log/load/storage paths, and a "Test translation" button.
- **`EnvVarsActivity`** — searchable reference of all ~50 `EMUERA_*` environment variables with descriptions and current values.
- **JNI bridge extensions** — `getAllSettings()`, `setSetting()`, `getTranslationEngines()`, `setTranslationEngine()`, `getTranslationConfig()`, `setTranslationConfig()`, `testTranslation()`, `getEnvVars()`, `reloadConfig()`.
- **Game menu integration** — "Settings" entry in the gear menu opening `SettingsActivity`, with the game continuing in the background.
- **Live settings** — changes to live settings (e.g., contrast, wide, disable-image-rendering) apply immediately via `reloadConfig()`; non-live settings take effect on next game start.
- **Config persistence** — uses the same `$XDG_CONFIG_HOME/emuera-tty/config` path logic as desktop (resolves to app's internal data directory on Android).

**How the categories map (from design decision #3):**

| Category | Settings (from Rust `ITEMS`) |
|---|---|
| Language | language, **images: boolean disable-only**, contrast, wide, yen, letters |
| Input | clipboard, statusbar, memory, kbdpointer, gamepad, wheel, tab_complete, scroll_end_on_key |
| Translation | translator, translate, translate_from, translate_what — opens `TranslationSettingsActivity` |
| Advanced | warmup, mouse, frontend, ee_version |

> **Note on "images":** The Rust setting has 6 protocol choices (auto, kitty, iterm, sixel, fb, none). Android uses a fixed Canvas backend, so the UI exposes only a boolean "Disable image rendering" mapping to "none". Other protocols are not applicable on Android.

### What's missing (engine features not yet on Android)

- Audio focus: a game's sound plays alongside another app's music.
- Native settings and save screens (they go through the engine's model — the OpenSpec change adds the settings UI, but the save manager screen stays engine-driven).
- 7z and rar import.
- Showing a game's real title in the list (shows the folder name).
- Tooltips and hover pictures; `<div>` boxes draw background only.
- Animated sprites, and the hover picture for `CBG*` button sprites.
- Scripts reading the input box (`GETTEXTBOX`, `SETTEXTBOX`) or keys other than mouse buttons with `GETKEY`.
- Keeping `SAVETEXT` files under their own names when re-importing (they look like the game's files).
- Speed work for cheap phones.

### Intentionally out of scope (non-goals from the OpenSpec change)

- Implement new translation engines (uses existing ones in `mt.rs`).
- Change the Rust settings system or config file format.
- Add settings not already in `src/settings.rs` `ITEMS`.
- Implement a new config storage backend.
- Jetpack Compose rewrite (current codebase uses Views; `SettingsActivity` uses custom `RecyclerView`).
- Individual JNI getter/setter per setting (batch TSV getter avoids 60+ JNI functions).
- Duplicating translation engine discovery logic in Kotlin (stays in Rust via `getTranslationEngines()`).
- Game-specific `emuera.config` settings in the global settings UI (they stay in `GameSettings.kt`, accessible from both the list and the new main settings screen).

### Install

```sh
adb install -r build/erb-player.apk
```

The APK is built by `android/build.sh` (see [Building](#building)). `-r` keeps installed games and saves.

### Playing

Open **Emuera-Android** on the phone and import a game:

- **Import a zip or tar:** zip, tar, or tar.gz. Fast: AnonTW takes ~6 s on a POCO F1 (Snapdragon 845).
- **Import a folder:** for a game you've already unpacked (e.g., from 7z or rar). Slower: AnonTW takes ~82 s.

Importing a game that's already there replaces its files and keeps its saves: `sav/`, save files in the game's folder, and `dat/chara_*.dat`. A save that came with the new copy is kept beside yours with `.from-archive` added to its name.

In the list, **⚙** beside a game opens its settings and **✕** deletes it. The settings are those of Emuera's settings window that do something here (autosaves, save slots, history, infinite loop warnings, PRINTC, text colours, compressed saves), kept in the game's `emuera.config`, which Emuera.exe reads too.

In a game:

- Tap a button to choose it.
- Tap anywhere to pass a wait.
- Drag to scroll back.
- Use the bar at the bottom to type.
- **Back** asks before leaving (anything since your last in-game save is lost).
- Buttons are slightly highlighted to show what's clickable (reveals secret buttons — may be removed later).

**⚙** in the top right corner is the game's menu:

- **Game settings:** same as from the list; a running game takes them up at once.
- **Manage saves:** copy/move a save to another slot, export to a file, delete it; back up all saves, restore a backup, import a save file. A save deleted, overwritten, or replaced is kept in the backups first, in the app's `data/emuera-tty/backups/<game>/`.
- **Back to the title** and **Restart the game**, after asking. A restart starts the game in a new process.
- **Reload ERB files:** reads the scripts again, keeping the game where it is (for scripts changed in the file manager).
- *(Planned)* **Settings:** opens the new global `SettingsActivity` (from the `expose-all-settings-android` change).

At a menu that reads the mouse itself (`INPUTMOUSEKEY`):

- Tap = left click anywhere on the screen.
- Long press = right click.
- Drag = mouse wheel.

A game keeps running while you use other apps, with a notification to come back. The app asks to show that notification, and all of this works without it. Swiping the game away in Recents ends it, like leaving with Back.

Sound plays at the phone's media volume and pauses while the game is hidden.

A game starts at the window width it declares in its config, as in Emuera, or 120 half-width columns if it declares none, unless that would make its text smaller than 12 sp (`MIN_TEXT_SP` in `GameView.kt`), which follows the screen and Android's font size setting. On a POCO F1 that's 101 columns, so AnonTW's 150 and pops-tw's 192 both start larger.

A phone keeps a game in landscape. A tablet turns it with the screen, and the text follows that minimum on the new width: on a Galaxy Tab S6 Lite that's 184 columns in landscape and 110 upright. A game may warn that its window is narrower than it expects, and pictures printed among the text can be cut off at the sides, as upright text is larger compared to the game's window.

**A-** and **A+** in the bottom bar change the text size, at once and for that game only, and what you choose is kept across restarts and new versions of the game, even below that minimum. Bigger text wraps to fewer columns, so wide lines take two rows. Menus drawn as pictures (`CBG*`) don't change. A size you've chosen stays the same share of the game's window, so it looks smaller when a tablet is turned upright.

If a game stops with an error, the app shows what the engine logged, with a button to share it. If it dies without a word, the app says so next time the list opens. The logs are `engine.log` in the app's files, and `engine.log.1` from the run before.

Saves stay in the game's folder in the app's storage. `adb uninstall io.github.astartes_gaming.emuera` removes the app and everything in it, saves included — copy out what you want to keep first.

The games also show in the system's Files app, under **Emuera-Android**. Copy saves to or from another device there (Emuera reads the same files), or add to a game. Do this with the game closed, as it may be writing the same files. A name already taken gets `(1)` added rather than replacing the file, so to replace a save, delete or rename the old one first.

### Building

The build runs on ARM Linux with no Gradle or Android Studio. `.toolchain/` needs four things from Google, under Google's Android SDK terms:

- `sdk/android.jar`, from `platform-35_r02.zip`
- `dl/r8-8.13.25.jar`, from Google's Maven repository (`com.android.tools:r8`)
- `ndk/android-ndk-r30/`, the target libraries from `android-ndk-r30-linux.zip`
- `sdk/build-tools/36.1.0`, for APK signing with the packaged `apksigner`

Only the NDK's target libraries and `apksigner` are used; no other x86-64 programs are run. Everything comes from `https://dl.google.com/android/`.

```sh
./fetch-toolchain.sh    # downloads ~820 MB, checks SHA-256, keeps ~270 MB
./build.sh
```

For NixOS or distros compatible with `nix`:

```sh
nix shell nixpkgs#rustup nixpkgs#gcc nixpkgs#kotlin nixpkgs#jdk21 nixpkgs#apksigner \
  nixpkgs#zip nixpkgs#unzip -c ./build.sh          # add --fast --arm64-only while developing
```

The APK is `build/erb-player.apk`, about 6 MB with both 64-bit and 32-bit engines. The minimum is Android 7 (API 24), set by `MIN_SDK` in `build.sh`. Android 15 and 16 have been tried.

Without `KEYSTORE` the APK is signed with a debug key that `build.sh` makes in `.toolchain/`. `KEYSTORE=<keystore>` signs it with yours; apksigner asks for the password. Android installs an update only when it's signed with the same key as the installed app, so keep yours safe: without it no later build can replace an installed copy.

### Architecture

| Path | What it is |
|---|---|
| `../src/android.rs` | The Android frontend and replay mode |
| `kotlin/MainActivity.kt` | The game list, and importing games through the system's file picker |
| `kotlin/GameActivity.kt`, `GameView.kt` | A running game, in its own process: a custom view that draws rows of cells on a canvas |
| `kotlin/GameSettings.kt` | A game's settings (its `emuera.config`), from the list or the game's menu, through the engine's own model of them |
| `kotlin/Saves.kt` | The game's menu's save manager, through emuera-tty's own (`savemgr.rs`) |
| `kotlin/GameService.kt` | The foreground service that keeps a game alive while another app is in front |
| `kotlin/GameAudio.kt` | The game's sound and music, with Android's `MediaPlayer` |
| `kotlin/GamesProvider.kt`, `Paths.kt` | The games in the system's file manager, and the rule that keeps its paths inside them |
| `kotlin/Crash.kt` | Reporting a game that stopped: what the engine logged, with a way to share it |
| `tools/mkmanifest.rs` | Writes the binary manifest, in place of `aapt2` |
| *(planned)* `kotlin/SettingsActivity.kt` | Main settings screen (Language, Input, Translation, Advanced) |
| *(planned)* `kotlin/TranslationSettingsActivity.kt` | Translation engine and parameters |
| *(planned)* `kotlin/EnvVarsActivity.kt` | Environment variables reference |
| *(planned)* `src/jni.rs` | Rust JNI bridge extensions for settings/translation/env-vars |

The engine runs on a thread with a 64 MB stack (`AndroidFe` in `android.rs`). It blocks in `Frontend::input`, so it never touches the UI directly. Each update lays lines out into rows of cells and sends them as one byte array, which Kotlin parses and draws. Answers come back through a channel.

Replay mode plays a recorded session with no player. It computes the same output hash as the headless reference (`bench/erbbench`), so a run on the real Android allocator with real drawing can be checked byte for byte against the headless reference.

### Configuration

The engine's settings that do something on Android are those whose tab is not "Clipboard" and whose key is not "キーボードマクロを使用する" (use keyboard macros), "ボタンの途中で行を折りかえさない" (don't wrap lines in the middle of a button), or "フォント名" (font name for text drawn into pictures — found with `fc-match`, which Android doesn't have). See `android_setting` in `android.rs`.

Environment variables the Android build sets by default (in `Java_..._start`):

| Variable | Small device (≤3 GB RAM) | Larger device |
|---|---|---|
| `EMUERA_CODE_MB` | 24 | 48 |
| `EMUERA_GBUF_MB` | 32 | 64 |
| `EMUERA_PICTURES_MB` | 32 | 64 |
| `EMUERA_CUTS_MB` | 16 | 32 |
| `EMUERA_APP_IMAGE_MB` | 16 | 32 |

Phone-sized limits unless told otherwise. The engine's defaults scale with RAM and rarely release anything on a phone, while tight ones cost no measurable speed.

Extra settings passed from Kotlin (`env_list`):

- `AFFINITY=hexmask`: pins the whole process (UI and render threads included) to those cores — a stand-in for a phone that only has slow ones.
- `EVICT=1`: drops the game's files from the page cache before loading, so a load reads the storage as it would after a reboot.
- `STEPS=n`: ends a replay early (for testing).
- `PANIC=1`: panics at the start, to test the crash report.

### Testing

The `android/` folder has several test scripts that run on a connected phone with throwaway games:

- `test-game.sh`: checks that touches reach the script, a game survives in the background, `CBG*` layers land where they should, crashes get reported, sound plays, text can be resized, the screen can turn, and the game's menu changes its settings, reloads the scripts, manages saves, goes back to the title and restarts.
- `test-import.sh`: replaces throwaway games through the system file picker and checks their saves survive.
- `test-files.sh`, `tools/paths_test.kt`: check moving files through the file manager on a connected phone, and on a PC that paths can't lead out of the games.
- `push-game.sh`: copies a game from a PC, faster than importing it on the phone while developing.

### Tips

- **Memory:** the engine logs its timing and memory to `engine.log` in the app's files. `EMUERA_TIMING=1` is set by default.
- **Crash reports:** if a game stops with an error, the app shows the engine's log with a share button. If it dies silently, the app says so next time the list opens. `engine.log.1` is the previous run's log.
- **Saves:** `adb uninstall io.github.astartes_gaming.emuera` removes everything. Copy saves out first if you want to keep them.
- **Files app:** games appear under **Emuera-Android**. Do file operations with the game closed.
- **Text size:** the **A-**/**A+** buttons change the text size instantly. Your choice is saved per game and survives updates.
- **Tablets:** the game turns with the screen. The text follows the minimum size on the new width; pictures among text can be cut off when upright.