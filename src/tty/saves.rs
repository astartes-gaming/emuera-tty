//! The save manager, opened from the settings overlay: the game's saves
//! with their comments, and per save copy / move to another slot, export
//! to a file, delete; for the whole folder back up, restore a backup and
//! import a file. Files and folders are picked in a browser like the
//! launcher's *Add a game* one. The file work is in [`crate::savemgr`].

use super::{write_all, Key, Tty};
use crate::console::Console;
use crate::frontend::InputReq;
use crate::resources::Resources;
use crate::savemgr as sm;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use unicode_width::UnicodeWidthChar;

/// What a list box returned.
pub(super) enum Pick {
    Row(usize),
    Back,
    Quit,
}

/// What the file browser is for.
#[derive(Clone, Copy, PartialEq)]
enum Want {
    /// an existing save file (import)
    Open,
    /// a folder of saves (restore)
    Folder,
    /// where to write a save: this folder, a file there, or a typed name
    SaveAs,
}

/// What the file browser returned.
enum Browsed {
    /// the folder shown (its "▶" row)
    Here(PathBuf),
    /// a save file, or a path typed after `/`
    File(PathBuf),
    Back,
    Quit,
}

/// A row of the file browser.
enum Row {
    Here,
    Up,
    Dir(PathBuf),
    File(PathBuf),
    /// only text
    Info,
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
                            if self.confirm(con, res, req, &t!("Replace the saves with {backup}? (the current ones are backed up first)", backup = tilde(&b))) {
                                note = match sm::restore(&b, &dir, &root) {
                                    Ok(n) => t!("restored {n} files; load one from the game's load menu", n = n),
                                    Err(e) => e,
                                };
                            }
                        }
                    },
                    2 => {
                        let start = self.browse_start();
                        let src = match self.browse(con, res, req, &t!("Import a save"), Want::Open, &start) {
                            Browsed::Quit => return true,
                            Browsed::File(p) => p,
                            _ => continue,
                        };
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
                            Ok(p) => t!("imported as {file}", file = tilde(&p)),
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
                    // ~/emuera-saves, made on export if it isn't there yet
                    let def = sm::default_export(&game, s);
                    let start = match &self.browse_dir {
                        Some(d) if d.is_dir() => d.clone(),
                        _ => def.parent().map_or_else(|| self.browse_start(), Path::to_path_buf),
                    };
                    let title = t!("Export {save}", save = s.label());
                    let to = match self.browse(con, res, req, &title, Want::SaveAs, &start) {
                        Browsed::Quit => return true,
                        Browsed::Back => continue,
                        Browsed::File(p) => p,
                        Browsed::Here(d) => {
                            let name = def.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let head = t!("Export {save} to {dir}", save = s.label(), dir = tilde(&d));
                            let Some(n) = self.prompt(con, res, req, &head, &t!("File name:"), &name) else { continue };
                            d.join(expand_home(n.trim()))
                        }
                    };
                    if to.is_file() && !self.confirm(con, res, req, &t!("{file} exists. Overwrite it?", file = tilde(&to))) {
                        continue;
                    }
                    note = match sm::export(&s.path, &to) {
                        Ok(p) => t!("exported to {file}", file = tilde(&p)),
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

    /// Pick a backup folder, or any folder of saves (another computer's
    /// backup, Emuera's `sav/`): Ok(folder), Err(true) to quit, Err(false) back.
    fn choose_backup(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, root: &Path) -> Result<PathBuf, bool> {
        let bks = sm::backups(root);
        let mut rows: Vec<String> = bks
            .iter()
            .map(|(p, n)| format!("{}  {}", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), t!("({n} files)", n = n)))
            .collect();
        let (mut s, help) = if bks.is_empty() {
            rows.push(t!("(no backups yet)"));
            (1, t!("backups go to {dir}", dir = root.display()))
        } else {
            (0, t!("in {dir}", dir = root.display()))
        };
        let other = rows.len();
        rows.push(t!("Another folder…"));
        match self.list_box(con, res, req, &t!("Restore a backup"), &rows, other, &mut s, &help) {
            Pick::Row(k) if k < bks.len() => Ok(bks[k].0.clone()),
            Pick::Row(k) if k == other => {
                let start = self.browse_start();
                match self.browse(con, res, req, &t!("Restore saves from a folder"), Want::Folder, &start) {
                    Browsed::Here(d) => Ok(d),
                    Browsed::Quit => Err(true),
                    _ => Err(false),
                }
            }
            Pick::Quit => Err(true),
            Pick::Row(_) | Pick::Back => Err(false),
        }
    }

    /// Where the file browser opens: where it was last used, else
    /// `~/emuera-saves`, else home.
    fn browse_start(&self) -> PathBuf {
        if let Some(d) = self.browse_dir.as_ref().filter(|d| d.is_dir()) {
            return d.clone();
        }
        let home = crate::sys::home_dir().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("/"));
        let saves = home.join("emuera-saves");
        if saves.is_dir() {
            saves
        } else {
            home
        }
    }

    /// Browse the folders for a save file or a folder, as the launcher's
    /// *Add a game* browser does for games. `/` types a path instead.
    fn browse(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, want: Want, start: &Path) -> Browsed {
        let r = self.browse_in(con, res, req, title, want, start);
        self.box_closed.set(true);
        r
    }

    fn browse_in(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, title: &str, want: Want, start: &Path) -> Browsed {
        let mut dir = tidy(start);
        let mut hidden = false;
        let mut note = String::new();
        // going up: the folder to select in its parent
        let mut came_from: Option<PathBuf> = None;
        let mut rows: Vec<(Row, String)> = Vec::new();
        let mut labels: Vec<String> = Vec::new();
        let mut sel = 0;
        let mut load = true;
        loop {
            if std::mem::take(&mut load) {
                rows = browse_rows(&dir, want, hidden);
                labels = rows.iter().map(|r| r.1.clone()).collect();
                let back_to = came_from.take();
                sel = rows
                    .iter()
                    .position(|r| matches!(&r.0, Row::Dir(d) if Some(d) == back_to.as_ref()))
                    .or_else(|| rows.iter().position(|r| !matches!(r.0, Row::Up | Row::Info)))
                    .unwrap_or(0);
            }
            let n = rows.len();
            self.no_images = true;
            self.render(con, res, Some(req));
            // the end of the folder's path, which is the part that tells
            let room = self.box_geo(0).2.saturating_sub(crate::i18n::width(title) + 3);
            let head = format!("{}  {}", title, fit_tail(&tilde(&dir), room));
            let keys = t!("Enter open/pick  ← up  ~ home  . hidden  / type a path  Esc back");
            let help = if note.is_empty() { keys } else { format!("{}\n{}", std::mem::take(&mut note), keys) };
            let (top, start) = self.draw_list(&head, &labels, n, sel, &help);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    return Browsed::Quit;
                }
                continue;
            };
            let mut go = false;
            match k {
                Key::Up | Key::Char('k') => sel = (sel + n - 1) % n,
                Key::Down | Key::Char('j') => sel = (sel + 1) % n,
                Key::PageUp => sel = sel.saturating_sub(10),
                Key::PageDown => sel = (sel + 10).min(n - 1),
                Key::Home => sel = 0,
                Key::End => sel = n - 1,
                Key::Esc | Key::Settings | Key::Char('q') => return Browsed::Back,
                Key::CtrlC => return Browsed::Quit,
                Key::Left | Key::Backspace => {
                    if let Some(p) = dir.parent() {
                        came_from = Some(dir.clone());
                        dir = p.to_path_buf();
                        load = true;
                    }
                }
                Key::Char('~') => {
                    if let Some(h) = crate::sys::home_dir() {
                        dir = h;
                        load = true;
                    }
                }
                Key::Char('.') => {
                    hidden = !hidden;
                    load = true;
                }
                Key::Char('/') => {
                    let mut def = tilde(&dir);
                    if !def.ends_with(std::path::MAIN_SEPARATOR) {
                        def.push(std::path::MAIN_SEPARATOR);
                    }
                    let Some(t) = self.prompt(con, res, req, title, &t!("Folder or file:"), &def) else { continue };
                    if t.trim().is_empty() {
                        continue;
                    }
                    let p = tidy(&dir.join(expand_home(t.trim())));
                    if p.is_dir() {
                        dir = p;
                        load = true;
                    } else if want == Want::SaveAs || (want == Want::Open && p.is_file()) {
                        self.browse_dir = p.parent().map(Path::to_path_buf);
                        return Browsed::File(p);
                    } else {
                        note = t!("no such folder: {dir}", dir = tilde(&p));
                    }
                }
                Key::Right => go = matches!(rows[sel].0, Row::Dir(_)),
                Key::Enter | Key::Char(' ') => go = true,
                Key::Mouse { btn, y, press: true, .. } => match btn & !(4 | 8 | 16) {
                    64 => sel = (sel + n - 1) % n,
                    65 => sel = (sel + 1) % n,
                    0 => {
                        let row = y as usize;
                        if row > top && row - top - 1 + start < n && row - top - 1 < self.rows {
                            sel = row - top - 1 + start;
                            go = true;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if !go {
                continue;
            }
            match &rows[sel].0 {
                Row::Here => {
                    self.browse_dir = Some(dir.clone());
                    return Browsed::Here(dir);
                }
                Row::Up => {
                    if let Some(p) = dir.parent() {
                        came_from = Some(dir.clone());
                        dir = p.to_path_buf();
                        load = true;
                    }
                }
                Row::Dir(d) => {
                    dir = d.clone();
                    load = true;
                }
                Row::File(f) if want != Want::Folder => {
                    self.browse_dir = Some(dir.clone());
                    return Browsed::File(f.clone());
                }
                Row::File(_) | Row::Info => {}
            }
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

/// The file browser's rows for `dir`: its "▶" row, `../`, the folders,
/// then the `.sav` files with their comments.
fn browse_rows(dir: &Path, want: Want, hidden: bool) -> Vec<(Row, String)> {
    let mut v = Vec::new();
    match want {
        Want::Open => {}
        Want::SaveAs => v.push((Row::Here, format!("▶ {}", t!("Export to this folder…")))),
        Want::Folder => match sm::list(dir).len() {
            0 => v.push((Row::Info, t!("(nothing to restore here: no saveNN.sav or global.sav)"))),
            1 => v.push((Row::Here, format!("▶ {}", t!("Restore the save in this folder")))),
            n => v.push((Row::Here, format!("▶ {}", t!("Restore the {n} saves in this folder", n = n)))),
        },
    }
    if dir.parent().is_some() {
        v.push((Row::Up, "../".into()));
    }
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    match std::fs::read_dir(dir) {
        Ok(rd) => {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !hidden && name.starts_with('.') {
                    continue;
                }
                let p = e.path();
                if p.is_dir() {
                    dirs.push((name, p));
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
                    files.push((name, p));
                }
            }
        }
        Err(_) if !dir.exists() => v.push((Row::Info, t!("(a new folder: made when the save is written)"))),
        Err(e) => v.push((Row::Info, t!("(can't read this folder: {error})", error = e))),
    }
    dirs.sort_by_key(|d| d.0.to_lowercase());
    files.sort_by_key(|f| f.0.to_lowercase());
    let empty = dirs.is_empty() && files.is_empty();
    v.extend(dirs.into_iter().map(|(name, p)| (Row::Dir(p), format!("{}/", name))));
    for (name, p) in files {
        let c = sm::comment_of(&p).unwrap_or_else(|| t!("(not an Emuera save)"));
        v.push((Row::File(p), format!("{}   {}", name, c)));
    }
    if empty && !v.iter().any(|r| matches!(r.0, Row::Info)) {
        v.push((Row::Info, t!("(no folders or save files here)")));
    }
    v
}

/// `p` with `.` and `..` worked out (links aren't followed).
fn tidy(p: &Path) -> PathBuf {
    let mut t = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                t.pop();
            }
            c => t.push(c),
        }
    }
    t
}

/// `p` with the home folder written `~`.
fn tilde(p: &Path) -> String {
    match crate::sys::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(r) if r.as_os_str().is_empty() => "~".into(),
        Some(r) => format!("~{}{}", std::path::MAIN_SEPARATOR, r.display()),
        None => p.display().to_string(),
    }
}

/// The end of `s` in `w` columns, with "…" in front when cut.
fn fit_tail(s: &str, w: usize) -> String {
    if crate::i18n::width(s) <= w {
        return s.to_string();
    }
    let mut t = Vec::new();
    let mut used = 1;
    for c in s.chars().rev() {
        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > w {
            break;
        }
        t.push(c);
        used += cw;
    }
    std::iter::once('…').chain(t.into_iter().rev()).collect()
}

/// `~/x` → the home folder's x.
fn expand_home(p: &str) -> String {
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), crate::sys::home_dir()) {
        (Some(rest), Some(h)) => h.join(rest).display().to_string(),
        _ => p.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::render_tests::{cases, check};
    use super::super::{capture, ColorMode, Tty};

    /// The save list and a confirmation box in every pseudo-language
    #[test]
    fn save_boxes_in_every_script() {
        for (lang, cols, rows) in cases() {
            crate::i18n::set_language(lang);
            let t = Tty::blank(None, cols, rows, ColorMode::True);
            let slot = t!("slot {n}", n = "01");
            let rows_text = vec![
                format!("{} {} {}", crate::i18n::pad(&slot, 8), crate::i18n::pad("2026/10/24 12:00", 16), "Day 3 - 博麗神社"),
                t!("Back up all saves now"),
                t!("Restore a backup…"),
                t!("Import a save file…"),
                t!("Back") + "  (Esc)",
            ];
            let help = t!("Enter: save actions   saves in {dir}", dir = "/home/player/games/eraTW/sav");
            let out = capture(|| {
                t.draw_list(&t!("Saves"), &rows_text, 1, 0, &help);
            });
            check("the save list", lang, cols, rows, &out);
            let q = t!("Replace the saves with {backup}? (the current ones are backed up first)", backup = "backups/2026-10-24");
            let out = capture(|| {
                t.draw_list(&q, &[t!("Yes"), t!("No")], 0, 0, "");
            });
            check("a confirmation", lang, cols, rows, &out);
        }
        crate::i18n::set_language("en");
    }

    /// The file browser in every pseudo-language, a long path in its title
    #[test]
    fn file_browser_in_every_script() {
        let d = std::env::temp_dir().join(format!("emuera-browse-{}", std::process::id())).join("a rather long folder name").join("emuera-saves");
        std::fs::create_dir_all(d.join("old pc")).unwrap();
        std::fs::write(d.join("eraTW-save03-20261031-083016.sav"), b"not a save").unwrap();
        for (lang, cols, rows) in cases() {
            crate::i18n::set_language(lang);
            let t = Tty::blank(None, cols, rows, ColorMode::True);
            for want in [super::Want::Open, super::Want::Folder, super::Want::SaveAs] {
                let labels: Vec<String> = super::browse_rows(&d, want, false).into_iter().map(|r| r.1).collect();
                let title = t!("Export {save}", save = t!("slot {n}", n = "03"));
                let room = t.box_geo(0).2.saturating_sub(crate::i18n::width(&title) + 3);
                let head = format!("{}  {}", title, super::fit_tail(&d.display().to_string(), room));
                let help = format!("{}\n{}", t!("no such folder: {dir}", dir = "~/x"), t!("Enter open/pick  ← up  ~ home  . hidden  / type a path  Esc back"));
                let out = capture(|| {
                    t.draw_list(&head, &labels, labels.len(), 0, &help);
                });
                check("the file browser", lang, cols, rows, &out);
            }
        }
        crate::i18n::set_language("en");
        let _ = std::fs::remove_dir_all(d.parent().unwrap().parent().unwrap());
    }
}
