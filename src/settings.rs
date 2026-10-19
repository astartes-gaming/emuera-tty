//! Settings and the game library, kept in `$XDG_CONFIG_HOME/emuera-tty/config`
//! (default `~/.config/emuera-tty/config`) as `key=value` lines:
//!
//! ```text
//! game=/path/last/played
//! library=/path/to/eraTW
//! library=/path/to/eraMegaten
//! images=auto
//! contrast=normal
//! ```
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
    Item { key: "images", label: "Images", choices: &["auto", "kitty", "iterm", "sixel", "fb", "none"], live: true, help: "how pictures are drawn (auto: whatever the terminal supports)" },
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
        key: "clipboard",
        label: "Copy to clipboard",
        choices: &["game", "on", "off"],
        live: true,
        help: "put printed text on the clipboard (OSC 52); game: as emuera.config says",
    },
    Item {
        key: "statusbar",
        label: "Bottom bar details",
        choices: &["on", "off"],
        live: true,
        help: "the hovered button's value and the running function",
    },
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
        choices: &["auto", "local", "argos", "translate-shell", "deepl"],
        live: true,
        help: "auto: whichever is there (this computer's first); local: Ollama, LibreTranslate… only; translate-shell, DeepL: online",
    },
    Item {
        key: "translate",
        label: "Translate to",
        choices: &["off", "en", "es", "pt", "fr", "de", "it", "ru", "ja", "zh", "ko"],
        live: true,
        help: "the language to show the game in (with the translator below); off: as the game prints it",
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
    Item {
        key: "sound",
        label: "Sound",
        choices: &["auto", "on", "off"],
        live: true,
        help: "PLAYSOUND / PLAYBGM through mpv, ffplay… (auto: not over SSH)",
    },
    Item {
        key: "kbdpointer",
        label: "Keyboard pointer",
        choices: &["auto", "on", "off"],
        live: true,
        help: "cursor keys move a pointer, Insert or Shift+Enter click (PgUp/PgDn scroll); auto: on the Linux console without a mouse",
    },
    Item { key: "wheel", label: "Wheel scrolls", choices: &["3", "1", "5", "10"], live: true, help: "lines per mouse-wheel step" },
    Item { key: "mouse", label: "Console mouse", choices: &["auto", "off"], live: false, help: "gpm / /dev/input/mice on the Linux console" },
    Item {
        key: "frontend",
        label: "Display",
        choices: &["auto", "ansi", "retro"],
        live: false,
        help: "retro: ASCII through terminfo, for old or dumb terminals",
    },
];

#[derive(Clone, Default)]
pub struct Settings {
    values: BTreeMap<String, String>,
    /// games added to the library, in the order they were added
    pub library: Vec<PathBuf>,
    /// the game played last
    pub last: Option<PathBuf>,
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
                _ => {
                    s.values.insert(k.to_string(), v.to_string());
                }
            }
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

    /// Remember `game` as the last one played, adding it to the library.
    pub fn played(&mut self, game: &Path) {
        let g = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
        if !self.library.contains(&g) {
            self.library.push(g.clone());
        }
        self.last = Some(g);
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
}
