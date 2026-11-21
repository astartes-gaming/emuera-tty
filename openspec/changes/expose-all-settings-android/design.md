# Design

## Context

The Android app (`android/kotlin/`) currently has minimal settings exposure:
- `GameActivity.showMenu()` offers 5 items: Game settings, Manage saves, Back to title, Restart game, Reload ERB files
- `GameSettings.kt` shows only the game's `emuera.config` settings (from `emuconfig.rs` ITEMS)
- `Native.kt` JNI bridge has functions for game settings, saves, import, and basic engine control
- The Rust side (`src/settings.rs`, `src/mt.rs`, `src/emuconfig.rs`) has all settings defined but not exposed to Android

The emuera-tty config file (`~/.config/emuera-tty/config`) stores user settings as `key=value` lines. The `Settings` struct in `settings.rs` manages 30+ settings across categories: Language, Display, Input, Translation, Advanced.

Translation engines are defined in `mt.rs` `Engine` enum: Ollama, LibreTranslate, Bergamot, Argos, Command, Translate-shell, DeepL (curl), DeepL (CLI). The `Mt` struct manages translation state.

## Goals / Non-Goals

**Goals:**
- Expose all 30+ emuera-tty settings through a coherent Android settings UI
- Provide a dedicated translation configuration screen with engine selection and parameters
- Provide an environment variables reference screen
- Wire settings through JNI to the existing Rust config mechanism
- Ensure settings persist and live settings apply immediately
- Reuse existing Rust settings definitions (no duplication)

**Non-Goals:**
- Implement new translation engines (use existing ones in `mt.rs`)
- Change the Rust settings system or config file format
- Add settings not already in `src/settings.rs` ITEMS
- Implement a new config storage backend

## Decisions

### 1. Settings UI Structure: Three Activities

**Decision:** Create three new Android Activities:
- `SettingsActivity` — main settings screen with categorized list
- `TranslationSettingsActivity` — translation engine and parameters
- `EnvVarsActivity` — environment variables reference

**Rationale:** Android's Activity model fits hierarchical settings. Each screen has distinct lifecycle and navigation. Alternatives considered:
- Single activity with fragments: more complex state management for nested screens
- Dialog-based: too limited for translation config with many fields
- Jetpack Compose: would require adding Compose dependency; current codebase uses Views

### 2. JNI Bridge: Extend Native.kt with Settings Getters/Setters

**Decision:** Add JNI functions to `Native.kt` and corresponding Rust `extern "C"` functions:
- `getAllSettings(): String` — returns all settings as TSV (key\tlabel\tchoices\tcurrent\tdefault\tlive\thelp)
- `setSetting(key: String, value: String): String` — sets a setting, returns error or empty
- `getTranslationEngines(): String` — returns available engines as TSV
- `setTranslationEngine(engine: String): String`
- `getTranslationConfig(): String` — returns current translation config
- `setTranslationConfig(key: String, value: String): String`
- `testTranslation(text: String): String` — translates test text with current engine
- `getEnvVars(): String` — returns all EMUERA_* vars as TSV

**Rationale:** Batch getters avoid dozens of JNI calls. TSV is simple to parse in Kotlin. Alternatives:
- Individual getter/setter per setting: 60+ JNI functions, high maintenance
- JSON over JNI: adds JSON parsing dependency to Rust side
- SharedPreferences on Android side: would diverge from Rust config file

### 3. Settings Categories Match Rust ITEMS Order (with Android Simplification)

**Decision:** Organize `SettingsActivity` tabs/categories based on `src/settings.rs` ITEMS array, with the "images" setting simplified for Android and "colors" omitted (Android always has truecolor):
1. Language (language, **images: boolean disable-only**, contrast, wide, yen, letters)
2. Input (clipboard, statusbar, memory, kbdpointer, gamepad, wheel, tab_complete, scroll_end_on_key)
3. Translation (translator, translate, translate_from, translate_what) — opens TranslationSettingsActivity
4. Advanced (warmup, mouse, frontend, ee_version)

**Rationale:** Single source of truth for most settings. The "images" setting in Rust has 6 protocol choices (auto, kitty, iterm, sixel, fb, none), but Android uses a fixed rendering backend (GameView draws directly to Canvas). The Android UI exposes only a boolean "Disable image rendering" that maps to the Rust "none" choice. Other protocols are not applicable on Android.

### 4. Translation Engine Discovery on Rust Side

**Decision:** `Engine::find()` and `Engine::choose()` logic stays in Rust. Android calls `getTranslationEngines()` which runs discovery and returns available engines with descriptions.

**Rationale:** Engine detection checks for binaries (ollama, trans, curl, argos-translate), HTTP endpoints (Ollama, LibreTranslate), Python modules (Argos). This logic is complex and platform-dependent; keeping it in Rust avoids duplicating in Kotlin.

### 5. Game Settings (emuera.config) Stay in GameSettings.kt

**Decision:** The existing `GameSettings.kt` dialog for game's Emuera settings (from `emuconfig.rs` ITEMS) remains unchanged, accessible from both the game menu and the new main settings screen.

**Rationale:** Already implemented and working. The emuera.config settings are game-specific, not global, so they belong in a per-game dialog.

### 6. Live Settings: Call Native.reloadConfig() After Change

**Decision:** When a live setting is changed via `setSetting()`, the Rust side applies it immediately. Android calls `Native.reloadConfig()` to notify the running game.

**Rationale:** Matches desktop behavior where live settings take effect at next input. The `Settings.cycle()` and `set()` methods in Rust already handle live application.

### 7. Config File Path: Use Existing emuera-tty Config

**Decision:** Android uses the same config file path logic as desktop: `$XDG_CONFIG_HOME/emuera-tty/config` (via `crate::sys::config_base()`). On Android, this resolves to the app's internal data directory.

**Rationale:** `sys.rs` already handles Android paths correctly. No new path logic needed.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| JNI batch getter returns large string | TSV is compact; 30 settings × ~200 chars = ~6KB, well within JNI limits |
| Translation engine discovery blocks UI thread | Run discovery on background thread in Rust; Kotlin calls via coroutine |
| Live settings need game restart on some changes | Document which settings are live (from `Item.live`); Rust already tracks this |
| Android 14+ foreground service restrictions | Game already uses foreground service; settings changes don't affect it |
| Config file corruption on concurrent write | Rust `Settings.save()` uses atomic write (write to temp, rename); single process |
| Engine not found errors unclear to user | `Engine::choose()` returns descriptive errors; surface in UI |
| Many settings overwhelm users | Group by category; hide translation settings when translator=off (per `Settings.shown()`) |

## Migration Plan

1. Add Rust JNI functions in `src/jni.rs` (new module) or extend existing JNI bridge
2. Update `Native.kt` with new external declarations
3. Create `SettingsActivity.kt` with RecyclerView for categorized settings
4. Create `TranslationSettingsActivity.kt` with engine spinner, language pickers, file pickers
5. Create `EnvVarsActivity.kt` with searchable list
6. Update `GameActivity.showMenu()` to add "Settings" entry opening `SettingsActivity`
7. Add Activities to `AndroidManifest.xml`
8. Test each setting category with a real game
9. Verify config persistence across app restarts
10. Verify live settings apply without game restart

Rollback: Revert Kotlin files and JNI additions; no database migrations needed.

## Open Questions

1. Should `SettingsActivity` use a `PreferenceFragmentCompat` (AndroidX Preference library) or custom RecyclerView? Custom RecyclerView gives more control but PreferenceFragment handles persistence automatically. Decision: Custom RecyclerView to match existing UI style and avoid new dependency.

2. How to handle translation filter/load/log/storage file paths on Android? Android's scoped storage means users pick files via `ACTION_OPEN_DOCUMENT`/`ACTION_CREATE_DOCUMENT`. The paths passed to Rust must be accessible (use `contentResolver.openFileDescriptor`). Need to copy to cache file for Rust to read, or pass FD.

3. Should the env-vars screen show only variables relevant to Android (hide Linux-console-specific ones like EMUERA_GPMCTL)? Decision: Show all with platform tags; user can search.