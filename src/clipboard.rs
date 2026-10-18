//! Emuera-EE's clipboard feature (from Emuera-Anchor): text the game prints
//! goes to the clipboard on chosen triggers, for translation tools and
//! text hookers. In a terminal the copy is an OSC 52 escape, which kitty,
//! WezTerm, foot, Windows Terminal, iTerm2, Konsole (when allowed) and tmux
//! (`set -g set-clipboard on`) pass to the system clipboard.
//!
//! The options are emuera.config's (Japanese or English names); the
//! settings overlay can force the feature on or off.

use crate::console::{Console, Line};
use base64::Engine;
use std::time::{Duration, Instant};

/// emuera.config's clipboard options, with Emuera's defaults.
#[derive(Clone, Debug)]
pub struct ClipConfig {
    pub enabled: bool,
    pub ignore_tags: bool,
    pub replace_tags: String,
    pub new_lines_only: bool,
    pub clear_on_refresh: bool,
    pub left_click: bool,
    pub middle_click: bool,
    pub double_click: bool,
    pub on_wait: bool,
    pub on_input: bool,
    /// lines put on the clipboard at most
    pub max_lines: usize,
    /// lines remembered at most
    pub buffer: usize,
    pub min_interval: Duration,
}

impl Default for ClipConfig {
    fn default() -> Self {
        ClipConfig {
            enabled: false,
            ignore_tags: false,
            replace_tags: ".".into(),
            new_lines_only: true,
            clear_on_refresh: false,
            left_click: true,
            middle_click: false,
            double_click: false,
            on_wait: false,
            on_input: true,
            max_lines: 25,
            buffer: 300,
            min_interval: Duration::from_millis(800),
        }
    }
}

impl ClipConfig {
    /// Apply one config line (key already made Japanese). True if it was
    /// a clipboard option.
    pub fn set(&mut self, key: &str, v: &str) -> bool {
        let yes = v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
        let num = |d: usize| v.trim().parse().unwrap_or(d);
        match key {
            "表示したテキストをクリップボードにコピーする" => self.enabled = yes,
            "テキスト中の<>タグを無視する" => self.ignore_tags = yes,
            "<>を次の文で置き換える" => self.replace_tags = v.to_string(),
            "新しい行のみコピーする" => self.new_lines_only = yes,
            "画面のリフレッシュ時にクリップボードとバッファを消去する" => self.clear_on_refresh = yes,
            "左クリックをトリガーにする" => self.left_click = yes,
            "ホイールクリックをトリガーにする" => self.middle_click = yes,
            "ダブルクリックをトリガーにする" => self.double_click = yes,
            "WAITをトリガーにする" => self.on_wait = yes,
            "INPUTをトリガーにする" => self.on_input = yes,
            "クリップボードに貼り付ける行数" => self.max_lines = num(self.max_lines).max(1),
            "総バッファサイズ" => self.buffer = num(self.buffer).max(1),
            "クリップボードの更新間隔(ミリ秒)" => self.min_interval = Duration::from_millis(num(800) as u64),
            _ => return false,
        }
        true
    }
}

/// What makes a copy happen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Trigger {
    LeftClick,
    MiddleClick,
    DoubleClick,
    Wait,
    Input,
}

/// The copier: remembers which printed lines were copied already.
#[derive(Default)]
pub struct Clipboard {
    /// absolute index (dropped lines included) of the first line not copied
    mark: usize,
    last_text: String,
    last_time: Option<Instant>,
}

/// A line as plain text; pictures (`[img:…]`, the only tags left once a
/// line is laid out) dropped or replaced like Emuera's `<>` tags.
fn plain(line: &Line, cfg: &ClipConfig) -> String {
    let t = Console::line_text(line);
    if !cfg.ignore_tags || !t.contains("[img:") {
        return t;
    }
    let mut out = String::new();
    let mut rest = t.as_str();
    while let Some(i) = rest.find("[img:") {
        out.push_str(&rest[..i]);
        out.push_str(&cfg.replace_tags);
        rest = rest[i..].find(']').map_or("", |j| &rest[i + j + 1..]);
    }
    out.push_str(rest);
    out
}

impl Clipboard {
    /// The game cleared the screen / went back to the title.
    pub fn reset(&mut self, con: &Console, cfg: &ClipConfig) {
        if cfg.clear_on_refresh {
            self.mark = con.dropped_front + con.lines.len();
            self.last_text.clear();
        }
    }

    /// If `t` is an enabled trigger and there are lines to copy, the OSC 52
    /// sequence that puts them on the clipboard.
    pub fn check(&mut self, con: &Console, cfg: &ClipConfig, t: Trigger) -> Option<Vec<u8>> {
        let on = match t {
            Trigger::LeftClick => cfg.left_click,
            Trigger::MiddleClick => cfg.middle_click,
            Trigger::DoubleClick => cfg.double_click,
            Trigger::Wait => cfg.on_wait,
            Trigger::Input => cfg.on_input,
        };
        if !cfg.enabled || !on {
            return None;
        }
        if self.last_time.is_some_and(|l| l.elapsed() < cfg.min_interval) {
            return None;
        }
        let end = con.dropped_front + con.lines.len();
        let new = end.saturating_sub(self.mark.max(con.dropped_front));
        if cfg.new_lines_only && new == 0 {
            return None;
        }
        // new lines only: those printed since the last copy (as many as
        // the buffer holds); else the last `max_lines`
        let n = if cfg.new_lines_only { new.min(cfg.buffer) } else { cfg.max_lines.min(cfg.buffer) }.min(con.lines.len());
        let lines = &con.lines[con.lines.len() - n..];
        let mut text = String::new();
        for l in lines {
            text.push_str(&plain(l, cfg));
            text.push('\n');
        }
        self.mark = end;
        if text.trim().is_empty() || text == self.last_text {
            return None;
        }
        self.last_time = Some(Instant::now());
        let seq = format!("\x1b]52;c;{}\x07", base64::engine::general_purpose::STANDARD.encode(text.as_bytes()));
        self.last_text = text;
        Some(seq.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_new_lines_once() {
        let mut con = Console::new();
        let cfg = ClipConfig { enabled: true, ignore_tags: true, ..ClipConfig::default() };
        con.print("hello");
        con.newline();
        let mut cb = Clipboard::default();
        let seq = cb.check(&con, &cfg, Trigger::Input).unwrap();
        let s = String::from_utf8(seq).unwrap();
        let b64 = s.trim_start_matches("\x1b]52;c;").trim_end_matches('\x07');
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(b64).unwrap(), b"hello\n");
        // nothing new: nothing copied
        assert!(cb.check(&con, &ClipConfig { min_interval: Duration::ZERO, ..cfg.clone() }, Trigger::Input).is_none());
        // a disabled trigger
        con.print("more");
        con.newline();
        assert!(cb.check(&con, &ClipConfig { min_interval: Duration::ZERO, ..cfg.clone() }, Trigger::Wait).is_none());
    }
}
