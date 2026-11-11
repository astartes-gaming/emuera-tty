//! The debug panel: Emuera's debug window (DebugDialog) in the terminal,
//! for games run with `--game-debug`. Ctrl-D (or `@DEBUG`) opens it over
//! the game, at an input. Three tabs, as Emuera's: the variable watch (the
//! expressions in `debug/watchlist.csv` and their values), the stack
//! trace, and the console (DEBUGPRINT's output; a line runs a debug
//! command). What it shows comes from the VM (`vm/debugwin.rs`): every
//! change is sent back with [`DebugAction`] and the panel opens again on
//! the VM's answer.

use super::{write_all, Key, Tty};
use crate::console::{Console, DebugView};
use crate::frontend::{DebugAction, InputReq, InputResult};
use crate::i18n::{fit, width};
use crate::resources::Resources;
use std::io::Write;

/// The tabs, as Emuera names them (変数ウォッチ, スタックトレース, コンソール).
pub(super) const DEBUG_TABS: &[&str] = &[n!("Variable watch"), n!("Stack trace"), n!("Console")];

/// What the panel keeps between the VM's answers.
#[derive(Default)]
pub(super) struct PanelState {
    /// the watch selected (one past the last: the empty row to add one)
    pub sel: usize,
    /// lines scrolled back from the end (stack trace, console)
    pub scroll: usize,
    /// the console's input line
    pub line: super::lineedit::LineEdit,
}

impl Tty {
    /// Show the panel until it needs the VM: a debug action, or Quit.
    pub(super) fn debug_panel(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        let mut note = String::new();
        loop {
            self.no_images = true;
            self.render(con, res, Some(req));
            self.draw_debug_panel(&con.debug, &self.dbg, &note);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    return InputResult::Quit;
                }
                continue;
            };
            note.clear();
            let d = &mut con.debug;
            let n_watch = d.watches.len();
            match k {
                Key::CtrlC => return InputResult::Quit,
                Key::Esc | Key::Debug => {
                    d.open = false;
                    self.force_frame = true;
                    return InputResult::Debug(DebugAction::Close);
                }
                Key::Skip => {
                    d.tab = (d.tab + 1) % DEBUG_TABS.len();
                    self.dbg.scroll = 0;
                }
                Key::Left if d.tab != 2 => {
                    d.tab = (d.tab + DEBUG_TABS.len() - 1) % DEBUG_TABS.len();
                    self.dbg.scroll = 0;
                }
                Key::Right if d.tab != 2 => {
                    d.tab = (d.tab + 1) % DEBUG_TABS.len();
                    self.dbg.scroll = 0;
                }
                Key::PageUp => self.dbg.scroll += self.rows.saturating_sub(8).max(1),
                Key::PageDown => self.dbg.scroll = self.dbg.scroll.saturating_sub(self.rows.saturating_sub(8).max(1)),
                // the variable watch
                Key::Up if d.tab == 0 => self.dbg.sel = self.dbg.sel.saturating_sub(1),
                Key::Down if d.tab == 0 => self.dbg.sel = (self.dbg.sel + 1).min(n_watch),
                Key::Char('r') if d.tab == 0 => return InputResult::Debug(DebugAction::Refresh),
                Key::Delete | Key::Backspace | Key::Char('d') if d.tab == 0 && self.dbg.sel < n_watch => {
                    d.watches.remove(self.dbg.sel);
                    return InputResult::Debug(DebugAction::Refresh);
                }
                Key::Enter | Key::Char('a') | Key::Char('e') if d.tab == 0 => {
                    let i = if matches!(k, Key::Char('a')) { n_watch } else { self.dbg.sel.min(n_watch) };
                    let old = d.watches.get(i).cloned().unwrap_or_default();
                    let title = t!("Variable watch");
                    let label = t!("Expression (empty: remove):");
                    let Some(e) = self.prompt(con, res, req, &title, &label, &old) else { continue };
                    let d = &mut con.debug;
                    let e = e.trim().to_string();
                    match (i < d.watches.len(), e.is_empty()) {
                        (true, true) => {
                            d.watches.remove(i);
                        }
                        (true, false) => d.watches[i] = e,
                        (false, false) => {
                            d.watches.push(e);
                            self.dbg.sel = d.watches.len() - 1;
                        }
                        (false, true) => continue,
                    }
                    return InputResult::Debug(DebugAction::Refresh);
                }
                // the stack trace
                Key::Up if d.tab == 1 => self.dbg.scroll += 1,
                Key::Down if d.tab == 1 => self.dbg.scroll = self.dbg.scroll.saturating_sub(1),
                Key::Char('r') if d.tab == 1 => return InputResult::Debug(DebugAction::Refresh),
                // the console's input line
                Key::Enter if d.tab == 2 => {
                    let c = self.dbg.line.take();
                    if c.trim().is_empty() {
                        continue;
                    }
                    self.dbg.line.remember(&c);
                    self.dbg.scroll = 0;
                    return InputResult::Debug(DebugAction::Command(c));
                }
                Key::Up if d.tab == 2 => self.dbg.line.recall(-1),
                Key::Down if d.tab == 2 => self.dbg.line.recall(1),
                Key::Left if d.tab == 2 => self.dbg.line.left(),
                Key::Right if d.tab == 2 => self.dbg.line.right(),
                Key::Home if d.tab == 2 => self.dbg.line.home(),
                Key::End if d.tab == 2 => self.dbg.line.end(),
                Key::Backspace if d.tab == 2 => self.dbg.line.backspace(),
                Key::Delete if d.tab == 2 => self.dbg.line.delete(),
                Key::Char('\u{15}') if d.tab == 2 => self.dbg.line.clear(),
                Key::Char(c) if d.tab == 2 && !c.is_control() => self.dbg.line.insert(c),
                _ => {}
            }
        }
    }

    /// Draw the panel over the game (its own lines: no picture, no
    /// buttons).
    pub(super) fn draw_debug_panel(&self, d: &DebugView, st: &PanelState, note: &str) {
        self.redraw_all.set(true);
        let w = self.cols.saturating_sub(2).max(30);
        let inner = w - 2;
        let left = self.cols.saturating_sub(w) / 2 + 1;
        let h = self.rows.max(10);
        let top = 1;
        // rows between the tab bar and the help line
        let body = h.saturating_sub(6).max(1);
        let bg = (22, 32, 58);
        let frame = self.ov_style((150, 170, 210), bg, false);
        let title = self.ov_style((255, 255, 255), bg, true);
        let normal = self.ov_style((235, 235, 235), bg, false);
        let dim = self.ov_style((130, 140, 165), bg, false);
        let hilite = self.ov_style((0, 0, 0), (230, 230, 230), false);
        let tab_off = self.ov_style((170, 190, 225), bg, false);
        let helpc = self.ov_style((255, 220, 90), bg, false);
        let errc = self.ov_style((255, 140, 140), bg, false);
        let mut out: Vec<u8> = Vec::new();
        let row = |out: &mut Vec<u8>, r: usize, t: &str, st: &str| {
            let _ = write!(out, "\x1b[{};{}H{}│{}{}{}│", r, left, frame, st, fit(t, inner), frame);
        };
        let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
        row(&mut out, top + 1, &format!(" {}", t!("Debug  (Emuera's debug window)")), &title);
        // the tab bar
        let _ = write!(out, "\x1b[{};{}H{}│{} ", top + 2, left, frame, normal);
        let mut used = 1;
        for (i, t) in DEBUG_TABS.iter().enumerate() {
            let label = format!(" {} ", crate::i18n::tr(t));
            let lw = width(&label);
            if used + lw + 1 > inner {
                break;
            }
            let _ = write!(out, "{}{}{} ", if i == d.tab { &hilite } else { &tab_off }, label, normal);
            used += lw + 1;
        }
        let _ = write!(out, "{}{}│", " ".repeat(inner - used), frame);
        let first = top + 3;
        let mut cursor = None;
        match d.tab {
            0 => {
                // expression | value, Emuera's two columns; one empty row
                // more to add a watch
                let ew = (inner / 3).clamp(10, 40);
                let vw = inner.saturating_sub(ew + 3);
                let n = d.watches.len() + 1;
                let start = (st.sel + 1).saturating_sub(body).min(n.saturating_sub(body));
                for k in 0..body {
                    let i = start + k;
                    let r = first + k;
                    if i >= n {
                        row(&mut out, r, "", &normal);
                        continue;
                    }
                    let (e, v) = match d.watches.get(i) {
                        Some(e) => (e.clone(), d.values.get(i).cloned().unwrap_or_default()),
                        None => (t!("(new: Enter)"), String::new()),
                    };
                    let text = format!(" {} │ {}", fit(&e, ew), fit(&v.replace('\n', " "), vw));
                    let style = if i == st.sel {
                        &hilite
                    } else if i == d.watches.len() {
                        &dim
                    } else {
                        &normal
                    };
                    row(&mut out, r, &text, style);
                }
            }
            1 | 2 => {
                // the text, its end at the bottom (scrolled back by `scroll`)
                let text = if d.tab == 1 { &d.trace } else { &d.log };
                let mut lines: Vec<String> = Vec::new();
                for l in text.lines() {
                    // (long lines wrapped to the panel; an empty one kept)
                    let w = crate::i18n::wrap(l, inner.saturating_sub(2));
                    if w.is_empty() {
                        lines.push(String::new());
                    }
                    lines.extend(w.into_iter().map(|x| x.to_string()));
                }
                let shown = if d.tab == 2 { body.saturating_sub(1) } else { body };
                let end = lines.len().saturating_sub(st.scroll.min(lines.len().saturating_sub(shown)));
                let start = end.saturating_sub(shown);
                let starts_at = if d.tab == 1 { 0 } else { shown - (end - start) };
                for k in 0..shown {
                    let r = first + k;
                    let line = if d.tab == 1 { lines.get(start + k) } else { k.checked_sub(starts_at).and_then(|j| lines.get(start + j)) };
                    row(&mut out, r, &format!(" {}", line.map(String::as_str).unwrap_or("")), &normal);
                }
                if d.tab == 2 {
                    let r = first + shown;
                    let prompt = "> ";
                    row(&mut out, r, &format!(" {}{}", prompt, st.line.text()), &title);
                    cursor = Some((r, left + 2 + width(prompt) + st.line.cursor_cols()));
                }
            }
            _ => {}
        }
        let help = match d.tab {
            0 => t!("Tab tab  ↑↓ choose  Enter edit  a add  d remove  r refresh  Esc close"),
            1 => t!("Tab tab  ↑↓ PgUp PgDn scroll  r refresh  Esc close"),
            _ => t!("Tab tab  Enter run  ↑↓ history  PgUp PgDn scroll  Esc close"),
        };
        row(&mut out, first + body, &format!(" {}", help), &helpc);
        row(&mut out, first + body + 1, &format!(" {}", note), &errc);
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", first + body + 2, left, frame, "─".repeat(inner));
        if let Some((r, c)) = cursor {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", r, c.min(left + inner));
        }
        write_all(&out);
    }
}

#[cfg(test)]
mod tests {
    use super::super::render_tests::{cases, check};
    use super::super::{capture, ColorMode, Tty};
    use super::PanelState;
    use crate::console::DebugView;

    /// The debug panel, every tab, in every pseudo-language
    #[test]
    fn debug_panel_in_every_script() {
        let d = DebugView {
            enabled: true,
            open: true,
            watches: vec!["FLAG:3".into(), "NAME:MASTER".into()],
            values: vec!["5".into(), "あなた".into()],
            trace: "*実行中の行\nファイル名:ERB/SYSTEM.ERB\n行番号:12 関数名:SHOW_SHOP\n\n*スタックトレース\nCALL :@SHOW_SHOP:ERB/SYSTEM.ERB:10行目\n".into(),
            log: "FLAG:3 * 2\n10\n".into(),
            ..Default::default()
        };
        for (lang, cols, rows) in cases() {
            crate::i18n::set_language(lang);
            let t = Tty::blank(None, cols, rows, ColorMode::True);
            for tab in 0..3 {
                let d = DebugView { tab, ..d.clone() };
                let out = capture(|| t.draw_debug_panel(&d, &PanelState::default(), ""));
                check(&format!("debug panel tab {}", tab), lang, cols, rows, &out);
            }
        }
        crate::i18n::set_language("en");
    }
}
