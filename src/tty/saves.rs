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
pub(super) enum Pick {
    Row(usize),
    Back,
    Quit,
}

use crate::i18n::cut as fit;

impl Tty {
    /// Run the save manager. Returns true if the player asked to quit.
    pub(super) fn save_manager(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> bool {
        let Some((game, dir)) = self.game.clone() else { return false };
        let root = sm::backup_root(&game).unwrap_or_else(|| dir.join("backups"));
        let mut sel = 0usize;
        let mut note = String::new();
        loop {
            let saves = sm::list(&dir);
            let mut rows: Vec<String> =
                saves.iter().map(|s| format!("{} {} {}", crate::i18n::pad(&s.label(), 8), crate::i18n::pad(&sm::when(s.modified), 16), s.comment)).collect();
            if rows.is_empty() {
                rows.push(t!("(no saves yet)"));
            }
            let first_action = rows.len();
            rows.extend([t!("Back up all saves now"), t!("Restore a backup…"), t!("Import a save file…"), t!("Back") + "  (Esc)"]);
            let help = if note.is_empty() { t!("Enter: save actions   saves in {dir}", dir = dir.display()) } else { std::mem::take(&mut note) };
            let pick = self.list_box(con, res, req, &t!("Saves"), &rows, first_action, &mut sel, &help);
            let k = match pick {
                Pick::Quit => return true,
                Pick::Back => return false,
                Pick::Row(k) => k,
            };
            if k >= first_action {
                match k - first_action {
                    0 => {
                        note = match sm::backup_all(&dir, &root, "") {
                            Ok((p, n)) => t!("backed up {n} files to {dir}", n = n, dir = p.display()),
                            Err(e) => e,
                        }
                    }
                    1 => match self.choose_backup(con, res, req, &root) {
                        Err(true) => return true,
                        Err(false) => {}
                        Ok(b) => {
                            if self.confirm(con, res, req, &t!("Replace the saves with {backup}? (the current ones are backed up first)", backup = b.display()))
                            {
                                note = match sm::restore(&b, &dir, &root) {
                                    Ok(n) => t!("restored {n} files; load one from the game's load menu", n = n),
                                    Err(e) => e,
                                };
                            }
                        }
                    },
                    2 => {
                        let Some(path) = self.prompt(con, res, req, &t!("Import a save"), &t!("File to import:"), "") else { continue };
                        let src = PathBuf::from(expand_home(path.trim()));
                        let global = src.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("global"));
                        let slot = if global {
                            None
                        } else {
                            let free = (0..1000).find(|n| !dir.join(sm::slot_name(*n)).exists()).unwrap_or(99);
                            match self.prompt(con, res, req, &t!("Import a save"), &t!("Into slot:"), &free.to_string()).and_then(|s| s.trim().parse().ok()) {
                                Some(n) => Some(n),
                                None => continue,
                            }
                        };
                        note = match sm::import(&src, &dir, slot, &root) {
                            Ok(p) => t!("imported as {file}", file = p.display()),
                            Err(e) => e,
                        };
                    }
                    _ => return false,
                }
                continue;
            }
            let Some(s) = saves.get(k) else { continue };
            let what = format!("{}: {}", s.label(), s.comment);
            let opts: Vec<String> = vec![t!("Copy to slot…"), t!("Move to slot…"), t!("Export to a file…"), t!("Delete"), t!("Cancel")];
            let mut osel = 0;
            match self.list_box(con, res, req, &what, &opts, 0, &mut osel, &t!("a deleted or overwritten save is kept in the backups")) {
                Pick::Quit => return true,
                Pick::Back | Pick::Row(4) => {}
                Pick::Row(o @ (0 | 1)) => {
                    let free = (0..1000).find(|n| !dir.join(sm::slot_name(*n)).exists()).unwrap_or(99);
                    let Some(n) = self.prompt(con, res, req, &what, &t!("To slot:"), &free.to_string()).and_then(|t| t.trim().parse::<u32>().ok()) else {
                        continue;
                    };
                    if dir.join(sm::slot_name(n)).exists() && !self.confirm(con, res, req, &t!("Slot {n} has a save. Overwrite it?", n = format!("{:02}", n))) {
                        continue;
                    }
                    note = match sm::copy_to_slot(&s.path, &dir, n, &root, o == 1) {
                        Ok(_) if o == 1 => t!("moved {save} to slot {n}", save = s.label(), n = format!("{:02}", n)),
                        Ok(_) => t!("copied {save} to slot {n}", save = s.label(), n = format!("{:02}", n)),
                        Err(e) => e,
                    };
                }
                Pick::Row(2) => {
                    let def = sm::default_export(&game, s);
                    let Some(p) = self.prompt(con, res, req, &what, &t!("Export to:"), &def.display().to_string()) else { continue };
                    note = match sm::export(&s.path, Path::new(&expand_home(p.trim()))) {
                        Ok(p) => t!("exported to {file}", file = p.display()),
                        Err(e) => e,
                    };
                }
                Pick::Row(3) => {
                    if self.confirm(con, res, req, &t!("Delete {save}?", save = s.label())) {
                        note = match sm::delete(&s.path, &root) {
                            Ok(()) => t!("deleted {save} (a copy is in {dir})", save = s.label(), dir = root.display()),
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
            return match self.list_box(
                con,
                res,
                req,
                &t!("Restore a backup"),
                &[t!("(no backups yet)")],
                1,
                &mut s,
                &t!("backups go to {dir}", dir = root.display()),
            ) {
                Pick::Quit => Err(true),
                _ => Err(false),
            };
        }
        let rows: Vec<String> = bks
            .iter()
            .map(|(p, n)| format!("{}  {}", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), t!("({n} files)", n = n)))
            .collect();
        let mut s = 0;
        match self.list_box(con, res, req, &t!("Restore a backup"), &rows, rows.len(), &mut s, &t!("in {dir}", dir = root.display())) {
            Pick::Row(k) => Ok(bks[k].0.clone()),
            Pick::Quit => Err(true),
            Pick::Back => Err(false),
        }
    }

    pub(super) fn confirm(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, q: &str) -> bool {
        let mut s = 1;
        // (the box hides pictures; put them back as they were)
        let images = self.no_images;
        let yes = matches!(self.list_box(con, res, req, q, &[t!("Yes"), t!("No")], 0, &mut s, ""), Pick::Row(0));
        self.no_images = images;
        yes
    }

    /// The overlay frame: (left column, top row, inner width), 1-based.
    pub(super) fn box_geo(&self, n_rows: usize) -> (usize, usize, usize) {
        let w = self.cols.saturating_sub(4).clamp(30, 110);
        let h = n_rows + 4;
        ((self.cols.saturating_sub(w)) / 2 + 1, (self.rows.saturating_sub(h)) / 2 + 1, w - 2)
    }

    /// Draw a boxed list: `title`, the rows (those from `first_action` on in
    /// the action colour), `help` at the bottom (one line per `\n`: wrapping
    /// is the caller's); scrolled to show `sel`.
    /// Returns the top row and the first row shown.
    fn draw_list(&self, title: &str, rows: &[String], first_action: usize, sel: usize, help: &str) -> (usize, usize) {
        self.redraw_all.set(true);
        let default_help = t!("↑↓ choose  Enter pick  Esc back");
        let help_lines: Vec<&str> = if help.is_empty() { vec![default_help.as_str()] } else { help.split('\n').collect() };
        let vis = rows.len().min(self.rows.saturating_sub(5 + help_lines.len()).max(3));
        let start = sel.saturating_sub(vis.saturating_sub(1)).min(rows.len() - vis);
        let (left, top, inner) = self.box_geo(vis + help_lines.len() - 1);
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
            let st = if i == sel {
                &hilite
            } else if i >= first_action {
                &action
            } else {
                &normal
            };
            line(&mut out, top + 2 + i - start, &format!("  {}", r), st);
        }
        for (k, l) in help_lines.iter().enumerate() {
            line(&mut out, top + 2 + vis + k, &format!(" {}", l), &helpc);
        }
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 2 + vis + help_lines.len(), left, frame, "─".repeat(inner));
        write_all(&out);
        (top + 1, start)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn list_box(
        &mut self,
        con: &mut Console,
        res: &mut Resources,
        req: &InputReq,
        title: &str,
        rows: &[String],
        first_action: usize,
        sel: &mut usize,
        help: &str,
    ) -> Pick {
        let r = self.list_box_in(con, res, req, title, rows, first_action, sel, help);
        self.box_closed.set(true);
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn list_box_in(
        &mut self,
        con: &mut Console,
        res: &mut Resources,
        req: &InputReq,
        title: &str,
        rows: &[String],
        first_action: usize,
        sel: &mut usize,
        help: &str,
    ) -> Pick {
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
    pub(super) fn prompt(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, label: &str, default: &str) -> Option<String> {
        let r = self.prompt_in(con, res, req, title, label, default);
        self.box_closed.set(true);
        r
    }

    fn prompt_in(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, label: &str, default: &str) -> Option<String> {
        let mut text = default.to_string();
        // the suggested text is replaced by typing, edited by Backspace
        let mut fresh = true;
        loop {
            self.no_images = true;
            self.render(con, res, Some(req));
            let (left, top, inner) = self.box_geo(2);
            self.redraw_all.set(true);
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
            row(&mut out, top + 4, &format!(" {}", t!("Enter accept  Esc cancel  Ctrl-U clear")), &helpc);
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
                    fresh = false;
                }
                Key::Char('\u{15}') => text.clear(),
                Key::Char(c) if !c.is_control() => {
                    if std::mem::take(&mut fresh) {
                        text.clear();
                    }
                    text.push(c);
                }
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
