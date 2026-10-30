//! The game's settings page, opened from the settings overlay: a copy of
//! Emuera's settings window (its tabs, check boxes and fields) with the
//! settings that make sense in a terminal, read and written as Emuera
//! does (see [`crate::emuconfig`]).

use super::{write_all, Key, Tty};
use crate::console::Console;
use crate::emuconfig::{normalize, Item, Kind, Source, Values, ITEMS, TABS};
use crate::frontend::InputReq;
use crate::resources::Resources;
use std::io::Write;

/// Columns `s` takes (wide characters count two).
use crate::i18n::{fit, tr, width};

/// "R,G,B" as a colour.
fn rgb(v: &str) -> Option<(u8, u8, u8)> {
    let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 3).then(|| (n[0], n[1], n[2]))
}

/// Where the page's parts are on screen, for mouse clicks.
struct Geo {
    /// the tab bar's row and each tab's columns (1-based)
    tab_row: usize,
    tabs: Vec<(usize, usize)>,
    /// the first item row, and the first item shown
    items_top: usize,
    first: usize,
}

impl Tty {
    /// Run the page. Returns (something changed, the player asked to quit).
    pub(super) fn game_settings(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> (bool, bool) {
        let Some((game, _)) = self.game.clone() else { return (false, false) };
        let mut vals = Values::read(&game);
        let (mut tab, mut sel, mut changed) = (0usize, 0usize, false);
        let mut note = String::new();
        loop {
            let items: Vec<&Item> = ITEMS.iter().filter(|it| it.tab == TABS[tab]).collect();
            sel = sel.min(items.len().saturating_sub(1));
            self.no_images = true;
            self.render(con, res, Some(req));
            let geo = self.draw_game_settings(&vals, tab, &items, sel, &note);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    return (changed, true);
                }
                continue;
            };
            note.clear();
            // what to do with the selected setting: 1 change, +-1 step, 2 back to the game's
            let mut act = 0i64;
            match k {
                Key::Esc | Key::Settings | Key::Char('q') => return (changed, false),
                Key::CtrlC => return (changed, true),
                Key::Left | Key::Char('h') => {
                    tab = (tab + TABS.len() - 1) % TABS.len();
                    sel = 0;
                }
                Key::Right | Key::Char('l') | Key::Skip => {
                    tab = (tab + 1) % TABS.len();
                    sel = 0;
                }
                Key::Up | Key::Char('k') => sel = (sel + items.len() - 1) % items.len(),
                Key::Down | Key::Char('j') => sel = (sel + 1) % items.len(),
                Key::Enter | Key::Char(' ') => act = 1,
                Key::Char('+') => act = 10,
                Key::Char('-') => act = -10,
                Key::Char('d') | Key::Backspace => act = 2,
                Key::Mouse { btn, x, y, press: true } => match btn & !(4 | 8 | 16) {
                    64 => sel = (sel + items.len() - 1) % items.len(),
                    65 => sel = (sel + 1) % items.len(),
                    0 => {
                        let (x, y) = (x as usize, y as usize);
                        if y == geo.tab_row {
                            if let Some(t) = geo.tabs.iter().position(|&(a, b)| x >= a && x < b) {
                                tab = t;
                                sel = 0;
                            }
                        } else if y >= geo.items_top && y - geo.items_top + geo.first < items.len() {
                            sel = y - geo.items_top + geo.first;
                            act = 1;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if act == 0 {
                continue;
            }
            let it = items[sel];
            let (v, src) = vals.get(it);
            if src == Source::Fixed {
                note = t!("{setting}: the game fixes this one (CSV/_fixed.config)", setting = tr(it.label));
                continue;
            }
            let new: Option<String> = match (act, it.kind) {
                (2, _) => None,
                (_, Kind::Bool) => Some(if v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE") { "NO" } else { "YES" }.to_string()),
                (10 | -10, Kind::Int(lo, hi)) => Some((v.parse::<i64>().unwrap_or(lo) + act / 10).clamp(lo, hi).to_string()),
                (10 | -10, _) => continue,
                (_, kind) => {
                    let what = match kind {
                        Kind::Int(lo, hi) => t!("A number from {lo} to {hi} (empty: the game's setting):", lo = lo, hi = hi),
                        Kind::Color => t!("R,G,B or #RRGGBB (empty: the game's setting):"),
                        _ => t!("Text (empty: the game's setting):"),
                    };
                    let Some(t) = self.prompt(con, res, req, &tr(it.label), &what, &v) else { continue };
                    if t.trim().is_empty() {
                        None
                    } else {
                        match normalize(it, &t) {
                            Ok(n) => Some(n),
                            Err(e) => {
                                note = format!("{}: {}", tr(it.label), e);
                                continue;
                            }
                        }
                    }
                }
            };
            match vals.set(it, new.as_deref()) {
                Ok(()) => {
                    changed = true;
                    note = format!("{}: {}", tr(it.label), vals.get(it).0);
                }
                Err(e) => note = e,
            }
        }
    }

    /// Draw the page over the game.
    fn draw_game_settings(&self, vals: &Values, tab: usize, items: &[&Item], sel: usize, note: &str) -> Geo {
        self.redraw_all.set(true);
        let most = TABS.iter().map(|t| ITEMS.iter().filter(|it| it.tab == *t).count()).max().unwrap_or(1);
        let vis = most.min(self.rows.saturating_sub(10).max(3));
        let first = sel.saturating_sub(vis - 1).min(items.len().saturating_sub(vis));
        let w = self.cols.saturating_sub(4).clamp(40, 84);
        let inner = w - 2;
        let h = vis + 8;
        let left = self.cols.saturating_sub(w) / 2 + 1;
        let top = self.rows.saturating_sub(h) / 2 + 1;
        let bg = (22, 32, 58);
        let frame = self.ov_style((150, 170, 210), bg, false);
        let title = self.ov_style((255, 255, 255), bg, true);
        let normal = self.ov_style((235, 235, 235), bg, false);
        let dim = self.ov_style((130, 140, 165), bg, false);
        let hl = (230, 230, 230);
        let hilite = self.ov_style((0, 0, 0), hl, false);
        let tab_off = self.ov_style((170, 190, 225), bg, false);
        let helpc = self.ov_style((255, 220, 90), bg, false);
        let notec = self.ov_style((140, 220, 255), bg, false);
        let mut out: Vec<u8> = Vec::new();
        let row = |out: &mut Vec<u8>, r: usize, t: &str, st: &str| {
            let _ = write!(out, "\x1b[{};{}H{}│{}{}{}│", r, left, frame, st, fit(t, inner), frame);
        };
        let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
        row(&mut out, top + 1, &format!(" {}", t!("Game settings  (Emuera's settings window)")), &title);
        // the tab bar
        let mut tabs = Vec::new();
        let _ = write!(out, "\x1b[{};{}H{}│{} ", top + 2, left, frame, normal);
        let mut used = 1;
        for (i, t) in TABS.iter().enumerate() {
            let label = format!(" {} ", tr(t));
            let lw = width(&label);
            if used + lw + 1 > inner {
                break;
            }
            tabs.push((left + 1 + used, left + 1 + used + lw));
            let _ = write!(out, "{}{}{} ", if i == tab { &hilite } else { &tab_off }, label, normal);
            used += lw + 1;
        }
        let _ = write!(out, "{}{}│", " ".repeat(inner - used), frame);
        let _ = write!(out, "\x1b[{};{}H{}├{}┤", top + 3, left, frame, "─".repeat(inner));
        // the settings: a check box or a value, then the name
        let field = 20;
        for k in 0..vis {
            let r = top + 4 + k;
            let Some(it) = items.get(first + k) else {
                row(&mut out, r, "", &normal);
                continue;
            };
            let (v, src) = vals.get(it);
            let is_sel = first + k == sel;
            let base = if is_sel {
                &hilite
            } else if src == Source::Fixed {
                &dim
            } else {
                &normal
            };
            let row_bg = if is_sel { hl } else { bg };
            let from = match src {
                Source::Emuera => String::new(),
                Source::Game => tc!("setting source", "game"),
                Source::Player => tc!("setting source", "changed"),
                Source::Fixed => tc!("setting source", "fixed"),
            };
            let _ = write!(out, "\x1b[{};{}H{}│{} ", r, left, frame, base);
            let mut used = 1;
            match it.kind {
                Kind::Bool => {
                    let on = v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
                    let _ = write!(out, "[{}] ", if on { "x" } else { " " });
                    used += 4;
                }
                Kind::Color => {
                    // a swatch of the colour
                    let c = rgb(&v).unwrap_or((0, 0, 0));
                    let _ = write!(out, "[{}██{}{}] ", self.ov_style(c, row_bg, false), base, fit(&format!(" {}", v), field - 5));
                    used += field;
                }
                _ => {
                    let _ = write!(out, "[{}] ", fit(&v, field - 3));
                    used += field;
                }
            }
            let tag_w = 8;
            let name_w = inner.saturating_sub(used + tag_w + 1);
            let _ = write!(out, "{}{} {}{}", fit(&tr(it.label), name_w), if is_sel { "" } else { &dim }, fit(&from, tag_w), frame);
            let _ = write!(out, "│");
        }
        row(&mut out, top + 4 + vis, "", &normal);
        row(&mut out, top + 5 + vis, &format!(" {}", t!("←→ tab  ↑↓ choose  Enter change  +/- step  d game's value  Esc back")), &helpc);
        let saved_in = format!(" {}", t!("Saved in the game's emuera.config, which Emuera.exe reads too."));
        let n = if note.is_empty() { saved_in.as_str() } else { note };
        row(&mut out, top + 6 + vis, &format!(" {}", n.trim_start()), &notec);
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 7 + vis, left, frame, "─".repeat(inner));
        write_all(&out);
        Geo { tab_row: top + 2, tabs, items_top: top + 4, first }
    }
}

#[cfg(test)]
mod tests {
    use super::super::render_tests::{cases, check};
    use super::super::{capture, ColorMode, Tty};
    use crate::emuconfig::{Values, ITEMS, TABS};

    /// The game settings page, every tab, in every pseudo-language
    #[test]
    fn game_settings_in_every_script() {
        let vals = Values::read(std::path::Path::new("/nonexistent/emuera-tty-test-game"));
        for (lang, cols, rows) in cases() {
            crate::i18n::set_language(lang);
            let t = Tty::blank(None, cols, rows, ColorMode::True);
            for (tab, name) in TABS.iter().enumerate() {
                let items: Vec<_> = ITEMS.iter().filter(|it| it.tab == *name).collect();
                let out = capture(|| {
                    t.draw_game_settings(&vals, tab, &items, 0, "");
                });
                check(&format!("game settings tab {}", name), lang, cols, rows, &out);
            }
        }
        crate::i18n::set_language("en");
    }
}
