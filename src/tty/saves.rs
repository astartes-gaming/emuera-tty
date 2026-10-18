//! The save manager, opened from the settings overlay: the game's saves
//! with their comments, and per save copy / move to another slot, export
//! to a file, delete; for the whole folder back up, restore a backup and
//! import a file. The file work is in [`crate::savemgr`].

use super::{write_all, Key, Tty};
use crate::console::Console;
use crate::frontend::InputReq;
use crate::resources::Resources;
use crate::savemgr as sm;
use std::io::Write;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

/// What a list box returned.
enum Pick {
    Row(usize),
    Back,
    Quit,
}

/// `s` cut to `w` columns.
fn fit(s: &str, w: usize) -> (String, usize) {
    let mut t = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > w {
            break;
        }
        t.push(c);
        used += cw;
    }
    (t, used)
}

impl Tty {
    /// Run the save manager. Returns true if the player asked to quit.
    pub(super) fn save_manager(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> bool {
        let Some((game, dir)) = self.game.clone() else { return false };
        let root = sm::backup_root(&game).unwrap_or_else(|| dir.join("backups"));
        let mut sel = 0usize;
        let mut note = String::new();
        loop {
            let saves = sm::list(&dir);
            let mut rows: Vec<String> = saves.iter().map(|s| format!("{:<8} {:<16} {}", s.label(), sm::when(s.modified), s.comment)).collect();
            if rows.is_empty() {
                rows.push("(no saves yet)".into());
            }
            let first_action = rows.len();
            let actions = ["Back up all saves now", "Restore a backup…", "Import a save file…", "Back  (Esc)"];
            rows.extend(actions.iter().map(|a| a.to_string()));
            let help = if note.is_empty() { format!("Enter: save actions   saves in {}", dir.display()) } else { std::mem::take(&mut note) };
            let pick = self.list_box(con, res, req, "Saves", &rows, first_action, &mut sel, &help);
            let k = match pick {
                Pick::Quit => return true,
                Pick::Back => return false,
                Pick::Row(k) => k,
            };
            if k >= first_action {
                match k - first_action {
                    0 => {
                        note = match sm::backup_all(&dir, &root, "") {
                            Ok((p, n)) => format!("backed up {} files to {}", n, p.display()),
                            Err(e) => e,
                        }
                    }
                    1 => match self.choose_backup(con, res, req, &root) {
                        Err(true) => return true,
                        Err(false) => {}
                        Ok(b) => {
                            if self.confirm(con, res, req, &format!("Replace the saves with {}? (the current ones are backed up first)", b.display())) {
                                note = match sm::restore(&b, &dir, &root) {
                                    Ok(n) => format!("restored {} files; load one from the game's load menu", n),
                                    Err(e) => e,
                                };
                            }
                        }
                    },
                    2 => {
                        let Some(path) = self.prompt(con, res, req, "Import a save", "File to import:", "") else { continue };
                        let src = PathBuf::from(expand_home(path.trim()));
                        let global = src.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("global"));
                        let slot = if global {
                            None
                        } else {
                            let free = (0..1000).find(|n| !dir.join(sm::slot_name(*n)).exists()).unwrap_or(99);
                            match self.prompt(con, res, req, "Import a save", "Into slot:", &free.to_string()).and_then(|s| s.trim().parse().ok()) {
                                Some(n) => Some(n),
                                None => continue,
                            }
                        };
                        note = match sm::import(&src, &dir, slot, &root) {
                            Ok(p) => format!("imported as {}", p.display()),
                            Err(e) => e,
                        };
                    }
                    _ => return false,
                }
                continue;
            }
            let Some(s) = saves.get(k) else { continue };
            let what = format!("{}: {}", s.label(), s.comment);
            let opts: Vec<String> = ["Copy to slot…", "Move to slot…", "Export to a file…", "Delete", "Cancel"].iter().map(|x| x.to_string()).collect();
            let mut osel = 0;
            match self.list_box(con, res, req, &what, &opts, 0, &mut osel, "a deleted or overwritten save is kept in the backups") {
                Pick::Quit => return true,
                Pick::Back | Pick::Row(4) => {}
                Pick::Row(o @ (0 | 1)) => {
                    let free = (0..1000).find(|n| !dir.join(sm::slot_name(*n)).exists()).unwrap_or(99);
                    let Some(n) = self.prompt(con, res, req, &what, "To slot:", &free.to_string()).and_then(|t| t.trim().parse::<u32>().ok()) else { continue };
                    if dir.join(sm::slot_name(n)).exists() && !self.confirm(con, res, req, &format!("Slot {:02} has a save. Overwrite it?", n)) {
                        continue;
                    }
                    note = match sm::copy_to_slot(&s.path, &dir, n, &root, o == 1) {
                        Ok(_) => format!("{} {} to slot {:02}", if o == 1 { "moved" } else { "copied" }, s.label(), n),
                        Err(e) => e,
                    };
                }
                Pick::Row(2) => {
                    let def = sm::default_export(&game, s);
                    let Some(p) = self.prompt(con, res, req, &what, "Export to:", &def.display().to_string()) else { continue };
                    note = match sm::export(&s.path, Path::new(&expand_home(p.trim()))) {
                        Ok(p) => format!("exported to {}", p.display()),
                        Err(e) => e,
                    };
                }
                Pick::Row(3) => {
                    if self.confirm(con, res, req, &format!("Delete {}?", s.label())) {
                        note = match sm::delete(&s.path, &root) {
                            Ok(()) => format!("deleted {} (a copy is in {})", s.label(), root.display()),
                            Err(e) => e,
                        };
                    }
                }
                Pick::Row(_) => {}
            }
        }
    }

    /// Pick a backup folder: Ok(folder), Err(true) to quit, Err(false) back.
    fn choose_backup(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, root: &Path) -> Result<PathBuf, bool> {
        let bks = sm::backups(root);
        if bks.is_empty() {
            let mut s = 0;
            return match self.list_box(con, res, req, "Restore a backup", &["(no backups yet)".to_string()], 1, &mut s, &format!("backups go to {}", root.display())) {
                Pick::Quit => Err(true),
                _ => Err(false),
            };
        }
        let rows: Vec<String> = bks.iter().map(|(p, n)| format!("{}  ({} files)", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), n)).collect();
        let mut s = 0;
        match self.list_box(con, res, req, "Restore a backup", &rows, rows.len(), &mut s, &format!("in {}", root.display())) {
            Pick::Row(k) => Ok(bks[k].0.clone()),
            Pick::Quit => Err(true),
            Pick::Back => Err(false),
        }
    }

    fn confirm(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, q: &str) -> bool {
        let mut s = 1;
        matches!(self.list_box(con, res, req, q, &["Yes".to_string(), "No".to_string()], 0, &mut s, ""), Pick::Row(0))
    }

    /// The overlay frame: (left column, top row, inner width), 1-based.
    fn box_geo(&self, n_rows: usize) -> (usize, usize, usize) {
        let w = self.cols.saturating_sub(4).clamp(30, 110);
        let h = n_rows + 4;
        ((self.cols.saturating_sub(w)) / 2 + 1, (self.rows.saturating_sub(h)) / 2 + 1, w - 2)
    }

    /// Draw a boxed list: `title`, the rows (those from `first_action` on in
    /// the action colour), `help` at the bottom; scrolled to show `sel`.
    /// Returns the top row and the first row shown.
    fn draw_list(&self, title: &str, rows: &[String], first_action: usize, sel: usize, help: &str) -> (usize, usize) {
        let vis = rows.len().min(self.rows.saturating_sub(6).max(3));
        let start = sel.saturating_sub(vis.saturating_sub(1)).min(rows.len() - vis);
        let (left, top, inner) = self.box_geo(vis);
        let bg = (22, 32, 58);
        let frame = self.ov_style((150, 170, 210), bg, false);
        let tstyle = self.ov_style((255, 255, 255), bg, true);
        let normal = self.ov_style((235, 235, 235), bg, false);
        let action = self.ov_style((140, 220, 255), bg, false);
        let hilite = self.ov_style((0, 0, 0), (230, 230, 230), false);
        let helpc = self.ov_style((255, 220, 90), bg, false);
        let mut out: Vec<u8> = Vec::new();
        let line = |out: &mut Vec<u8>, row: usize, text: &str, style: &str| {
            let (t, used) = fit(text, inner);
            let _ = write!(out, "\x1b[{};{}H{}│{}{}{}{}│", row, left, frame, style, t, " ".repeat(inner - used), frame);
        };
        let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
        line(&mut out, top + 1, &format!(" {}", title), &tstyle);
        for (i, r) in rows.iter().enumerate().skip(start).take(vis) {
            let st = if i == sel { &hilite } else if i >= first_action { &action } else { &normal };
            line(&mut out, top + 2 + i - start, &format!("  {}", r), st);
        }
        line(&mut out, top + 2 + vis, &format!(" {}", if help.is_empty() { "↑↓ choose  Enter pick  Esc back" } else { help }), &helpc);
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 3 + vis, left, frame, "─".repeat(inner));
        write_all(&out);
        (top + 1, start)
    }

    #[allow(clippy::too_many_arguments)]
    fn list_box(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, rows: &[String], first_action: usize, sel: &mut usize, help: &str) -> Pick {
        let n = rows.len();
        if n == 0 {
            return Pick::Back;
        }
        *sel = (*sel).min(n - 1);
        loop {
            self.no_images = true;
            self.render(con, res, Some(req));
            let (top, start) = self.draw_list(title, rows, first_action, *sel, help);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    return Pick::Quit;
                }
                continue;
            };
            match k {
                Key::Up | Key::Char('k') => *sel = (*sel + n - 1) % n,
                Key::Down | Key::Char('j') => *sel = (*sel + 1) % n,
                Key::PageUp => *sel = sel.saturating_sub(10),
                Key::PageDown => *sel = (*sel + 10).min(n - 1),
                Key::Home => *sel = 0,
                Key::End => *sel = n - 1,
                Key::Esc | Key::Settings | Key::Char('q') | Key::Left | Key::Backspace => return Pick::Back,
                Key::CtrlC => return Pick::Quit,
                Key::Enter | Key::Right | Key::Char(' ') => return Pick::Row(*sel),
                Key::Mouse { btn, y, press: true, .. } => match btn & !(4 | 8 | 16) {
                    64 => *sel = (*sel + n - 1) % n,
                    65 => *sel = (*sel + 1) % n,
                    0 => {
                        let row = y as usize;
                        if row > top && row - top - 1 + start < n && row - top - 1 < self.rows {
                            *sel = row - top - 1 + start;
                            return Pick::Row(*sel);
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// Ask for a line of text in a box (Enter accepts, Esc cancels).
    fn prompt(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, label: &str, default: &str) -> Option<String> {
        let mut text = default.to_string();
        loop {
            self.no_images = true;
            self.render(con, res, Some(req));
            let (left, top, inner) = self.box_geo(2);
            let bg = (22, 32, 58);
            let frame = self.ov_style((150, 170, 210), bg, false);
            let tstyle = self.ov_style((255, 255, 255), bg, true);
            let field = self.ov_style((0, 0, 0), (230, 230, 230), false);
            let helpc = self.ov_style((255, 220, 90), bg, false);
            let mut out: Vec<u8> = Vec::new();
            let row = |out: &mut Vec<u8>, r: usize, t: &str, st: &str| {
                let (t, used) = fit(t, inner);
                let _ = write!(out, "\x1b[{};{}H{}│{}{}{}{}│", r, left, frame, st, t, " ".repeat(inner - used), frame);
            };
            let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
            row(&mut out, top + 1, &format!(" {}", title), &tstyle);
            row(&mut out, top + 2, &format!(" {}", label), &tstyle);
            // the end of the text, so the cursor end is visible
            let room = inner.saturating_sub(3);
            let mut shown: Vec<char> = Vec::new();
            let mut used = 0;
            for c in text.chars().rev() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > room {
                    break;
                }
                shown.push(c);
                used += cw;
            }
            let shown: String = shown.into_iter().rev().collect();
            row(&mut out, top + 3, &format!(" {}▏", shown), &field);
            row(&mut out, top + 4, " Enter accept  Esc cancel  Ctrl-U clear", &helpc);
            let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 5, left, frame, "─".repeat(inner));
            write_all(&out);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    return None;
                }
                continue;
            };
            match k {
                Key::Enter => return Some(text),
                Key::Esc | Key::CtrlC | Key::Settings => return None,
                Key::Backspace => {
                    text.pop();
                }
                Key::Char('\u{15}') => text.clear(),
                Key::Char(c) if !c.is_control() => text.push(c),
                _ => {}
            }
        }
    }
}

/// `~/x` → the home folder's x.
fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), crate::sys::home_dir()) {
        (Some(rest), Some(h)) => h.join(rest).display().to_string(),
        _ => p.to_string(),
    }
}
