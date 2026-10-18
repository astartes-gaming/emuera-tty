//! The game's settings page, opened from the settings overlay: the
//! options of Emuera's own settings window, read and written as Emuera
//! does (see [`crate::emuconfig`]).

use super::saves::Pick;
use super::Tty;
use crate::console::Console;
use crate::emuconfig::{normalize, Kind, Source, Values, ITEMS};
use crate::frontend::InputReq;
use crate::resources::Resources;

/// `s` padded with spaces to `w` columns (wide characters count two).
fn pad(s: &str, w: usize) -> String {
    let used: usize = s.chars().map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    format!("{}{}", s, " ".repeat(w.saturating_sub(used)))
}

impl Tty {
    /// Run the page. Returns (something changed, the player asked to quit).
    pub(super) fn game_settings(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> (bool, bool) {
        let Some((game, _)) = self.game.clone() else { return (false, false) };
        let mut vals = Values::read(&game);
        let (mut sel, mut note, mut changed) = (1usize, String::new(), false);
        loop {
            // group headings, then each setting: value, where it comes from
            let mut rows = Vec::new();
            let mut item_at: Vec<Option<usize>> = Vec::new();
            let mut group = "";
            for (i, it) in ITEMS.iter().enumerate() {
                if it.group != group {
                    group = it.group;
                    rows.push(format!("── {} ──", group));
                    item_at.push(None);
                }
                let (v, src) = vals.get(it);
                let from = match src {
                    Source::Emuera => "",
                    Source::Game => "game",
                    Source::Player => "changed",
                    Source::Fixed => "fixed",
                };
                rows.push(format!("{} {} {:<8}{}", pad(it.label, 46), pad(&v, 18), from, if it.here { "" } else { "Emuera.exe" }));
                item_at.push(Some(i));
            }
            let first_action = rows.len();
            rows.push("Put back the game's settings…".into());
            rows.push("Back  (Esc)".into());
            let help = if note.is_empty() { "Enter: change   saved in the game's emuera.config, which Emuera.exe reads too".to_string() } else { std::mem::take(&mut note) };
            let k = match self.list_box(con, res, req, "Game settings (Emuera's)", &rows, first_action, &mut sel, &help) {
                Pick::Quit => return (changed, true),
                Pick::Back => return (changed, false),
                Pick::Row(k) => k,
            };
            if k == first_action {
                if self.confirm(con, res, req, "Take your changes out of emuera.config (back to the game's settings)?") {
                    let mine: Vec<_> = ITEMS.iter().filter(|it| vals.get(it).1 == Source::Player).collect();
                    for it in mine {
                        if let Err(e) = vals.set(it, None) {
                            note = e;
                        }
                        changed = true;
                    }
                }
                continue;
            }
            if k > first_action {
                return (changed, false);
            }
            let Some(i) = item_at.get(k).copied().flatten() else { continue };
            let it = &ITEMS[i];
            let (v, src) = vals.get(it);
            if src == Source::Fixed {
                note = format!("{}: the game fixes it (CSV/_fixed.config)", it.label);
                continue;
            }
            let new = match it.kind {
                Kind::Bool => Some(if v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE") { "NO" } else { "YES" }.to_string()),
                _ => {
                    let what = match it.kind {
                        Kind::Int(lo, hi) => format!("A number from {} to {} (empty: the game's setting):", lo, hi),
                        Kind::Color => "R,G,B or #RRGGBB (empty: the game's setting):".to_string(),
                        _ => "Text (empty: the game's setting):".to_string(),
                    };
                    let Some(t) = self.prompt(con, res, req, it.label, &what, &v) else { continue };
                    if t.trim().is_empty() {
                        None
                    } else {
                        match normalize(it, &t) {
                            Ok(n) => Some(n),
                            Err(e) => {
                                note = format!("{}: {}", it.label, e);
                                continue;
                            }
                        }
                    }
                }
            };
            match vals.set(it, new.as_deref()) {
                Ok(()) => {
                    changed = true;
                    let (v, _) = vals.get(it);
                    note = format!("{}: {}{}", it.label, v, if it.here { "" } else { "  (used by Emuera.exe; emuera-tty doesn't)" });
                }
                Err(e) => note = e,
            }
        }
    }
}
