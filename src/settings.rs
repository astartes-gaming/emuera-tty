//! Settings and the game library, kept in `$XDG_CONFIG_HOME/emuera-tty/config`
//! (default `~/.config/emuera-tty/config`) as `key=value` lines:
//!
//! ```text
//! game=/path/last/played
//! library=/path/to/eraTW
//! library=/path/to/eraMegaten
//! new=/path/to/eraMegaten
//! images=auto
//! contrast=normal
//! ```
//!
//! (`clone_url=`, `clone_dir=`, `clone_branch=` and `clone_depth=` are what
//! the launcher's clone prompts were last given: the next defaults.)
//!
//! (`new=` marks a game added from the launcher (cloned or picked) and not
//! played yet: it is listed as "(NEW)" until it is first played.)
//!
//! [`ITEMS`] describes every setting once, for the launcher's settings page
//! and the in-game overlay. Command-line options and `EMUERA_*` variables
//! still win over what is saved here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One setting: its config key, what the menus show, the values it cycles
/// through (the first is the default), whether a change takes effect at
/// once in a running game, the category the menus list it under, whether
/// it means something on Android, and why not when it does not.
///
/// `group`, `applies_on_android` and `android_limit` are read by the
/// Android frontend through `global_settings()` JNI (not by Rust code on
/// the host), so they appear dead to host-target clippy.
#[allow(dead_code)]
pub struct Item {
    pub key: &'static str,
    pub label: &'static str,
    pub choices: &'static [&'static str],
    pub live: bool,
    pub group: &'static str,
    pub applies_on_android: bool,
    pub android_limit: Option<&'static str>,
    pub help: &'static str,
}

pub const ITEMS: &[Item] = &[
    Item {
        key: "language",
        label: n!("Language"),
        choices: &["auto", "en", "ja", "zh-Hans", "zh-Hant", "ko", "es"],
        live: true,
        group: "Language",
        applies_on_android: true,
        android_limit: None,
        help: n!("the language of emuera-tty's own menus and messages, not the game's (auto: the system's)"),
    },
    Item {
        key: "images",
        label: n!("Images"),
        choices: &["auto", "kitty", "iterm", "sixel", "fb", "none"],
        live: true,
        group: "Display",
        applies_on_android: true,
        android_limit: Some("only \"none\" means anything: the app stops drawing pictures; the terminal protocols do not apply"),
        help: n!("how pictures are drawn (auto: whatever the terminal supports)"),
    },
    Item { key: "colors", label: n!("Colours"), choices: &["auto", "truecolor", "256", "16"], live: true, group: "Display", applies_on_android: false, android_limit: Some("the colour depth sent to a terminal; the Android renderer draws its own colours"), help: n!("colour depth sent to the terminal") },
    Item {
        key: "contrast",
        label: n!("Readable colours"),
        choices: &["normal", "high", "off"],
        live: true,
        group: "Display",
        applies_on_android: true,
        android_limit: None,
        help: n!("lighten coloured text too dark to read (e.g. blue in time stop)"),
    },
    Item {
        key: "wide",
        label: n!("Wide symbols"),
        choices: &["auto", "wide", "narrow", "single", "osc66"],
        live: true,
        group: "Display",
        applies_on_android: true,
        android_limit: None,
        help: n!("how box drawing / symbols get Emuera's two columns (auto: measured)"),
    },
    Item {
        key: "yen",
        label: n!("Backslash as ¥"),
        choices: &["auto", "on", "off"],
        live: true,
        group: "Display",
        applies_on_android: true,
        android_limit: None,
        help: n!("draw \\ as ¥ as Japanese fonts do (auto: when the game's font is one, as Emuera's default MS Gothic)"),
    },
    Item {
        key: "letters",
        label: n!("Greek and Cyrillic"),
        choices: &["auto", "wide", "narrow"],
        live: true,
        group: "Display",
        applies_on_android: true,
        android_limit: None,
        help: n!("two columns each, as Emuera's MS Gothic draws them, or one (auto: as the game's font, but words in those scripts narrow)"),
    },
    Item {
        key: "clipboard",
        label: n!("Copy to clipboard"),
        choices: &["game", "on", "off"],
        live: true,
        group: "Input",
        applies_on_android: false,
        android_limit: Some("the phone has its own clipboard, not wired to OSC 52"),
        help: n!("put printed text on the clipboard (OSC 52); game: as emuera.config says"),
    },
    Item { key: "statusbar", label: n!("Bottom bar details"), choices: &["on", "off"], live: true, group: "Input", applies_on_android: false, android_limit: Some("a touch screen has no hover"), help: n!("the hovered button's value and the running function") },
    Item {
        key: "memory",
        label: n!("Show memory"),
        choices: &["off", "on"],
        live: true,
        group: "Advanced",
        applies_on_android: true,
        android_limit: None,
        help: n!("show the memory in use in the bottom row (it is always on this page's title)"),
    },
    Item {
        key: "crashlog",
        label: n!("Save crash logs"),
        choices: &["on", "off"],
        live: true,
        group: "Advanced",
        applies_on_android: false,
        android_limit: Some("the app reports crashes itself (engine.log)"),
        help: n!("after a script error ends the game, save the error and the console to crash_logs/ in the game folder"),
    },
    Item {
        key: "translator",
        label: n!("Translator"),
        choices: &["off", "auto", "local", "argos", "translate-shell", "deepl", "file"],
        live: true,
        group: "Translation",
        applies_on_android: false,
        android_limit: Some("kept and marked; the translator is not wired into the Android frontend yet"),
        help: n!("off: the game shows as it prints itself; auto: whichever is there (this computer's first); local: Ollama, LibreTranslate… only; translate-shell, DeepL: online; file: only the translations read with --translate-load"),
    },
    Item {
        key: "translate",
        label: n!("Translate to"),
        choices: &["en", "es", "pt", "fr", "de", "it", "ru", "ja", "zh", "ko"],
        live: true,
        group: "Translation",
        applies_on_android: false,
        android_limit: Some("kept and marked; the translator is not wired into the Android frontend yet"),
        help: n!("the language to show the game in (needs a Translator other than off)"),
    },
    Item {
        key: "translate_from",
        label: n!("Translate from"),
        choices: &["auto", "ja", "en"],
        live: true,
        group: "Translation",
        applies_on_android: false,
        android_limit: Some("kept and marked; the translator is not wired into the Android frontend yet"),
        help: n!("the game's language: auto takes Japanese lines from Japanese and English ones from English"),
    },
    Item {
        key: "translate_what",
        label: n!("Translate what"),
        choices: &["auto", "dialog", "all"],
        live: true,
        group: "Translation",
        applies_on_android: false,
        android_limit: Some("kept and marked; the translator is not wired into the Android frontend yet"),
        help: n!("dialog: what characters say; all: menus and labels too (never maps); auto: all unless into English"),
    },
    Item { key: "sound", label: n!("Sound"), choices: &["auto", "on", "off"], live: true, group: "Input", applies_on_android: false, android_limit: Some("the app plays sound with MediaPlayer, not mpv or ffplay"), help: n!("PLAYSOUND / PLAYBGM through mpv, ffplay… (auto: not over SSH)") },
    Item {
        key: "kbdpointer",
        label: n!("Keyboard pointer"),
        choices: &["auto", "on", "off"],
        live: true,
        group: "Input",
        applies_on_android: false,
        android_limit: Some("there is no pointer on a touch screen"),
        help: n!("cursor keys move a pointer, Insert or Shift+Enter click (PgUp/PgDn scroll); auto: on a text console (Linux, Hurd) without a mouse"),
    },
    Item {
        key: "gamepad",
        label: n!("Gamepad"),
        choices: &["auto", "off"],
        live: true,
        group: "Input",
        applies_on_android: false,
        android_limit: Some("the app does not read gamepads"),
        help: n!("Steam Deck, Xbox, PlayStation… pads: d-pad between buttons, stick moves, A click, B skip, X/Y Enter, Start settings, LB/RB page"),
    },
    Item { key: "wheel", label: n!("Wheel scrolls"), choices: &["3", "1", "5", "10"], live: true, group: "Input", applies_on_android: true, android_limit: Some("the app's fling scrolls that many lines"), help: n!("lines per mouse-wheel step") },
    Item {
        key: "tab_complete",
        label: n!("Tab completion"),
        choices: &["off", "on"],
        live: true,
        group: "Input",
        applies_on_android: true,
        android_limit: None,
        help: n!("at an input, Tab completes what is typed to a button's value (by its start or a word of the button's text) or an @ command; Tab again shows the next one"),
    },
    Item {
        key: "scroll_end_on_key",
        label: n!("Scroll to end on keyboard input"),
        choices: &["off", "on"],
        live: true,
        group: "Input",
        applies_on_android: true,
        android_limit: None,
        help: n!("any key press while scrolled back jumps to the end (off: only mouse clicks and Enter do)"),
    },
    Item {
        key: "warmup",
        label: n!("Decode last used pictures"),
        choices: &["auto", "on", "off"],
        live: false,
        group: "Advanced",
        applies_on_android: true,
        android_limit: None,
        help: n!("decode pictures the game used last time in the background at start (auto: on unless headless)"),
    },
    Item { key: "mouse", label: n!("Console mouse"), choices: &["auto", "off"], live: false, group: "Advanced", applies_on_android: false, android_limit: Some("gpm and /dev/input/mice on the Linux console"), help: n!("gpm / /dev/input/mice on the Linux console") },
    Item {
        key: "frontend",
        label: n!("Display"),
        choices: &["auto", "ansi", "retro"],
        live: false,
        group: "Advanced",
        applies_on_android: false,
        android_limit: Some("ANSI and retro output are for terminals"),
        help: n!("retro: ASCII through terminfo, for old or dumb terminals"),
    },
    Item {
        key: "ee_version",
        label: n!("Target EE version"),
        choices: &["EEv56", "EEv56-anontw", "EEv57"],
        live: false,
        group: "Advanced",
        applies_on_android: true,
        android_limit: None,
        help: n!("which Emuera-EE version to emulate (EEv56: baseline; EEv56-anontw: EEv56 plus AnonTW fixes from Shakes4cc; EEv57: official EEv57)"),
    },
];

/// Every `EMUERA_*` environment variable emuera-tty reads, with what it
/// does (alphabetical), for the settings overlay's variables page.
pub const ENV_VARS: &[(&str, &str)] = &[
    ("EMUERA_AMBIGUOUS", n!("wide, narrow, single or osc66: how box drawing and symbols get Emuera's two columns")),
    ("EMUERA_BENCH_SYNC", n!("any value: wait for the terminal to answer after each frame, so frame rates measure the terminal (tools/termbench.sh)")),
    ("EMUERA_BREAKPOINTS", n!("comma-separated list of function:line breakpoints (debugger)")),
    ("EMUERA_CLOCK", n!("milliseconds since 1970 (UTC) the game's clock starts from, moving 1 ms per reading (reproducible runs)")),
    ("EMUERA_CODE_MB", n!("MB compiled functions may take (default: 1/16 of the memory); the least recently called are dropped")),
    ("EMUERA_COLORS", n!("truecolor, 256 or 16: the colour depth, when detection guesses wrong")),
    ("EMUERA_DEBUG_FRAMES", n!("any value: print each frame drawn (size and picture keys) to stderr (debugging)")),
    (
        "EMUERA_DISABLE_INFINITE_LOOP_DETECTION",
        n!("1: completely disable infinite loop detection, ignoring the timeout (useful for games with intentional long-running functions)"),
    ),
    ("EMUERA_GBUF_MB", n!("MB graphics buffers may take unpacked (default: 1/8 of the memory)")),
    ("EMUERA_GAME_DEBUG", n!("any value: enable game debugger at start")),
    ("EMUERA_GAMEPAD", n!("off: don't read gamepads; /dev/input/eventN: read just that device as a pad")),
    ("EMUERA_GPMCTL", n!("path of the gpm socket (default /dev/gpmctl), for testing the Linux-console mouse")),
    ("EMUERA_GPM_VC", n!("the virtual console number the gpm mouse is read for (testing)")),
    ("EMUERA_EE_VERSION", n!("EEv56, EEv56-anontw or EEv57: which Emuera-EE version to emulate for compatibility (default: EEv56)")),
    ("EMUERA_INFINITE_LOOP_ALERT_TIME", n!("ms before infinite loop warning (0 = disabled, default: 5000)")),
    ("EMUERA_INJECT", n!("STMT||STMT: statements run just before the EMUERA_INJECT_AT-th input (debugging)")),
    ("EMUERA_INJECT_AT", n!("the input number EMUERA_INJECT runs before")),
    (
        "EMUERA_LANG",
        n!("ja, zh-Hans, zh-Hant, ko, es or en: the interface language; x-cjk, x-arab, x-cyrl, x-deva, x-avst: every message in that script, to check the layout"),
    ),
    ("EMUERA_LIBRETRANSLATE", n!("host:port of the LibreTranslate server (default 127.0.0.1:5000)")),
    ("EMUERA_LINT", n!("any value, with --check: point at statements the parser may have split wrongly, and string >= / <= (Emuera evaluates them as <)")),
    ("EMUERA_LIST_BUTTONS", n!("1, with --headless: print the values of the buttons on screen before each input")),
    ("EMUERA_LOG_STYLE", n!("emuera: format log output like Emuera.exe (for headless mode)")),
    ("EMUERA_MEASURE", n!("0 skips measuring how wide the terminal draws ambiguous characters")),
    ("EMUERA_MEMSTATS", n!("file the report of what the memory holds is written to, at every input")),
    ("EMUERA_MOUSE", n!("off or term: no console mouse; mice: read /dev/input/mice even outside the Linux console")),
    ("EMUERA_MOUSE_LOG", n!("file the mouse events are logged to (finding out why a pointer misbehaves)")),
    ("EMUERA_PARSE", n!("all: compile every function at load instead of on its first call")),
    ("EMUERA_PICTURES_MB", n!("MB decoded picture files may take (default: 1/8 of the memory)")),
    ("EMUERA_PPROF", n!("file for the native profile (builds with --features profile; written on a clean exit)")),
    ("EMUERA_PROFILE", n!("file kept updated with which ERB function runs, sampled every millisecond")),
    ("EMUERA_RECOLOR", n!("colour override for headless mode (e.g. 'fg=255,255,255 bg=0,0,0')")),
    ("EMUERA_SAVE_HISTORY", n!("0: don't save history log to global.sav")),
    ("EMUERA_SEED", n!("number the random generator starts from (reproducible runs)")),
    ("EMUERA_SOUND", n!("on or off: play the game's sounds (default: yes, except over SSH)")),
    ("EMUERA_SOUND_PLAYER", n!("the sound player: mpv, ffplay, mplayer, afplay, pw-play, paplay, aplay or powershell")),
    ("EMUERA_STACK_MB", n!("stack of the script thread in MB (default 512, or the most the system allows)")),
    ("EMUERA_THREADS", n!("cap on the threads used for loading and picture decoding")),
    ("EMUERA_TIMING", n!("1: print how long each loading phase takes and the memory in use")),
    ("EMUERA_TRACE", n!("log calls to functions whose name contains this, with timestamps, to stderr")),
    ("EMUERA_TRANSLATE_CMD", n!("a command that reads a line on stdin and prints its translation (instead of the built-in translators)")),
    ("EMUERA_TRANSLATE_MODEL", n!("the Ollama or translateLocally model used for translating")),
    ("EMUERA_TRANS_ENGINE", n!("the engine translate-shell (trans) uses (default: Google)")),
    ("EMUERA_TTY_TESTING", n!("1: enable internal TTY testing hooks")),
    ("EMUERA_VM_DEBUG", n!("any value: enable VM debugger at start")),
    ("EMUERA_WARMUP", n!("1 / 0: decode last session's pictures in the background at start (default: on, off headless)")),
    ("EMUERA_GFX", n!("default for --gfx: kitty, iterm, sixel, fb or none")),
];

#[derive(Clone, Default)]
pub struct Settings {
    values: BTreeMap<String, String>,
    /// games added to the library, in the order they were added
    pub library: Vec<PathBuf>,
    /// the game played last
    pub last: Option<PathBuf>,
    /// games added from the launcher that haven't been played yet (listed as "(NEW)")
    pub fresh: Vec<PathBuf>,
}

/// `$XDG_CONFIG_HOME/emuera-tty/config` (default `~/.config/…`).
pub fn config_path() -> Option<PathBuf> {
    let base = crate::sys::config_base()?;
    Some(base.join("emuera-tty").join("config"))
}

/// The config file's modification-time based generation, for detecting changes:
/// frontends can compare this across reads and only re-read when it changed.
/// Returns `None` when the file can't be read or doesn't exist.
/// Used by the Android frontend; appears dead on host targets.
#[allow(dead_code)]
pub fn config_generation() -> Option<u64> {
    config_path()
        .and_then(|p| std::fs::metadata(&p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

impl Settings {
    pub fn load() -> Settings {
        config_path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Settings::default, |t| Settings::from_text(&t))
    }

    /// Settings from the config file's text.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "game" => s.last = Some(PathBuf::from(v)),
                "library" => {
                    let p = PathBuf::from(v);
                    if !s.library.contains(&p) {
                        s.library.push(p);
                    }
                }
                "new" => {
                    let p = PathBuf::from(v);
                    if !s.fresh.contains(&p) {
                        s.fresh.push(p);
                    }
                }
                _ => {
                    s.values.insert(k.to_string(), v.to_string());
                }
            }
        }
        // older versions turned translation off with "Translate to: off"
        match s.values.get("translate").map(String::as_str) {
            Some("off") => {
                s.values.remove("translate");
                s.values.insert("translator".into(), "off".into());
            }
            Some(_) if !s.values.contains_key("translator") => {
                s.values.insert("translator".into(), "auto".into());
            }
            _ => {}
        }
        // the remembered game is always in the library
        if let Some(g) = s.last.clone() {
            if !s.library.contains(&g) {
                s.library.insert(0, g);
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(p) = config_path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // write to a temp file and rename over the config, so a crash
        // mid-write cannot leave a half-written file
        let tmp = p.with_extension("config.tmp");
        if std::fs::write(&tmp, self.to_text()).is_err() {
            return;
        }
        let _ = std::fs::rename(&tmp, p);
    }

    /// Change one setting in the config file: re-read it, change the key,
    /// and save atomically. Returns "" on success or an error message.
    /// This is safe to call from any process: it re-reads the file first,
    /// so a change made elsewhere is not lost.
    /// Used by the Android frontend; appears dead on host targets.
    #[allow(dead_code)]
    pub fn set_one(key: &str, value: &str) -> String {
        if ITEMS.iter().find(|i| i.key == key).is_none() {
            return format!("unknown setting: {key}");
        }
        let mut s = Settings::load();
        s.set(key, value);
        s.save();
        String::new()
    }

    /// The config file's text for these settings.
    fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(g) = &self.last {
            out += &format!("game={}\n", g.display());
        }
        for g in &self.library {
            out += &format!("library={}\n", g.display());
        }
        for g in &self.fresh {
            out += &format!("new={}\n", g.display());
        }
        for (k, v) in &self.values {
            out += &format!("{}={}\n", k, v);
        }
        out
    }

    /// Did the player choose this setting (it's in their config file)?
    pub fn is_set(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// A setting's value (its default if unset or not a known choice).
    pub fn get(&self, key: &str) -> &str {
        let item = ITEMS.iter().find(|i| i.key == key);
        match (self.values.get(key), item) {
            (Some(v), Some(i)) if i.choices.contains(&v.as_str()) => i.choices.iter().find(|c| **c == v.as_str()).copied().unwrap_or(""),
            (_, Some(i)) => i.choices[0],
            (Some(v), None) => v.as_str(),
            (None, None) => "",
        }
    }

    /// A setting's value as the menus show it: a language by its own name.
    pub fn shown_value(&self, key: &str) -> String {
        let v = self.get(key);
        match key {
            "language" => crate::i18n::LANGUAGES.iter().find(|l| l.0 == v).map_or(v, |l| l.1).to_string(),
            _ => v.to_string(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
    }

    /// Remove a setting from the config file, back to its default.
    /// Used by the Android frontend; appears dead on host targets.
    #[allow(dead_code)]
    pub fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }

    /// Step a setting to its next (`dir` 1) or previous (-1) choice.
    pub fn cycle(&mut self, key: &str, dir: i32) -> &str {
        if let Some(i) = ITEMS.iter().find(|i| i.key == key) {
            let cur = i.choices.iter().position(|c| *c == self.get(key)).unwrap_or(0) as i32;
            let n = i.choices.len() as i32;
            let next = i.choices[((cur + dir) % n + n) as usize % n as usize];
            self.set(key, next);
        }
        self.get(key)
    }

    /// Is `it` offered? The language settings go while the Translator is off.
    pub fn shown(&self, it: &Item) -> bool {
        !(matches!(it.key, "translate" | "translate_from" | "translate_what") && self.get("translator") == "off")
    }

    /// Remember `game` as the last one played, adding it to the library.
    pub fn played(&mut self, game: &Path) {
        let g = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
        if !self.library.contains(&g) {
            self.library.push(g.clone());
        }
        // played at last: no longer new
        self.fresh.retain(|f| *f != g);
        self.last = Some(g);
    }

    /// Mark a game just added from the launcher as "(NEW)" until it is first played.
    pub fn mark_new(&mut self, game: &Path) {
        let g = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
        if !self.fresh.contains(&g) {
            self.fresh.push(g);
        }
    }

    pub fn add_game(&mut self, game: &Path) -> PathBuf {
        let g = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
        if !self.library.contains(&g) {
            self.library.push(g.clone());
        }
        g
    }

    /// Take a game off the list (its files stay where they are).
    pub fn remove_game(&mut self, game: &Path) {
        self.library.retain(|g| g != game);
        self.fresh.retain(|g| g != game);
        if self.last.as_deref() == Some(game) {
            self.last = None;
        }
    }

    /// Replace a game's path in the library (and last/fresh) with a new path.
    pub fn update_game(&mut self, old: &Path, new: &Path) {
        let new_canon = new.canonicalize().unwrap_or_else(|_| new.to_path_buf());
        for g in &mut self.library {
            if *g == old {
                *g = new_canon.clone();
            }
        }
        for g in &mut self.fresh {
            if *g == old {
                *g = new_canon.clone();
            }
        }
        if self.last.as_deref() == Some(old) {
            self.last = Some(new_canon);
        }
    }

    /// The contrast `console::readable` should reach.
    pub fn contrast_ratio(&self) -> f32 {
        match self.get("contrast") {
            "off" => 1.0,
            "high" => 4.5,
            _ => 3.0,
        }
    }

    /// Apply the settings that are read from the environment at startup,
    /// unless the variable is already set (the user's override wins).
    pub fn export_env(&self) {
        let set = |var: &str, v: &str| {
            if std::env::var_os(var).is_none() && v != "auto" {
                std::env::set_var(var, v);
            }
        };
        set("EMUERA_AMBIGUOUS", self.get("wide"));
        set("EMUERA_COLORS", self.get("colors"));
        set("EMUERA_WARMUP", self.get("warmup"));
        set("EMUERA_EE_VERSION", self.get("ee_version"));
        if self.get("mouse") == "off" {
            set("EMUERA_MOUSE", "off");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes tests that mutate the process-wide `XDG_CONFIG_HOME` env var.
    static CONFIG_ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// The language is saved as its code and shown by its own name
    #[test]
    fn language_setting() {
        let mut s = Settings::default();
        assert_eq!(s.get("language"), "auto");
        s.set("language", "zh-Hant");
        let back = Settings::from_text(&s.to_text());
        assert_eq!(back.get("language"), "zh-Hant");
        assert_eq!(back.shown_value("language"), "繁體中文");
        assert_eq!(back.shown_value("images"), "auto");
        s.set("language", "x-cjk");
        assert_eq!(Settings::from_text(&s.to_text()).get("language"), "auto");
    }

    #[test]
    fn defaults_and_cycling() {
        let mut s = Settings::default();
        assert_eq!(s.get("images"), "auto");
        assert_eq!(s.get("contrast"), "normal");
        assert_eq!(s.cycle("contrast", 1), "high");
        assert_eq!(s.cycle("contrast", 1), "off");
        assert_eq!(s.cycle("contrast", 1), "normal");
        assert_eq!(s.cycle("contrast", -1), "off");
        s.set("images", "bogus");
        assert_eq!(s.get("images"), "auto");
        s.played(Path::new("/nonexistent/eraMegaten"));
        s.played(Path::new("/nonexistent/eraTW"));
        s.played(Path::new("/nonexistent/eraMegaten"));
        assert_eq!(s.library.len(), 2);
        assert_eq!(s.last.as_deref(), Some(Path::new("/nonexistent/eraMegaten")));
        s.remove_game(Path::new("/nonexistent/eraMegaten"));
        assert_eq!(s.library, vec![PathBuf::from("/nonexistent/eraTW")]);
        assert!(s.last.is_none());
    }

    #[test]
    fn contrast_ratio_applies() {
        let mut s = Settings::default();
        assert_eq!(s.contrast_ratio(), 3.0); // normal
        s.set("contrast", "off");
        assert_eq!(s.contrast_ratio(), 1.0);
        s.set("contrast", "high");
        assert_eq!(s.contrast_ratio(), 4.5);
        s.set("contrast", "normal");
        assert_eq!(s.contrast_ratio(), 3.0);
    }

    #[test]
    fn export_env_sets_warmup_and_ee_version() {
        let _lock = CONFIG_ENV_MUTEX.lock().unwrap();
        std::env::remove_var("EMUERA_EE_VERSION");
        std::env::remove_var("EMUERA_WARMUP");
        let mut s = Settings::default();
        // defaults: warmup=auto → not set (auto means default behavior), ee_version=EEv56 → set
        s.export_env();
        assert_eq!(std::env::var("EMUERA_EE_VERSION").ok(), Some("EEv56".into()));
        assert!(std::env::var("EMUERA_WARMUP").is_err(), "auto warmup should not set the env var");

        // export_env does not override an already-set variable: remove it first
        std::env::remove_var("EMUERA_EE_VERSION");
        s.set("ee_version", "EEv57");
        s.export_env();
        assert_eq!(std::env::var("EMUERA_EE_VERSION").ok(), Some("EEv57".into()));

        std::env::remove_var("EMUERA_WARMUP");
        s.set("warmup", "on");
        s.export_env();
        assert_eq!(std::env::var("EMUERA_WARMUP").ok(), Some("on".into()));

        // clean up
        std::env::remove_var("EMUERA_EE_VERSION");
        std::env::remove_var("EMUERA_WARMUP");
    }

    #[test]
    fn new_games_stay_new_until_played() {
        let mut s = Settings::default();
        let g = Path::new("/nonexistent/eraNew");
        s.add_game(g);
        s.mark_new(g);
        s.mark_new(g);
        assert_eq!(s.fresh, vec![PathBuf::from("/nonexistent/eraNew")]);
        s.played(g);
        assert!(s.fresh.is_empty());
        s.mark_new(g);
        s.remove_game(g);
        assert!(s.fresh.is_empty());
    }

    #[test]
    fn every_item_has_a_group() {
        for it in ITEMS {
            assert!(!it.group.is_empty(), "{} has no group", it.key);
        }
    }

    #[test]
    fn items_applying_on_android_match_the_design() {
        let applying: Vec<&str> = ITEMS.iter().filter(|it| it.applies_on_android).map(|it| it.key).collect();
        let want = ["language", "images", "contrast", "wide", "yen", "letters", "memory", "wheel", "tab_complete", "scroll_end_on_key", "warmup", "ee_version"];
        assert_eq!(applying.len(), want.len());
        for k in &want {
            assert!(applying.contains(k), "{} should apply on Android", k);
        }
    }

    #[test]
    fn items_not_applying_on_android_have_a_reason() {
        for it in ITEMS {
            if !it.applies_on_android {
                assert!(it.android_limit.is_some(), "{} does not apply on Android but has no reason", it.key);
            }
        }
    }

    #[test]
    fn save_is_atomic_and_set_one_keeps_both_changes() {
        let _lock = CONFIG_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("emuera-setone-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let _ = std::fs::remove_dir_all(&tmp.join("emuera-tty"));

        // first change
        let err = Settings::set_one("contrast", "high");
        assert_eq!(err, "");
        // second change keeps the first
        let err = Settings::set_one("wide", "narrow");
        assert_eq!(err, "");

        let s = Settings::load();
        assert_eq!(s.get("contrast"), "high");
        assert_eq!(s.get("wide"), "narrow");

        // no temp file left behind
        let cfg = tmp.join("emuera-tty").join("config");
        let tmo = tmp.join("emuera-tty").join("config.tmp");
        assert!(cfg.is_file());
        assert!(!tmo.is_file());

        // unknown key is refused
        let err = Settings::set_one("no_such_key", "x");
        assert_ne!(err, "");
        assert!(err.contains("unknown setting"));

        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn leftover_config_tmp_is_ignored() {
        let _lock = CONFIG_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("emuera-tmpleft-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let cfg_dir = tmp.join("emuera-tty");
        let _ = std::fs::create_dir_all(&cfg_dir);

        // a real config with one setting
        std::fs::write(cfg_dir.join("config"), "contrast=high\n").unwrap();
        // a stale temp file from a crashed write
        std::fs::write(cfg_dir.join("config.tmp"), "contrast=off\nwide=narrow\n").unwrap();

        let s = Settings::load();
        assert_eq!(s.get("contrast"), "high");
        assert_eq!(s.get("wide"), "auto");

        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn config_generation_changes_when_the_file_changes() {
        let _lock = CONFIG_ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("emuera-configgen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let _ = std::fs::remove_dir_all(&tmp.join("emuera-tty"));

        // no config file: None
        assert_eq!(crate::settings::config_generation(), None);

        let cfg_dir = tmp.join("emuera-tty");
        let _ = std::fs::create_dir_all(&cfg_dir);
        let cfg = cfg_dir.join("config");
        std::fs::write(&cfg, "contrast=high\n").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));

        // file exists: Some generation
        let gen1 = crate::settings::config_generation();
        assert!(gen1.is_some());

        // unchanged: same generation
        let gen2 = crate::settings::config_generation();
        assert_eq!(gen2, gen1);

        // changed: different generation
        std::fs::write(&cfg, "contrast=off\n").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let gen3 = crate::settings::config_generation();
        assert_ne!(gen3, gen1);

        std::env::remove_var("XDG_CONFIG_HOME");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
