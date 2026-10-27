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
/// through (the first is the default), and whether a change takes effect
/// at once in a running game.
pub struct Item {
    pub key: &'static str,
    pub label: &'static str,
    pub choices: &'static [&'static str],
    pub live: bool,
    pub help: &'static str,
}

pub const ITEMS: &[Item] = &[
    Item {
        key: "images",
        label: "Images",
        choices: &["auto", "kitty", "iterm", "sixel", "fb", "none"],
        live: true,
        help: "how pictures are drawn (auto: whatever the terminal supports)",
    },
    Item { key: "colors", label: "Colours", choices: &["auto", "truecolor", "256", "16"], live: true, help: "colour depth sent to the terminal" },
    Item {
        key: "contrast",
        label: "Readable colours",
        choices: &["normal", "high", "off"],
        live: true,
        help: "lighten coloured text too dark to read (e.g. blue in time stop)",
    },
    Item {
        key: "wide",
        label: "Wide symbols",
        choices: &["auto", "wide", "narrow", "single", "osc66"],
        live: true,
        help: "how box drawing / symbols get Emuera's two columns (auto: measured)",
    },
    Item {
        key: "yen",
        label: "Backslash as ¥",
        choices: &["auto", "on", "off"],
        live: true,
        help: "draw \\ as ¥ as Japanese fonts do (auto: when the game's font is one, as Emuera's default MS Gothic)",
    },
    Item {
        key: "clipboard",
        label: "Copy to clipboard",
        choices: &["game", "on", "off"],
        live: true,
        help: "put printed text on the clipboard (OSC 52); game: as emuera.config says",
    },
    Item { key: "statusbar", label: "Bottom bar details", choices: &["on", "off"], live: true, help: "the hovered button's value and the running function" },
    Item {
        key: "memory",
        label: "Show memory",
        choices: &["off", "on"],
        live: true,
        help: "show the memory in use in the bottom row (it is always on this page's title)",
    },
    Item {
        key: "translator",
        label: "Translator",
        choices: &["off", "auto", "local", "argos", "translate-shell", "deepl", "file"],
        live: true,
        help: "off: the game shows as it prints itself; auto: whichever is there (this computer's first); local: Ollama, LibreTranslate… only; translate-shell, DeepL: online; file: only the translations read with --translate-load",
    },
    Item {
        key: "translate",
        label: "Translate to",
        choices: &["en", "es", "pt", "fr", "de", "it", "ru", "ja", "zh", "ko"],
        live: true,
        help: "the language to show the game in (needs a Translator other than off)",
    },
    Item {
        key: "translate_from",
        label: "Translate from",
        choices: &["auto", "ja", "en"],
        live: true,
        help: "the game's language: auto takes Japanese lines from Japanese and English ones from English",
    },
    Item {
        key: "translate_what",
        label: "Translate what",
        choices: &["auto", "dialog", "all"],
        live: true,
        help: "dialog: what characters say; all: menus and labels too (never maps); auto: all unless into English",
    },
    Item { key: "sound", label: "Sound", choices: &["auto", "on", "off"], live: true, help: "PLAYSOUND / PLAYBGM through mpv, ffplay… (auto: not over SSH)" },
    Item {
        key: "kbdpointer",
        label: "Keyboard pointer",
        choices: &["auto", "on", "off"],
        live: true,
        help: "cursor keys move a pointer, Insert or Shift+Enter click (PgUp/PgDn scroll); auto: on the Linux console without a mouse",
    },
    Item { key: "wheel", label: "Wheel scrolls", choices: &["3", "1", "5", "10"], live: true, help: "lines per mouse-wheel step" },
    Item {
        key: "warmup",
        label: "Decode last used pictures",
        choices: &["auto", "on", "off"],
        live: false,
        help: "decode pictures the game used last time in the background at start (auto: on unless headless)",
    },
    Item { key: "mouse", label: "Console mouse", choices: &["auto", "off"], live: false, help: "gpm / /dev/input/mice on the Linux console" },
    Item {
        key: "frontend",
        label: "Display",
        choices: &["auto", "ansi", "retro"],
        live: false,
        help: "retro: ASCII through terminfo, for old or dumb terminals",
    },
];

/// Every `EMUERA_*` environment variable emuera-tty reads, with what it
/// does (alphabetical), for the settings overlay's variables page.
pub const ENV_VARS: &[(&str, &str)] = &[
    ("EMUERA_AMBIGUOUS", "wide, narrow, single or osc66: how box drawing and symbols get Emuera's two columns"),
    ("EMUERA_CODE_MB", "MB compiled functions may take (default: 1/16 of the memory); the least recently called are dropped"),
    ("EMUERA_COLORS", "truecolor, 256 or 16: the colour depth, when detection guesses wrong"),
    ("EMUERA_DEBUG_FRAMES", "any value: print each frame drawn (size and picture keys) to stderr (debugging)"),
    ("EMUERA_GBUF_MB", "MB graphics buffers may take unpacked (default: 1/8 of the memory)"),
    ("EMUERA_INFINITE_LOOP_ALERT_TIME", "ms before infinite loop warning (0 = disabled, default: 5000)"),
    ("EMUERA_PICTURES_MB", "MB decoded picture files may take (default: 1/8 of the memory)"),
    ("EMUERA_WARMUP", "1 / 0: decode last session's pictures in the background at start (default: on, off headless)"),
    ("EMUERA_GFX", "default for --gfx: kitty, iterm, sixel, fb or none"),
    ("EMUERA_GPMCTL", "path of the gpm socket (default /dev/gpmctl), for testing the Linux-console mouse"),
    ("EMUERA_GPM_VC", "the virtual console number the gpm mouse is read for (testing)"),
    ("EMUERA_INJECT", "STMT||STMT: statements run just before the EMUERA_INJECT_AT-th input (debugging)"),
    ("EMUERA_INJECT_AT", "the input number EMUERA_INJECT runs before"),
    ("EMUERA_LIBRETRANSLATE", "host:port of the LibreTranslate server (default 127.0.0.1:5000)"),
    ("EMUERA_LINT", "any value, with --check: point at statements the parser may have split wrongly"),
    ("EMUERA_LIST_BUTTONS", "1, with --headless: print the values of the buttons on screen before each input"),
    ("EMUERA_MEASURE", "0 skips measuring how wide the terminal draws ambiguous characters"),
    ("EMUERA_MEMSTATS", "file the report of what the memory holds is written to, at every input"),
    ("EMUERA_MOUSE", "off or term: no console mouse; mice: read /dev/input/mice even outside the Linux console"),
    ("EMUERA_MOUSE_LOG", "file the mouse events are logged to (finding out why a pointer misbehaves)"),
    ("EMUERA_PARSE", "all: compile every function at load instead of on its first call"),
    ("EMUERA_PPROF", "file for the native profile (builds with --features profile; written on a clean exit)"),
    ("EMUERA_PROFILE", "file kept updated with which ERB function runs, sampled every millisecond"),
    ("EMUERA_SEED", "number the random generator starts from (reproducible runs)"),
    ("EMUERA_SOUND", "on or off: play the game's sounds (default: yes, except over SSH)"),
    ("EMUERA_SOUND_PLAYER", "the sound player: mpv, ffplay, mplayer, afplay, pw-play, paplay, aplay or powershell"),
    ("EMUERA_THREADS", "cap on the threads used for loading and picture decoding"),
    ("EMUERA_TIMING", "1: print how long each loading phase takes and the memory in use"),
    ("EMUERA_TRACE", "log calls to functions whose name contains this, with timestamps, to stderr"),
    ("EMUERA_TRANSLATE_CMD", "a command that reads a line on stdin and prints its translation (instead of the built-in translators)"),
    ("EMUERA_TRANSLATE_MODEL", "the Ollama or translateLocally model used for translating"),
    ("EMUERA_TRANS_ENGINE", "the engine translate-shell (trans) uses (default: Google)"),
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

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = config_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return s };
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
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, out);
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

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.to_string(), value.to_string());
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
        if self.get("mouse") == "off" {
            set("EMUERA_MOUSE", "off");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
