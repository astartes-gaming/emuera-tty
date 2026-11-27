# Emuera-Android

[emuera-tty](https://github.com/astartes-gaming/emuera-tty) in a native Android app: the same engine, with a touch
frontend drawn by the app.

## Layout

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
| `fetch-toolchain.sh` | Gets what the build needs from Google, checked against known sums |
| `build.sh` | Builds the APK |
| `push-game.sh` | Copies a game from a PC, faster than importing it on the phone while developing |
| `test-import.sh` | Replaces throwaway games on a connected phone through the system file picker, and checks their saves survive |
| `test-game.sh` | Checks on a connected phone, with throwaway games, that touches reach the script, a game survives in the background, `CBG*` layers land where they should, crashes get reported, sound plays, text can be resized, the screen can turn, and the game's menu changes its settings, reloads the scripts, manages saves, goes back to the title and restarts |
| `test-files.sh`, `tools/paths_test.kt` | Check moving files through the file manager on a connected phone, and on a PC that paths can't lead out of the games |

## Build

Runs on Linux with no Gradle or Android Studio. `.toolchain/` needs these four things from Google,
under Google's Android SDK terms:

- `sdk/android.jar`, from `platform-35_r02.zip`
- `dl/r8-8.13.25.jar`, from Google's Maven repository (`com.android.tools:r8`)
- `ndk/android-ndk-r30/`, the target libraries from `android-ndk-r30-linux.zip`
- `sdk/build-tools/36.1.0`, for `aapt2` (the adaptive app icon and resources) and `apksigner` (signing)

Only the NDK's target libraries and `apksigner` are used; no other x86-64 programs are run. Everything comes from
`https://dl.google.com/android/`. `./fetch-toolchain.sh` downloads them (about 820 MB, mostly the NDK), checks their
SHA-256 sums and keeps only those files (about 270 MB).

For NixOS or distros compatible with `nix`:
```sh
nix shell nixpkgs#rustup nixpkgs#gcc nixpkgs#kotlin nixpkgs#jdk21 nixpkgs#apksigner \
  nixpkgs#zip nixpkgs#unzip -c ./build.sh          # add --fast --arm64-only while developing
```

For any other Linux distro:
```sh
./fetch-toolchain.sh
./build.sh
```

The APK is `build/emuera-tty-android.apk`, about 8 MB with both 64-bit and 32-bit engines. The minimum is Android 7
(API 24), set by `MIN_SDK` in `build.sh`. Android 15 and 16 have been tried.

Without `KEYSTORE` the APK is signed with a debug key that `build.sh` makes in `.toolchain/`. `KEYSTORE=<keystore>`
signs it with yours; apksigner asks for the password. Android installs an update only when it's signed with the same
key as the installed app, so keep yours safe: without it no later build can replace an installed copy.

## Install and play

```sh
adb install -r build/emuera-tty-android.apk      # -r keeps installed games and saves
```

Then open **Emuera-Android** on the phone and import a game:

- **Import a zip or tar:** zip, tar or tar.gz. It's the fast route: AnonTW takes 6 s on a POCO F1.
- **Import a folder:** for a game you've already unpacked, like one from a 7z or rar. AnonTW takes 82 s.

Importing a game that's already there replaces its files and keeps its saves: `sav/`, save files in the game's own
folder, and `dat/chara_*.dat`. A save that came with the new copy is kept beside yours, with `.from-archive` added to
its name.

In the list, ⚙ beside a game opens its settings and ✕ deletes it. The settings are those of Emuera's settings window
that do something here (autosaves, save slots, the history, infinite loop warnings, PRINTC, text colours, compressed
saves), kept in the game's `emuera.config`, which Emuera.exe reads too.

In a game, tap a button to choose it, tap anywhere to pass a wait, drag to scroll back, and use the bar at the
bottom to type. Back asks before leaving, because anything since your last in-game save is lost. Buttons are slightly
highlighted to help users know where and what parts of a button are clickable (could be removed later, as it has the
unintended side effect of revealing secret buttons, which can't be easily accounted for as they aren't any different
from normal buttons).

⚙ in the top right corner is the game's menu, as emuera-tty's settings overlay has it:

- **Game settings:** as from the list, and a running game takes them up at once.
- **Manage saves:** copy or move a save to another slot, export it to a file, delete it; back up all saves, restore a
  backup, import a save file. A save deleted, overwritten or replaced is kept in the backups first, in the app's
  `data/emuera-tty/backups/<game>/`.
- **Back to the title** and **Restart the game**, after asking. A restart starts the game in a new process.
- **Reload ERB files:** reads the scripts again, keeping the game where it is, for scripts changed in the file manager.

At a menu that reads the mouse itself (`INPUTMOUSEKEY`), a tap is a left click anywhere on the screen, a long press is
a right click, and a drag is the mouse wheel.

A game keeps running while you use other apps, with a notification to come back to it. The app asks to show that
notification, and all of this works without it. Swiping the game away in Recents ends it, like leaving with Back.

Sound plays at the phone's media volume, and pauses while the game is hidden.

A game starts at the window width it declares in its config, as in Emuera, or 120 half-width columns if it declares
none, unless that would make its text smaller than 12 sp (`MIN_TEXT_SP` in `GameView.kt`), which follows the screen and
Android's font size setting. On a POCO F1 that's 101 columns, so AnonTW's 150 and pops-tw's 192 both start larger.

A phone keeps a game in landscape. A tablet turns it with the screen, and the text follows that minimum on the new width: on
a Galaxy Tab S6 Lite that's 184 columns in landscape and 110 upright. A game may warn that its window is narrower than it
expects, and pictures printed among the text can be cut off at the sides, as upright text is larger compared to the game's
window.

**A-** and **A+** in the bottom bar change the text size, at once and for that game only, and what you choose is kept
across restarts and new versions of the game, even below that minimum. Bigger text wraps to fewer columns, so wide lines
take two rows. Menus drawn as pictures (`CBG*`) don't change. A size you've chosen stays the same share of the game's
window, so it looks smaller when a tablet is turned upright.

If a game stops with an error, the app shows what the engine logged, with a button to share it. If it dies without a
word, the app says so next time the list opens. The logs are `engine.log` in the app's files, and `engine.log.1` from the
run before.

Saves stay in the game's folder in the app's storage. `adb uninstall io.github.astartes_gaming.emuera` removes the app and
everything in it, saves included, so copy out what you want to keep first.

The games also show in the system's Files app, under **Emuera-Android**. Copy saves to or from another device there, as
Emuera reads the same files, or add to a game. Do this with the game closed, as it may be writing the same files. A name
already taken gets `(1)` added rather than replacing the file, so to replace a save, delete or rename the old one first.

## Not done yet

- Asking for audio focus: a game's sound plays alongside another app's music
- Native settings and save screens
- 7z and rar import: unpack them first and import the folder
- Tooltips and hover pictures; `<div>` boxes draw their background only, with no border or rounded corners
- Animated sprites, and the other picture a `CBG*` button sprite shows under the mouse: a touch screen has no hover
- Scripts reading the input box (`GETTEXTBOX`, `SETTEXTBOX`), or any key but the mouse buttons with `GETKEY`
- Keeping files a game writes with `SAVETEXT` under a name of its own when re-importing: they look like the game's files
- Speed work for cheap phones
