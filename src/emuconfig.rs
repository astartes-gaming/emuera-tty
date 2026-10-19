//! The game's Emuera settings: read from `CSV/_default.config`, the game's
//! `emuera.config` and `CSV/_fixed.config` (which wins), as Emuera does,
//! at start and when they change; and the model of the settings page that
//! edits them like Emuera's own settings window. Changes go where Emuera
//! saves them, the game's `emuera.config`, so Emuera.exe and emuera-tty
//! share them.

use crate::config_names::{canonical, NAMES};
use crate::console::Console;
use crate::vm::save::SaveOpts;
use std::path::{Path, PathBuf};

/// Read a few keys from the config files (key:value lines), in Emuera's
/// order: CSV/_default.config, emuera.config, CSV/_fixed.config (wins).
/// Returns the configured window width in pixels, if any.
pub fn load(game: &Path, csv_dir: &Path, con: &mut Console, text_exts: &mut Vec<String>, saves: &mut SaveOpts) -> Option<f32> {
    let mut paths = vec![crate::loader::find_file_ci(csv_dir, "_default.config"), crate::loader::find_file_ci(game, "emuera.config")];
    if let Some(d) = crate::loader::find_dir_ci(game, "config") {
        paths.push(crate::loader::find_file_ci(&d, "emuera.config"));
    }
    paths.push(crate::loader::find_file_ci(csv_dir, "_fixed.config"));
    let mut window_px = None;
    let yes = |v: &str| v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
    let mut zip = false;
    let mut binary = false;
    for p in paths.into_iter().flatten() {
        let Ok(text) = crate::loader::read_text(&p) else { continue };
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            let rgb = |v: &str| -> Option<crate::console::Rgb> {
                let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() == 3 { Some(crate::console::Rgb(n[0], n[1], n[2])) } else { None }
            };
            // keys may be written in Japanese or English (WINDOW WIDTH…)
            let k = line.trim_start_matches('\u{feff}').split_once(':').map(|x| x.0).unwrap_or(k);
            match crate::config_names::canonical(k) {
                "ウィンドウ幅" => window_px = v.parse().ok().or(window_px),
                "PRINTCを並べる数" => con.printc_per_line = v.parse().unwrap_or(con.printc_per_line),
                "フォント名" => {
                    con.default_font = v.to_string();
                    con.font_name = v.to_string();
                }
                "ボタンの途中で行を折りかえさない" => con.button_wrap = yes(v),
                "セーブデータをsavフォルダ内に作成する" => saves.in_folder = yes(v),
                "セーブデータをバイナリ形式で保存する" => binary = yes(v),
                "セーブデータを圧縮して保存する" => zip = yes(v),
                "オートセーブを行なう" => saves.autosave = yes(v),
                // Emuera keeps it within 20..80
                "表示するセーブデータ数" => saves.slots = v.parse::<i64>().map_or(saves.slots, |n| n.clamp(20, 80)),
                "LOADTEXTとSAVETEXTで使える拡張子" => {
                    for e in v.split(',').map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()) {
                        if !text_exts.contains(&e) {
                            text_exts.push(e);
                        }
                    }
                }
                "フォントサイズ" => con.font_size = v.parse().unwrap_or(con.font_size),
                "一行の高さ" => con.line_height = v.parse().unwrap_or(con.line_height),
                "文字色" => {
                    if let Some(c) = rgb(v) {
                        con.default_fg = c;
                        con.style.fg = c;
                    }
                }
                "背景色" => {
                    if let Some(c) = rgb(v) {
                        con.default_bg = c;
                        con.screen_bg = c;
                    }
                }
                "選択中文字色" => {
                    if let Some(c) = rgb(v) {
                        con.focus_fg = c
                    }
                }
                "履歴文字色" => {
                    if let Some(c) = rgb(v) {
                        con.log_fg = c
                    }
                }
                "PRINTCを並べる文字数" | "PRINTCの文字数" => con.printc_len = v.parse().unwrap_or(con.printc_len),
                "表示するログの最大行数" | "履歴ログの行数" => con.max_lines = v.parse().unwrap_or(con.max_lines),
                // Emuera-EE's clipboard options
                other => {
                    con.clip.set(other, v);
                }
            }
        }
    }
    // Emuera compresses only binary saves; emuera-tty always writes binary
    // (Emuera reads it whatever its config says)
    saves.zip = zip && binary;
    window_px
}


/// What kind of value a setting holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Bool,
    /// a number from `min` to `max`
    Int(i64, i64),
    /// `R,G,B`
    Color,
    Text,
}

/// One setting of Emuera's settings window.
pub struct Item {
    /// the key (Japanese, as Emuera names it)
    pub jp: &'static str,
    pub label: &'static str,
    /// the tab of Emuera's settings window it is on
    pub tab: &'static str,
    pub kind: Kind,
    /// Emuera's own default
    pub default: &'static str,
}

const fn item(tab: &'static str, jp: &'static str, label: &'static str, kind: Kind, default: &'static str) -> Item {
    Item { jp, label, tab, kind, default }
}

/// The tabs, in the order of Emuera's settings window.
pub const TABS: &[&str] = &["Environment", "Display", "Font", "System2", "Clipboard"];

/// The settings on the page: those of Emuera's settings window that do
/// something in emuera-tty and can't break a game. Left out: what only
/// a window has (size, position, FPS, the menu bar, macros), what changes
/// how scripts are read (the System, Compatibility and Debug tabs), the
/// font size and line height (games compute picture positions from
/// them), the save folder (existing saves would seem to vanish) and the
/// background and history colours (the terminal's own are used).
pub const ITEMS: &[Item] = &[
    item("Environment", "オートセーブを行なう", "Make autosaves", Kind::Bool, "YES"),
    item("Environment", "履歴ログの行数", "Lines in history log", Kind::Int(100, 1000000), "5000"),
    item("Environment", "表示するセーブデータ数", "Number of save slots", Kind::Int(20, 80), "20"),
    item("Display", "PRINTCを並べる数", "PRINTC items per line", Kind::Int(1, 100), "3"),
    item("Display", "PRINTCの文字数", "PRINTC item width", Kind::Int(1, 400), "25"),
    item("Display", "ボタンの途中で行を折りかえさない", "Don't break lines inside a button", Kind::Bool, "NO"),
    item("Font", "文字色", "Text colour", Kind::Color, "192,192,192"),
    item("Font", "選択中文字色", "Selected button colour", Kind::Color, "255,255,0"),
    item("Font", "フォント名", "Font (for text drawn into pictures)", Kind::Text, "ＭＳ ゴシック"),
    item("System2", "セーブデータを圧縮して保存する", "Compress save data", Kind::Bool, "NO"),
    item("Clipboard", "表示したテキストをクリップボードにコピーする", "Copy printed text to the clipboard", Kind::Bool, "NO"),
    item("Clipboard", "新しい行のみコピーする", "Only new lines", Kind::Bool, "YES"),
    item("Clipboard", "クリップボードに貼り付ける行数", "Lines copied (when not only new ones)", Kind::Int(1, 10000), "25"),
    item("Clipboard", "総バッファサイズ", "Lines remembered", Kind::Int(1, 100000), "300"),
    item("Clipboard", "クリップボードの更新間隔(ミリ秒)", "Minimum time between copies (ms)", Kind::Int(0, 60000), "800"),
    item("Clipboard", "テキスト中の<>タグを無視する", "Replace pictures (<> tags) …", Kind::Bool, "NO"),
    item("Clipboard", "<>を次の文で置き換える", "… with this text", Kind::Text, "."),
    item("Clipboard", "画面のリフレッシュ時にクリップボードとバッファを消去する", "Forget lines when the game clears the screen", Kind::Bool, "NO"),
    item("Clipboard", "左クリックをトリガーにする", "Copy on a left click", Kind::Bool, "YES"),
    item("Clipboard", "ホイールクリックをトリガーにする", "Copy on a middle click", Kind::Bool, "NO"),
    item("Clipboard", "ダブルクリックをトリガーにする", "Copy on a double click", Kind::Bool, "NO"),
    item("Clipboard", "WAITをトリガーにする", "Copy at a WAIT", Kind::Bool, "NO"),
    item("Clipboard", "INPUTをトリガーにする", "Copy at an input", Kind::Bool, "YES"),
];

/// Where a setting's value comes from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    /// Emuera's default
    Emuera,
    /// the game's CSV/_default.config
    Game,
    /// emuera.config (set by the player, here or in Emuera.exe)
    Player,
    /// CSV/_fixed.config: the game doesn't allow changing it
    Fixed,
}

/// The game's settings as the config files have them.
pub struct Values {
    /// key (Japanese) → (value, where from)
    vals: std::collections::HashMap<String, (String, Source)>,
    /// the game's emuera.config (it may not exist yet)
    pub path: PathBuf,
    /// write keys in English (the game's files do)
    english: bool,
}

/// `key:value` lines of a config file (BOM, blank and `;` lines skipped).
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|l| {
        let l = l.trim_start_matches('\u{feff}');
        if l.trim_start().starts_with(';') {
            return None;
        }
        l.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
    })
}

/// Does this key name an English one (`WINDOW WIDTH`)?
fn is_english(key: &str) -> bool {
    NAMES.iter().any(|(_, en)| key.eq_ignore_ascii_case(en))
}

impl Values {
    pub fn read(game: &Path) -> Values {
        let csv = crate::loader::find_dir_ci(game, "CSV");
        let path = crate::loader::find_file_ci(game, "emuera.config").unwrap_or_else(|| game.join("emuera.config"));
        let mut v = Values { vals: Default::default(), path: path.clone(), english: false };
        let files = [
            (csv.as_deref().and_then(|c| crate::loader::find_file_ci(c, "_default.config")), Source::Game),
            (Some(path), Source::Player),
            (csv.as_deref().and_then(|c| crate::loader::find_file_ci(c, "_fixed.config")), Source::Fixed),
        ];
        let mut seen_keys = (0, 0);
        for (p, src) in files {
            let Some(text) = p.and_then(|p| crate::loader::read_text(&p).ok()) else { continue };
            for (k, val) in pairs(&text) {
                if src != Source::Fixed {
                    if is_english(k) {
                        seen_keys.1 += 1;
                    } else {
                        seen_keys.0 += 1;
                    }
                }
                v.vals.insert(canonical(k).to_string(), (val.to_string(), src));
            }
        }
        // write the way the game's files are written (Emuera reads both)
        v.english = seen_keys.1 > seen_keys.0;
        v
    }

    /// Is the yes/no setting `jp` (its Japanese key) on? Off when unset.
    pub fn flag(&self, jp: &str) -> bool {
        self.vals.get(jp).is_some_and(|(x, _)| x.trim().eq_ignore_ascii_case("YES") || x.trim().eq_ignore_ascii_case("TRUE"))
    }

    /// The value in effect, and where it comes from.
    pub fn get(&self, it: &Item) -> (String, Source) {
        match self.vals.get(it.jp) {
            Some((v, s)) => (v.clone(), *s),
            None => (it.default.to_string(), Source::Emuera),
        }
    }

    /// Set a value in emuera.config (None: take it out, back to the
    /// game's default). Other lines of the file stay as they are.
    pub fn set(&mut self, it: &Item, value: Option<&str>) -> Result<(), String> {
        let raw = std::fs::read(&self.path).unwrap_or_default();
        let sjis = !raw.is_empty() && crate::loader::is_sjis(&raw);
        let text = crate::loader::decode(&raw);
        let mut lines: Vec<String> = text.lines().map(|l| l.trim_start_matches('\u{feff}').to_string()).collect();
        let at = lines.iter().position(|l| l.split_once(':').is_some_and(|(k, _)| canonical(k) == it.jp));
        match (at, value) {
            (Some(i), Some(v)) => {
                let key = lines[i].split_once(':').map(|(k, _)| k.to_string()).unwrap_or_default();
                lines[i] = format!("{}:{}", key, v);
            }
            (Some(i), None) => {
                lines.remove(i);
            }
            (None, Some(v)) => {
                let key = if self.english { NAMES.iter().find(|(jp, _)| *jp == it.jp).map_or(it.jp, |(_, en)| en).to_uppercase() } else { it.jp.to_string() };
                lines.push(format!("{}:{}", key, v));
            }
            (None, None) => {}
        }
        let mut out = lines.join("\r\n");
        if !out.is_empty() {
            out.push_str("\r\n");
        }
        // Emuera writes UTF-8 with a BOM; an old Shift_JIS file stays so
        let bytes = if sjis { encoding_rs::SHIFT_JIS.encode(&out).0.into_owned() } else { [&[0xEF, 0xBB, 0xBF][..], out.as_bytes()].concat() };
        std::fs::write(&self.path, bytes).map_err(|e| format!("{}: {}", self.path.display(), e))?;
        // what is in effect now
        let csv = self.path.parent().map(Values::read);
        if let Some(n) = csv {
            self.vals = n.vals;
        }
        Ok(())
    }
}

/// A typed value made into what the config file holds, or why it can't be.
pub fn normalize(it: &Item, s: &str) -> Result<String, String> {
    let s = s.trim();
    match it.kind {
        Kind::Bool => match s.to_uppercase().as_str() {
            "YES" | "Y" | "TRUE" | "ON" | "1" => Ok("YES".into()),
            "NO" | "N" | "FALSE" | "OFF" | "0" => Ok("NO".into()),
            _ => Err("YES or NO".into()),
        },
        Kind::Int(lo, hi) => match s.parse::<i64>() {
            Ok(n) if (lo..=hi).contains(&n) => Ok(n.to_string()),
            _ => Err(format!("a number from {} to {}", lo, hi)),
        },
        Kind::Color => {
            // R,G,B or #RRGGBB
            let n: Vec<i64> = if let Some(h) = s.strip_prefix('#').filter(|h| h.len() == 6) {
                (0..3).filter_map(|i| i64::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()).collect()
            } else {
                s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
            };
            match n[..] {
                [r, g, b] if [r, g, b].iter().all(|c| (0..=255).contains(c)) => Ok(format!("{},{},{}", r, g, b)),
                _ => Err("R,G,B (0-255 each) or #RRGGBB".into()),
            }
        }
        Kind::Text => Ok(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_keeps_the_file() {
        let d = std::env::temp_dir().join(format!("emuera-cfg-{}", std::process::id()));
        std::fs::create_dir_all(d.join("CSV")).unwrap();
        std::fs::write(d.join("CSV/_default.config"), "ITEMS PER LINE FOR PRINTC:5\nFONT SIZE:16\nSAVE DATA COUNT PER PAGE:200\n").unwrap();
        std::fs::write(d.join("CSV/_fixed.config"), "NUMBER OF ITEM CHARACTERS FOR PRINTC:16\n").unwrap();
        let it = |jp: &str| ITEMS.iter().find(|i| i.jp == jp).unwrap();
        let mut v = Values::read(&d);
        assert_eq!(v.get(it("PRINTCを並べる数")), ("5".to_string(), Source::Game));
        assert_eq!(v.get(it("PRINTCの文字数")).1, Source::Fixed);
        assert_eq!(v.get(it("文字色")), ("192,192,192".to_string(), Source::Emuera));
        // the game writes English keys: so does a new emuera.config
        v.set(it("PRINTCを並べる数"), Some("4")).unwrap();
        v.set(it("文字色"), Some(&normalize(it("文字色"), "#102030").unwrap())).unwrap();
        let raw = std::fs::read(&v.path).unwrap();
        assert!(raw.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8_lossy(&raw[3..]).to_string();
        assert_eq!(text, "ITEMS PER LINE FOR PRINTC:4\r\nTEXT COLOR:16,32,48\r\n");
        assert_eq!(v.get(it("PRINTCを並べる数")), ("4".to_string(), Source::Player));
        // back to the game's value
        v.set(it("PRINTCを並べる数"), None).unwrap();
        assert_eq!(v.get(it("PRINTCを並べる数")), ("5".to_string(), Source::Game));
        // and what the game loads
        let mut con = Console::new();
        let (mut ext, mut so) = (Vec::new(), SaveOpts::default());
        load(&d, &d.join("CSV"), &mut con, &mut ext, &mut so);
        assert_eq!((con.default_fg.0, con.default_fg.1, con.default_fg.2), (16, 32, 48));
        assert_eq!(so.slots, 80, "clamped as Emuera does");
        assert!(normalize(it("表示するセーブデータ数"), "100").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
