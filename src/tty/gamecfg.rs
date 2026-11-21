//! The game's settings page, opened from the settings overlay: a copy of
//! Emuera's settings window (its tabs, check boxes, drop-down lists and
//! fields) with every setting it has, read and written as Emuera does
//! (see [`crate::emuconfig`]). A setting emuera-tty reads only while the
//! game loads, or doesn't use at all, says so when it is chosen.

use super::{write_all, Key, Tty};
use crate::console::Console;
use crate::emuconfig::{choice_label, normalize, step_choice, Effect, Item, Kind, Source, Store, Values, ITEMS, TABS};
use crate::frontend::InputReq;
use crate::resources::Resources;
use std::io::Write;

/// Columns `s` takes (wide characters count two).
use crate::i18n::{fit, tr, width, wrap};

/// "R,G,B" as a colour.
fn rgb(v: &str) -> Option<(u8, u8, u8)> {
    let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 3).then(|| (n[0], n[1], n[2]))
}

/// A setting's value as the page shows it: a choice by its label.
fn shown(it: &Item, value: &str) -> String {
    match it.kind {
        Kind::Choice(options) => choice_label(options, value),
        _ => value.to_string(),
    }
}

/// What the hint line says about a setting that doesn't simply apply at
/// once to emuera.config (None: the usual "Saved in…" line).
fn effect_hint(it: &Item) -> Option<String> {
    match (it.store, it.effect) {
        (Store::Json(_), _) => Some(t!("Saved in setting.json for Emuera.exe; emuera-tty doesn't use it.")),
        (_, Effect::Restart) => Some(t!("emuera-tty reads this while the game loads: it takes effect at the next start.")),
        (_, Effect::ExeOnly) => Some(t!("Only Emuera.exe uses this (and scripts, through GETCONFIG).")),
        (_, Effect::Now) => None,
    }
}

/// The first tab the bar shows: the leftmost that still lets the chosen
/// one fit in `room` columns (`widths`: each tab's, with its gap).
fn first_tab_shown(widths: &[usize], chosen: usize, room: usize) -> usize {
    let mut first = 0;
    while first < chosen && widths[first..=chosen].iter().sum::<usize>() > room {
        first += 1;
    }
    first
}

/// Where the page's parts are on screen, for mouse clicks.
struct Geo {
    /// the tab bar's row, and each tab shown: (its index, its columns,
    /// 1-based); the bar scrolls when the tabs don't all fit
    tab_row: usize,
    tabs: Vec<(usize, usize, usize)>,
    /// the first item row, and the item on each row shown (a long
    /// name wraps onto more rows)
    items_top: usize,
    row_items: Vec<usize>,
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
                            if let Some(&(t, _, _)) = geo.tabs.iter().find(|&&(_, a, b)| x >= a && x < b) {
                                tab = t;
                                sel = 0;
                            }
                        } else if let Some(&i) = y.checked_sub(geo.items_top).and_then(|r| geo.row_items.get(r)) {
                            sel = i;
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
                // a drop-down list: Enter and + take the next choice, - the one before
                (_, Kind::Choice(options)) => Some(step_choice(options, &v, if act == -10 { -1 } else { 1 })),
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
                    note = format!("{}: {}", tr(it.label), shown(it, &vals.get(it).0));
                }
                Err(e) => note = e,
            }
        }
    }

    /// Draw the page over the game.
    fn draw_game_settings(&self, vals: &Values, tab: usize, items: &[&Item], sel: usize, note: &str) -> Geo {
        self.redraw_all.set(true);
        let w = self.cols.saturating_sub(4).clamp(40, 84);
        let inner = w - 2;
        let left = self.cols.saturating_sub(w) / 2 + 1;

        // Pre-calculate wrapped label lines for each item
        let field = 20;
        let tag_w = 8;
        let name_w = inner.saturating_sub(field + tag_w + 2); // +2 for padding
        let mut item_rows: Vec<Vec<String>> = Vec::with_capacity(items.len());
        for it in items {
            let label = tr(it.label);
            let lines = if name_w > 0 { wrap(&label, name_w) } else { vec![label] };
            item_rows.push(lines);
        }

        // Convert selection index to row index (accounting for multi-row items)
        let mut row_of_item = vec![0; items.len()];
        let mut total_rows = 0;
        for (i, lines) in item_rows.iter().enumerate() {
            row_of_item[i] = total_rows;
            total_rows += lines.len().max(1);
        }
        let sel_row = row_of_item[sel.min(items.len() - 1)];

        // Calculate visible rows
        let max_vis_rows = self.rows.saturating_sub(10).max(3);
        let first_row = sel_row.saturating_sub(max_vis_rows - 1).min(total_rows.saturating_sub(max_vis_rows));
        let h = max_vis_rows + 8;
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
        // the tab bar: a cell each side for ‹ › when it scrolls, and the
        // tabs from the first that lets the chosen one show
        let mut tabs = Vec::new();
        let room = inner - 2;
        let labels: Vec<String> = TABS.iter().map(|t| format!(" {} ", tr(t))).collect();
        let widths: Vec<usize> = labels.iter().map(|l| width(l) + 1).collect();
        let first = first_tab_shown(&widths, tab, room);
        let more_left = if first > 0 { "‹" } else { " " };
        let _ = write!(out, "\x1b[{};{}H{}│{}{}", top + 2, left, frame, normal, more_left);
        let mut used = 0;
        let mut last = first;
        for (i, label) in labels.iter().enumerate().skip(first) {
            // a single tab wider than the bar is cut
            let label = if widths[i] > room && i == first { fit(label, room - 1) } else { label.clone() };
            let lw = width(&label);
            if used + lw + 1 > room {
                break;
            }
            tabs.push((i, left + 2 + used, left + 2 + used + lw));
            let _ = write!(out, "{}{}{} ", if i == tab { &hilite } else { &tab_off }, label, normal);
            used += lw + 1;
            last = i;
        }
        let more_right = if last + 1 < TABS.len() { "›" } else { " " };
        let _ = write!(out, "{}{}{}│", " ".repeat(room - used), more_right, frame);
        let _ = write!(out, "\x1b[{};{}H{}├{}┤", top + 3, left, frame, "─".repeat(inner));
        // the settings: a check box or a value, then the name (word-wrapped)
        let mut row_idx = 0;
        let mut row_items = Vec::new();
        for (item_idx, it) in items.iter().enumerate() {
            let lines = &item_rows[item_idx];
            let (v, src) = vals.get(it);
            let is_sel = item_idx == sel;
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

            // continuation lines start under the name: after the space
            // and the value field
            let label_start = field;

            for (line_idx, label_line) in lines.iter().enumerate() {
                if row_idx < first_row {
                    row_idx += 1;
                    continue;
                }
                if row_idx >= first_row + max_vis_rows {
                    break;
                }
                let r = top + 4 + row_idx - first_row;
                row_items.push(item_idx);
                let _ = write!(out, "\x1b[{};{}H{}│{} ", r, left, frame, base);
                let mut used = 1;
                if line_idx == 0 {
                    // First line: show the value field (all padded to field width)
                    match it.kind {
                        Kind::Bool => {
                            let on = v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
                            let val_str = format!("[{}]", if on { "x" } else { " " });
                            let _ = write!(out, "{}{} ", fit(&val_str, field - 1), base);
                            used += field;
                        }
                        Kind::Color => {
                            let c = rgb(&v).unwrap_or((0, 0, 0));
                            let _ = write!(out, "[{}██{}{}] ", self.ov_style(c, row_bg, false), base, fit(&format!(" {}", v), field - 5));
                            used += field;
                        }
                        _ => {
                            let _ = write!(out, "[{}] ", fit(&shown(it, &v), field - 3));
                            used += field;
                        }
                    }
                } else {
                    // Continuation lines: indent to align with label start on first line
                    let _ = write!(out, "{}", " ".repeat(label_start));
                    used += label_start;
                }
                // Label (with source tag on first line only)
                let label_w = inner.saturating_sub(used + tag_w + 1);
                if line_idx == 0 {
                    let _ = write!(out, "{}{} {}{}", fit(label_line, label_w), if is_sel { "" } else { &dim }, fit(&from, tag_w), frame);
                } else {
                    let _ = write!(out, "{}{}", fit(label_line, inner.saturating_sub(used)), frame);
                }
                let _ = write!(out, "│");
                row_idx += 1;
            }
        }
        // Fill remaining visible rows
        while row_idx < first_row + max_vis_rows {
            let r = top + 4 + row_idx - first_row;
            row(&mut out, r, "", &normal);
            row_idx += 1;
        }
        row(&mut out, top + 4 + max_vis_rows, "", &normal);
        row(&mut out, top + 5 + max_vis_rows, &format!(" {}", t!("←→ tab  ↑↓ choose  Enter change  +/- step  d game's value  Esc back")), &helpc);
        let saved_in = items.get(sel).and_then(|it| effect_hint(it)).unwrap_or_else(|| t!("Saved in the game's emuera.config, which Emuera.exe reads too."));
        let n = if note.is_empty() { saved_in.as_str() } else { note };
        row(&mut out, top + 6 + max_vis_rows, &format!(" {}", n.trim_start()), &notec);
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 7 + max_vis_rows, left, frame, "─".repeat(inner));
        write_all(&out);
        Geo { tab_row: top + 2, tabs, items_top: top + 4, row_items }
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
                let scr = check(&format!("game settings tab {}", name), lang, cols, rows, &out);
                // the bar scrolls so the chosen tab always shows
                if lang == "en" {
                    assert!(scr.text().contains(&format!(" {} ", name)), "tab {} not shown at {}x{}\n{}", name, cols, rows, scr.text());
                }
            }
        }
        crate::i18n::set_language("en");
    }
}
