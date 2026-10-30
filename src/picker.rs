//! The game launcher and first-run picker: a small full-screen list with
//! three pages. The **library** lists the games played or added (eraTW,
//! eraMegaten, eraPoPs…) to launch one; **Add a game** opens a directory
//! browser for an Emuera game folder (one with `CSV/` and `ERB/` inside)
//! or a downloaded archive of one (extracted on choice, see
//! `crate::archive`); **Settings** edits `crate::settings`.
//!
//! Library keys: Enter plays, `a` adds, `d` / Delete removes from the list
//! (files stay), `s` settings, q quits. Settings: ← / → change, Esc back.
//! Browser keys:
//! Keys: ↑/↓ (or k/j), PgUp/PgDn, Home/End move; Enter plays a marked game
//! or opens a folder; → opens a folder; ← / Backspace goes up; `~` jumps
//! home; `.` shows hidden folders; q / Esc cancels. Mouse: click selects,
//! clicking the selected entry again activates it, the wheel scrolls. Works
//! with the same mouse sources as the game screen (SGR / urxvt / X10
//! reports, or `/dev/input/mice` on the Linux console).

use crate::loader::{find_dir_ci, find_file_ci, read_text};
use std::io::Write;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;

/// Wraps a string into multiple lines at `width` display columns.
/// Handles ANSI escape codes (preserves them, doesn't count them in width).
/// Wraps at word boundaries where possible, falls back to character boundaries.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    // We need to track ANSI escape sequences to preserve them across wraps
    let mut pending_ansi = String::new();
    let mut in_ansi = false;
    let mut ansi_buffer = String::new();

    for c in s.chars() {
        if c == '\x1b' {
            // Start of ANSI escape sequence
            if in_ansi && !ansi_buffer.is_empty() {
                // Nested escape? Treat previous as literal
                current_line.push_str(&ansi_buffer);
                ansi_buffer.clear();
            }
            in_ansi = true;
            ansi_buffer.push(c);
            continue;
        }

        if in_ansi {
            ansi_buffer.push(c);
            if c.is_ascii_alphabetic() {
                // End of ANSI escape sequence
                in_ansi = false;
                // Preserve the ANSI code in the current line
                current_line.push_str(&ansi_buffer);
                // Also track it for potential re-emission on next line
                if ansi_buffer.starts_with("\x1b[") && ansi_buffer.ends_with('m') {
                    // This is an SGR code - track it for continuation
                    pending_ansi = ansi_buffer.clone();
                }
                ansi_buffer.clear();
            }
            continue;
        }

        let cw = UnicodeWidthChar::width(c).unwrap_or(0);
        if current_width + cw > width && !current_line.is_empty() {
            // Wrap: finish current line, start new one with pending ANSI
            lines.push(current_line);
            current_line = pending_ansi.clone();
            current_width = 0;
            // Recalculate width of pending ANSI (should be 0 since stripped)
            // But we need to account for any printable chars in pending_ansi (there shouldn't be any)
        }

        current_line.push(c);
        current_width += cw;
    }

    // Handle any remaining ANSI buffer
    if in_ansi && !ansi_buffer.is_empty() {
        current_line.push_str(&ansi_buffer);
    }

    if !current_line.is_empty() || lines.is_empty() {
        lines.push(current_line);
    }

    lines
}

/// `s` cut to `w` display cells (kept for backward compatibility where truncation is needed).
fn fit(s: &str, w: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|c| {
            used += UnicodeWidthChar::width(*c).unwrap_or(0);
            used <= w
        })
        .collect()
}

/// Is `dir` an Emuera game (has CSV/ and ERB/, any letter case)?
pub fn is_game_dir(dir: &Path) -> bool {
    find_dir_ci(dir, "CSV").is_some() && find_dir_ci(dir, "ERB").is_some()
}

/// The タイトル line of a game's GAMEBASE.csv, if any.
fn game_title(dir: &Path) -> Option<String> {
    let csv = find_dir_ci(dir, "CSV")?;
    let text = read_text(&find_file_ci(&csv, "GAMEBASE.CSV")?).ok()?;
    text.lines().find_map(|l| l.strip_prefix("タイトル,").map(|t| t.trim().to_string())).filter(|t| !t.is_empty())
}

/// One row of the listing.
struct Entry {
    label: String,
    /// Pre-computed wrapped lines for this entry (empty if not yet computed)
    wrapped: Vec<String>,
    kind: Kind,
}

impl Entry {
    /// Compute wrapped lines for this entry given the terminal width (a
    /// line break in the label starts a new line).
    fn wrap(&mut self, width: usize) {
        if self.wrapped.is_empty() || width == 0 {
            self.wrapped = self.label.split('\n').flat_map(|l| wrap_text(l, width.max(1))).collect();
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Library,
    Browse,
    Settings,
    /// the eraGames wiki's games (`Picker::wiki`)
    Download,
    /// one of them (`Picker::wiki_game`): its page's facts and repositories
    DownloadGame,
}

#[derive(Clone, PartialEq)]
enum Kind {
    /// library: a game to launch (`ok`: the folder is still there)
    Game {
        path: PathBuf,
        ok: bool,
    },
    AddGame,
    /// library: clone a game's git repository and add it
    CloneGame,
    /// library: the downloader (games listed on the eraGames wiki)
    Download,
    /// downloader: `Picker::wiki[i]`
    WikiGame(usize),
    /// downloader: the selected game's `repos[i]`, to clone
    WikiRepo(usize),
    /// text to read, not to choose (a game's facts)
    Info,
    /// downloader: log in to gitgud.io, or out
    Login,
    OpenSettings,
    /// settings: `crate::settings::ITEMS[i]`
    Setting(usize),
    Back,
    /// play the current folder (it is a game)
    PlayHere,
    Up,
    Dir {
        path: PathBuf,
        game: bool,
    },
    /// a .zip / .tar.gz / … that may hold a game
    Archive {
        path: PathBuf,
    },
}

/// What a key or click asks for.
enum Act {
    Move(isize),
    Page(isize),
    Top,
    Bottom,
    Activate,
    Open,
    Up,
    Home,
    ToggleHidden,
    Cancel,
    Click(usize, usize), // (x, y) screen coordinates
    /// a letter with no meaning in the browser (library / settings keys)
    Letter(char),
    Delete,
    None,
}

struct Picker {
    dir: PathBuf,
    entries: Vec<Entry>,
    /// the entry each screen line belongs to (long entries wrap)
    entry_line: Vec<usize>,
    sel: usize, // wrapped line index (visual selection)
    top: usize, // wrapped line index (scroll position)
    hidden: bool,
    note: String,
    mice: Option<crate::mice::Mice>,
    pending: Vec<u8>,
    mode: Mode,
    /// the browser was opened from the library to add a game
    adding: bool,
    /// the browser was opened from the library to update a missing game's folder
    updating: Option<PathBuf>,
    /// the full git output of the last update (`u` / `U`), shown with `g`
    /// for as long as its result line is on screen
    git_log: Option<String>,
    st: crate::settings::Settings,
    /// the downloader's games, fetched from the wiki when it first opens
    wiki: Vec<crate::download::WikiGame>,
    wiki_game: usize,
    wiki_info: crate::download::Details,
    /// the selected game's downloads, and whether each answered
    wiki_opts: Vec<crate::download::Opt>,
    /// logins typed in this session (never written anywhere by us)
    logins: Vec<Login>,
}

/// A user name and password (or access token) for a git host.
#[derive(Clone)]
struct Login {
    host: String,
    user: String,
    pass: String,
    /// kept in `logins_path()` for the next sessions
    remember: bool,
}

/// The host of most era games' repositories, which the downloader offers
/// to log in to.
const GITGUD: &str = "gitgud.io";

/// The launcher: the game library, with adding games and settings. Returns
/// the game to play (already remembered as the last one), or None.
/// `clone`: a git repository to clone into the library first (`--clone`).
pub fn launcher(clone: Option<&str>) -> Option<PathBuf> {
    let st = crate::settings::Settings::load();
    let start = browse_start(&st);
    run_screen(Mode::Library, &start, clone)
}

/// Is `s` a git repository's address rather than a folder: a URL git
/// fetches from (https://, ssh://, git://, file://…) or ssh's
/// `user@host:path`? Archives behind a URL aren't repositories.
pub fn is_git_url(s: &str) -> bool {
    let scheme = ["https://", "http://", "ssh://", "git://", "git+ssh://", "file://"].iter().any(|p| s.starts_with(p));
    let scp = s.split_once(':').is_some_and(|(host, path)| host.contains('@') && !host.contains('/') && !path.is_empty());
    (scheme || scp) && crate::archive::suffix(Path::new(s)).is_none()
}

/// Where the browser opens: next to the last game, else home.
fn browse_start(st: &crate::settings::Settings) -> PathBuf {
    st.last.as_ref().and_then(|g| g.parent().map(|p| p.to_path_buf())).or_else(crate::sys::home_dir).unwrap_or_else(|| PathBuf::from("."))
}

fn run_screen(mode: Mode, start: &Path, clone: Option<&str>) -> Option<PathBuf> {
    let raw_mode = crate::sys::RawMode::enter().ok()?;
    let term = std::env::var("TERM").unwrap_or_default();
    let is_vt = term == "linux" || term.starts_with("fbterm");
    let mice = if crate::mice::Mice::wanted(is_vt) { crate::mice::Mice::open().ok() } else { None };
    out(b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1015h\x1b[?1006h");
    let mut p = Picker {
        dir: start.to_path_buf(),
        entries: Vec::new(),
        entry_line: Vec::new(),
        sel: 0,
        top: 0,
        hidden: false,
        note: String::new(),
        mice,
        pending: Vec::new(),
        mode,
        adding: false,
        updating: None,
        git_log: None,
        st: crate::settings::Settings::load(),
        wiki: Vec::new(),
        wiki_game: 0,
        wiki_info: Default::default(),
        wiki_opts: Vec::new(),
        logins: load_logins(),
    };
    p.load();
    if let Some(url) = clone {
        p.draw();
        p.clone_direct(url);
    }
    let chosen = p.run();
    // (a text console has no alternate screen to go back from)
    if crate::tty::text_console(&term) {
        out(b"\x1b[0m\x1b[2J\x1b[H");
    }
    out(b"\x1b[?1006l\x1b[?1015l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
    raw_mode.restore();
    chosen
}

fn out(b: &[u8]) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(b);
    let _ = o.flush();
}

fn term_size() -> (usize, usize) {
    crate::sys::winsize().map_or((80, 24), |(c, r, _, _)| (c, r))
}

/// `s` cut to `w` display cells, ending in "…" when something was cut.
fn fit_dots(s: &str, w: usize) -> String {
    let width: usize = s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    if width <= w {
        s.to_string()
    } else {
        format!("{}…", fit(s, w.saturating_sub(1)))
    }
}

/// `s` cut or padded with spaces to exactly `w` display cells.
fn padded(s: &str, w: usize) -> String {
    let f = fit(s, w);
    let used: usize = f.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
    f + &" ".repeat(w.saturating_sub(used))
}

/// A checkout's folder name, for the gauge's title.
fn repo_title(root: &Path) -> String {
    root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// What a running git command reports to the launcher's gauge.
enum Step<'a> {
    /// a command starts: what it does
    Start(&'a str),
    /// its latest progress line
    Line(&'a str),
    /// 100 ms went by without output
    Quiet,
}

/// A box over the launcher's screen showing a long job (update, clone,
/// extraction), drawn the way Debian's installer draws its gauges: grey, with
/// a shadow, the bar filling in red. Until a percentage is known a block
/// sweeps along the bar instead, and a spinner turns while the job works.
/// Of several jobs (update all), the bar covers them all.
struct Gauge {
    title: String,
    /// the bottom line (how to cancel), may be empty
    hint: String,
    text: String,
    pct: Option<u32>,
    /// (this job, of how many)
    job: (usize, usize),
    born: std::time::Instant,
    drawn: std::time::Instant,
}

impl Gauge {
    fn new(title: &str, hint: &str) -> Gauge {
        let now = std::time::Instant::now();
        let mut g = Gauge { title: title.into(), hint: hint.into(), text: String::new(), pct: None, job: (0, 1), born: now, drawn: now };
        g.draw();
        g
    }

    /// Job `i` (from 0) of `n` starts, called `title`.
    fn job(&mut self, i: usize, n: usize, title: &str) {
        self.job = (i, n.max(1));
        self.title = title.into();
        self.step(Step::Start(""));
    }

    fn step(&mut self, s: Step) {
        match s {
            Step::Start(what) => {
                self.text = what.into();
                self.pct = None;
                self.draw();
            }
            Step::Line(l) => self.show(l, git_percent(l)),
            Step::Quiet => {
                if self.drawn.elapsed().as_millis() >= 100 {
                    self.draw();
                }
            }
        }
    }

    /// The job's latest news and, when known, how far along it is in percent
    /// (a line without one keeps the last).
    fn show(&mut self, text: &str, pct: Option<u32>) {
        let moved = pct.is_some() && pct != self.pct;
        self.text = text.chars().filter(|c| !c.is_control()).collect();
        if pct.is_some() {
            self.pct = pct;
        }
        if moved || self.drawn.elapsed().as_millis() >= 80 {
            self.draw();
        }
    }

    fn draw(&mut self) {
        self.drawn = std::time::Instant::now();
        let frame = self.born.elapsed().as_millis() as usize / 100;
        let spin = ['|', '/', '-', '\\'][frame % 4];
        let (i, n) = (self.job.0 as u32, self.job.1 as u32);
        // past the first of several jobs the bar is never empty
        let pct = match self.pct {
            Some(p) => Some((i * 100 + p.min(100)) / n),
            None if i > 0 => Some(i * 100 / n),
            None => None,
        };
        let (cols, rows) = term_size();
        let w = cols.saturating_sub(12).min(72);
        let h = if self.hint.is_empty() { 7 } else { 8 };
        if w < 30 || rows < h + 2 {
            // no room for a box: the status line
            let p = pct.map_or(String::new(), |p| format!(" {}%", p));
            let line = format!("{} {}: {}{}", spin, self.title, self.text, p);
            out(format!("\x1b[{};1H\x1b[0m\x1b[2K{}", rows.saturating_sub(1).max(1), fit(&line, cols)).as_bytes());
            return;
        }
        let inner = w - 2;
        // centred with its shadow (2 columns right, 1 row below)
        let (x, y) = ((cols - w - 2) / 2 + 1, (rows - h - 1) / 2 + 1);
        let face = "\x1b[0;30;47m";
        let shadow = "\x1b[0;40m  ";
        let mut s = String::new();
        let mut row = |r: usize, body: &str, shade: bool| {
            s += &format!("\x1b[{};{}H{}{}", y + r, x, face, body);
            if shade {
                s += shadow;
            }
        };
        let title = fit(&self.title, inner.saturating_sub(4));
        let tw: usize = title.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
        let left = (inner - tw - 2) / 2;
        row(0, &format!("┌{} {} {}┐", "─".repeat(left), title, "─".repeat(inner - tw - 2 - left)), false);
        let blank = format!("│{}│", " ".repeat(inner));
        row(1, &blank, true);
        row(2, &format!("│ {} {} │", spin, padded(&self.text, inner - 4)), true);
        row(3, &blank, true);
        let bw = inner - 7;
        let bar = match pct {
            Some(p) => {
                let full = bw * p.min(100) as usize / 100;
                format!("\x1b[41m{}\x1b[44m{}{} {:>3}%", " ".repeat(full), " ".repeat(bw - full), face, p.min(100))
            }
            None => {
                // a block bouncing between the ends
                let len = (bw / 5).max(2);
                let travel = bw - len;
                let t = frame % (2 * travel).max(1);
                let at = if t <= travel { t } else { 2 * travel - t };
                format!("\x1b[44m{}\x1b[41m{}\x1b[44m{}{}     ", " ".repeat(at), " ".repeat(len), " ".repeat(travel - at), face)
            }
        };
        row(4, &format!("│ {} │", bar), true);
        row(5, &blank, true);
        if !self.hint.is_empty() {
            let hint = fit(&self.hint, inner - 2);
            let hw: usize = hint.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            let l = (inner - hw) / 2;
            row(6, &format!("│{}{}{}│", " ".repeat(l), hint, " ".repeat(inner - hw - l)), true);
        }
        row(h - 1, &format!("└{}┘", "─".repeat(inner)), true);
        s += &format!("\x1b[{};{}H{}{}\x1b[0m", y + h, x + 2, shadow, " ".repeat(w - 2));
        out(s.as_bytes());
    }
}

impl Picker {
    /// List the current page.
    fn load(&mut self) {
        self.entries.clear();
        self.entry_line.clear();
        match self.mode {
            Mode::Library => {
                for g in self.st.library.clone() {
                    let ok = is_game_dir(&g);
                    let title = if ok { game_title(&g).unwrap_or_else(|| "Emuera game".into()) } else { "(folder missing)".into() };
                    let last = if self.st.last.as_ref() == Some(&g) { "  · last played" } else { "" };
                    // "· git <branch> @ <short hash>" (just "· git" if HEAD can't be read)
                    let git = match git_root(&g).filter(|_| ok) {
                        Some(root) => match git_head(&root) {
                            Some(h) => format!("  · git {}", h),
                            None => "  · git".to_string(),
                        },
                        None => String::new(),
                    };
                    // a game added from here (cloned or picked) and not played yet: "★ (NEW) Title" (gold in `draw`)
                    let new = if self.st.fresh.contains(&g) { "(NEW) " } else { "" };
                    self.entries.push(Entry {
                        label: format!("★ {}{}   {}{}{}", new, title, g.display(), last, git),
                        wrapped: Vec::new(),
                        kind: Kind::Game { path: g, ok },
                    });
                }
                self.entries.push(Entry { label: "＋ Add a game (folder or archive)…".into(), wrapped: Vec::new(), kind: Kind::AddGame });
                self.entries.push(Entry { label: "↓ Clone a game from a git repository…".into(), wrapped: Vec::new(), kind: Kind::CloneGame });
                self.entries.push(Entry { label: "⇩ Download a game (from the eraGames wiki)…".into(), wrapped: Vec::new(), kind: Kind::Download });
                self.entries.push(Entry { label: "⚙ Settings".into(), wrapped: Vec::new(), kind: Kind::OpenSettings });
                self.rebuild_entry_line();
                let entry_sel =
                    self.entries.iter().position(|e| matches!(&e.kind, Kind::Game { path, .. } if self.st.last.as_ref() == Some(path))).unwrap_or(0);
                self.sel = self.first_line(entry_sel);
                self.top = 0;
                return;
            }
            Mode::Settings => {
                for (i, it) in crate::settings::ITEMS.iter().enumerate().filter(|(_, it)| self.st.shown(it)) {
                    let v = self.st.get(it.key);
                    let restart = if it.live { "" } else { "  (next start)" };
                    self.entries.push(Entry { label: format!("{:<18} ‹ {} ›{}", it.label, v, restart), wrapped: Vec::new(), kind: Kind::Setting(i) });
                }
                self.entries.push(Entry { label: "← Back".into(), wrapped: Vec::new(), kind: Kind::Back });
                self.rebuild_entry_line();
                self.sel = self.sel.min(self.entry_line.len().saturating_sub(1));
                return;
            }
            Mode::Download => {
                let login = Entry { label: self.login_label(), wrapped: Vec::new(), kind: Kind::Login };
                self.entries.push(login);
                for (i, g) in self.wiki.iter().enumerate() {
                    let more = match g.repos.len() {
                        1 => String::new(),
                        n => format!(" (+{} more)", n - 1),
                    };
                    let first = g.repos.first().map_or(String::new(), repo_label);
                    // a second line: what the game is (cut to the screen)
                    let about = match g.info.summary() {
                        a if a.is_empty() => String::new(),
                        a => format!("\n    {}", fit_dots(&a, term_size().0.saturating_sub(6))),
                    };
                    self.entries.push(Entry { label: format!("⇩ {}   {}{}{}", g.title, first, more, about), wrapped: Vec::new(), kind: Kind::WikiGame(i) });
                }
                self.entries.push(Entry { label: "← Back".into(), wrapped: Vec::new(), kind: Kind::Back });
                self.rebuild_entry_line();
                // (the games come after the login entry)
                self.sel = self.first_line((self.wiki_game + 1).min(self.entries.len().saturating_sub(1)));
                self.top = 0;
                return;
            }
            Mode::DownloadGame => {
                let info = |label: String| Entry { label, wrapped: Vec::new(), kind: Kind::Info };
                for (k, v) in &self.wiki_info.facts {
                    self.entries.push(info(format!("  {}: {}", k, v)));
                }
                if !self.wiki_info.about.is_empty() {
                    self.entries.push(info(format!("  {}", self.wiki_info.about)));
                }
                if let Some(g) = self.wiki.get(self.wiki_game) {
                    self.entries.push(info(format!("  {}", crate::download::page_link(&g.title))));
                    self.entries.push(info(String::new()));
                    use crate::download::Access;
                    let gated = self.wiki_opts.iter().any(|o| o.access == Access::Login);
                    if gated || g.repos.iter().any(|r| url_host(&r.url).as_deref() == Some(GITGUD)) && self.login_for(&format!("https://{}/", GITGUD)).is_some()
                    {
                        self.entries.push(Entry { label: self.login_label(), wrapped: Vec::new(), kind: Kind::Login });
                    }
                    for (i, o) in self.wiki_opts.iter().enumerate() {
                        let note = match o.access {
                            Access::Login => "  (asks for a gitgud.io login, or is gone)".to_string(),
                            Access::NoBranch => "  (this branch is gone)".to_string(),
                            _ if !o.note.is_empty() => format!("  ({})", o.note),
                            _ => String::new(),
                        };
                        let label = format!("⇩ Download {}{}", repo_label(&o.repo), note);
                        self.entries.push(Entry { label, wrapped: Vec::new(), kind: Kind::WikiRepo(i) });
                    }
                }
                self.entries.push(Entry { label: "← Back".into(), wrapped: Vec::new(), kind: Kind::Back });
                self.rebuild_entry_line();
                let first = self.entries.iter().position(|e| matches!(e.kind, Kind::WikiRepo(_))).unwrap_or(0);
                self.sel = self.first_line(first);
                self.top = 0;
                return;
            }
            Mode::Browse => {}
        }
        if is_game_dir(&self.dir) {
            let t = game_title(&self.dir).unwrap_or_else(|| "this game".into());
            self.entries.push(Entry { label: format!("▶ Play {}", t), wrapped: Vec::new(), kind: Kind::PlayHere });
        }
        if self.dir.parent().is_some() {
            self.entries.push(Entry { label: "../".into(), wrapped: Vec::new(), kind: Kind::Up });
        }
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&self.dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        dirs.retain(|d| self.hidden || !d.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
        dirs.sort_by_key(|d| d.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        for d in dirs {
            let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let game = is_game_dir(&d);
            let label = if game { format!("★ {}/   ({})", name, game_title(&d).unwrap_or_else(|| "Emuera game".into())) } else { format!("  {}/", name) };
            self.entries.push(Entry { label, wrapped: Vec::new(), kind: Kind::Dir { path: d, game } });
        }
        let mut archives: Vec<PathBuf> =
            std::fs::read_dir(&self.dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| crate::archive::is_archive(p)).collect()).unwrap_or_default();
        archives.retain(|a| self.hidden || !a.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
        archives.sort_by_key(|a| a.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        for a in archives {
            let name = a.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = std::fs::metadata(&a).map(|m| m.len()).unwrap_or(0);
            let size = if bytes >= 1 << 20 { format!("{:.0} MB", bytes as f64 / 1048576.0) } else { format!("{} KB", bytes.div_ceil(1024)) };
            self.entries.push(Entry { label: format!("▣ {}   [archive, {}]", name, size), wrapped: Vec::new(), kind: Kind::Archive { path: a } });
        }
        // start on "Play" or the first folder rather than on ../
        let entry_sel = self.entries.iter().position(|e| e.kind != Kind::Up).unwrap_or(0);
        self.rebuild_entry_line();
        self.sel = self.first_line(entry_sel);
        self.top = 0;
    }

    /// Rebuild the entry_line mapping after entries change.
    fn rebuild_entry_line(&mut self) {
        self.entry_line.clear();
        let (cols, _) = term_size();
        let width = cols.saturating_sub(1);
        for (i, e) in self.entries.iter_mut().enumerate() {
            e.wrap(width);
            for _ in 0..e.wrapped.len() {
                self.entry_line.push(i);
            }
        }
    }

    /// The first screen line of entry `i`.
    fn first_line(&self, i: usize) -> usize {
        self.entry_line.iter().position(|&e| e == i).unwrap_or(0)
    }

    /// Select `d` entries further on (back when negative), passing over
    /// those that are only text.
    fn move_sel(&mut self, d: isize) {
        let Some(&cur) = self.entry_line.get(self.sel) else { return };
        let mut at = cur;
        let mut left = d.unsigned_abs();
        let mut i = cur as isize;
        while left > 0 {
            i += d.signum();
            match self.entries.get(i as usize) {
                Some(e) if i >= 0 => {
                    if e.kind != Kind::Info {
                        at = i as usize;
                        left -= 1;
                    }
                }
                _ => break,
            }
        }
        self.sel = self.first_line(at);
    }

    fn cd(&mut self, to: PathBuf) {
        let from = self.dir.clone();
        self.dir = to;
        self.load();
        // coming back up: keep the folder we came from selected
        if let Some(i) = self.entries.iter().position(|e| matches!(&e.kind, Kind::Dir { path, .. } if *path == from)) {
            self.sel = self.first_line(i);
        }
    }

    /// Rows available for the list (header 2 lines, footer 2).
    fn list_rows(&self) -> usize {
        term_size().1.saturating_sub(4).max(1)
    }

    fn draw(&mut self) {
        let (cols, _) = term_size();
        let rows = self.list_rows();
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + rows {
            self.top = self.sel + 1 - rows;
        }
        let mut s = String::from("\x1b[H\x1b[0m\x1b[2J");
        match self.mode {
            Mode::Library => {
                s += &format!(
                    "\x1b[1memuera-tty — your games\x1b[0m\r\n\x1b[2m{}\x1b[0m\r\n",
                    fit("Enter to play · a add a game · u update (git) · U update all · d remove from the list · s settings", cols)
                )
            }
            Mode::Settings => {
                s += &format!(
                    "\x1b[1mSettings\x1b[0m\r\n\x1b[2m{}\x1b[0m\r\n",
                    fit(&crate::settings::config_path().map(|p| p.display().to_string()).unwrap_or_default(), cols)
                )
            }
            Mode::Browse => {
                s += &format!(
                    "\x1b[1mChoose an Emuera game folder or archive\x1b[0m  (★ = game, ▣ = archive)\r\n\x1b[2m{}\x1b[0m\r\n",
                    fit(&self.dir.display().to_string(), cols)
                )
            }
            Mode::Download => {
                s += &format!(
                    "\x1b[1mDownload a game\x1b[0m  (the games of the eraGames wiki)\r\n\x1b[2m{}\x1b[0m\r\n",
                    fit(&format!("{} · each is cloned from its git repository, latest version only", crate::download::WIKI), cols)
                )
            }
            Mode::DownloadGame => {
                let title = self.wiki.get(self.wiki_game).map_or("", |g| g.title.as_str());
                s += &format!(
                    "\x1b[1m{}\x1b[0m\r\n\x1b[2m{}\x1b[0m\r\n",
                    fit(title, cols),
                    fit(&format!("cloned into {} (latest version only; u updates it)", self.clone_parent().display()), cols)
                )
            }
        }

        // Render wrapped lines
        let mut line_idx = 0;
        let sel_entry = self.entry_line.get(self.sel).copied();
        for (ei, e) in self.entries.iter().enumerate() {
            let selected = sel_entry == Some(ei);
            for (li, wrapped_line) in e.wrapped.iter().enumerate() {
                if line_idx < self.top {
                    line_idx += 1;
                    continue;
                }
                if line_idx >= self.top + rows {
                    break;
                }

                let pad = cols.saturating_sub(wrapped_line.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>() + 1);
                let color = match e.kind {
                    Kind::Game { ok: false, .. } => "\x1b[2m",
                    Kind::PlayHere | Kind::Dir { game: true, .. } | Kind::Game { .. } => "\x1b[33m",
                    Kind::AddGame | Kind::CloneGame | Kind::Download | Kind::OpenSettings => "\x1b[36m",
                    // (a game's description, under it, dimmed)
                    Kind::WikiGame(_) if li > 0 => "\x1b[2m",
                    Kind::WikiGame(_) | Kind::WikiRepo(_) => "\x1b[33m",
                    Kind::Login => "\x1b[36m",
                    Kind::Archive { .. } => "\x1b[36m",
                    _ => "",
                };

                // Apply special coloring for game entries
                let line = if matches!(e.kind, Kind::Game { .. }) {
                    let back = format!("\x1b[0m{}{}", color, if selected { "\x1b[7m" } else { "" });
                    let tint = |l: String, tag: &str, code: &str| match l.rfind(tag) {
                        Some(at) => format!("{}{}{}{}{}", &l[..at], code, tag, back, &l[at + tag.len()..]),
                        None => l,
                    };
                    // (the Hurd's console has 8 colours and reads 38;5 as blink)
                    let (sky, gold) = if std::env::var("TERM").as_deref() == Ok("hurd") { ("36", "33") } else { ("38;5;117", "38;5;220") };
                    let git_tail = |l: String| match l.rfind("· git") {
                        Some(at) => format!("{}\x1b[{}m{}{}", &l[..at], sky, &l[at..], back),
                        None => l,
                    };
                    let line = match wrapped_line.strip_prefix("★ (NEW)") {
                        Some(rest) => format!("★ \x1b[1;{}m(NEW){}{}", gold, back, rest),
                        None => wrapped_line.clone(),
                    };
                    git_tail(tint(line, "· last played", "\x1b[32m"))
                } else {
                    wrapped_line.clone()
                };

                if selected {
                    s += &format!("{}\x1b[7m {}{}\x1b[0m\r\n", color, line, " ".repeat(pad));
                } else {
                    s += &format!("{} {}\x1b[0m\r\n", color, line);
                }
                line_idx += 1;
            }
            if line_idx >= self.top + rows {
                break;
            }
        }
        if self.entries.is_empty() {
            s += "  (empty)\r\n";
        }
        let help = match self.mode {
            // right after an update, `g` is the full git output (not "top")
            Mode::Library if self.git_log.is_some() => "g git output  ↑↓ move  Enter play  a add  u update  U update all  d remove  s settings  q quit",
            Mode::Library => "↑↓ move  Enter play  a add  u update  U update all  d remove  s settings  q quit",
            Mode::Settings => "↑↓ move  ←/→ or Enter change  Esc back",
            Mode::Download => "↑↓ move  Enter details  Esc back",
            Mode::DownloadGame => "↑↓ move  Enter download  Esc back",
            Mode::Browse if self.adding => "↑↓ move  Enter add/open  → open  ← up  ~ home  . hidden  Esc back",
            Mode::Browse => "↑↓ move  Enter play/open  → open  ← up  ~ home  . hidden  q quit",
        };
        // the selected setting's explanation - find which entry the selected line belongs to
        if self.mode == Mode::Settings && !self.entry_line.is_empty() && self.sel < self.entry_line.len() {
            let entry_idx = self.entry_line[self.sel];
            if let Some(Entry { kind: Kind::Setting(i), .. }) = self.entries.get(entry_idx) {
                if self.note.is_empty() {
                    self.note = crate::settings::ITEMS[*i].help.to_string();
                }
            }
        }
        let (_, trows) = term_size();
        s += &format!("\x1b[{};1H\x1b[0m{}\x1b[{};1H\x1b[2m{}\x1b[0m", trows.saturating_sub(1), fit(&self.note, cols), trows, fit(help, cols));
        out(s.as_bytes());
    }

    fn run(&mut self) -> Option<PathBuf> {
        loop {
            self.draw();
            let act = self.next_act(false);
            // (a timeout only redraws: keep the message)
            // `g` on the update result opens the full git output and leaves the
            // result line (and the output) where they are
            let keep = matches!(self.mode, Mode::Library | Mode::DownloadGame) && self.git_log.is_some() && matches!(act, Act::Letter('g'));
            if !matches!(act, Act::None) && !keep {
                self.note.clear();
                self.git_log = None;
            }
            let act = match self.mode_act(act) {
                Ok(a) => a,
                Err(r) => {
                    if r.is_some() {
                        return r;
                    }
                    continue;
                }
            };
            let total_lines = self.entry_line.len();
            match act {
                Act::Move(d) => self.move_sel(d),
                Act::Page(d) => {
                    let r = self.list_rows() as isize;
                    let line = (self.sel as isize + d * r).clamp(0, total_lines.saturating_sub(1) as isize) as usize;
                    self.sel = self.first_line(self.entry_line.get(line).copied().unwrap_or(0));
                    if self.entries.get(self.entry_line.get(self.sel).copied().unwrap_or(0)).is_some_and(|e| e.kind == Kind::Info) {
                        self.move_sel(1);
                    }
                }
                Act::Top => {
                    self.sel = 0;
                    if self.entries.first().is_some_and(|e| e.kind == Kind::Info) {
                        self.move_sel(1);
                    }
                }
                Act::Bottom => self.sel = self.first_line(self.entries.len().saturating_sub(1)),
                Act::Click(_, i) if i < total_lines && self.entries.get(self.entry_line[i]).is_some_and(|e| e.kind != Kind::Info) => {
                    if self.entry_line[i] == self.entry_line.get(self.sel).copied().unwrap_or(usize::MAX) {
                        if let Some(p) = self.activate() {
                            return Some(p);
                        }
                    } else {
                        self.sel = self.first_line(self.entry_line[i]);
                    }
                }
                Act::Activate => {
                    if let Some(p) = self.activate() {
                        return Some(p);
                    }
                }
                Act::Open => {
                    let entry_idx = self.entry_line.get(self.sel).copied();
                    if entry_idx.is_some_and(|idx| matches!(self.entries.get(idx), Some(Entry { kind: Kind::Archive { .. }, .. }))) {
                        if let Some(p) = self.activate() {
                            return Some(p);
                        }
                    } else if let Some(idx) = entry_idx {
                        if let Some(Entry { kind: Kind::Dir { path, .. }, .. }) = self.entries.get(idx) {
                            let p = path.clone();
                            self.cd(p);
                        } else if self.entries.get(idx).is_some_and(|e| e.kind == Kind::Up) {
                            self.go_up();
                        }
                    }
                }
                Act::Up => self.go_up(),
                Act::Home => {
                    if let Some(h) = crate::sys::home_dir() {
                        self.cd(h);
                    }
                }
                Act::ToggleHidden => {
                    self.hidden = !self.hidden;
                    self.load();
                }
                Act::Cancel if self.mode == Mode::Browse && self.adding => {
                    self.adding = false;
                    self.mode = Mode::Library;
                    self.load();
                }
                Act::Cancel if self.mode == Mode::Browse && self.updating.is_some() => {
                    self.updating = None;
                    self.mode = Mode::Library;
                    self.load();
                }
                Act::Cancel if matches!(self.mode, Mode::Settings | Mode::Download | Mode::DownloadGame) => self.back(),
                Act::Cancel => return None,
                _ => {}
            }
        }
    }

    /// Library and settings keys. Ok(act): handle `act` the usual way;
    /// Err(Some(game)): play it; Err(None): handled.
    fn mode_act(&mut self, act: Act) -> Result<Act, Option<PathBuf>> {
        let entry_idx = self.entry_line.get(self.sel).copied();
        match (self.mode, &act) {
            (Mode::Library, Act::Letter('a')) => {
                self.start_adding();
                Err(None)
            }
            (Mode::Library, Act::Letter('s')) => {
                self.mode = Mode::Settings;
                self.sel = 0;
                self.load();
                Err(None)
            }
            (Mode::Library, Act::Letter('d') | Act::Delete) => {
                if let Some(idx) = entry_idx {
                    if let Some(Entry { kind: Kind::Game { path, .. }, .. }) = self.entries.get(idx) {
                        let p = path.clone();
                        self.st.remove_game(&p);
                        self.st.save();
                        let line_sel = self.sel;
                        self.load();
                        self.sel = line_sel.min(self.entry_line.len().saturating_sub(1));
                        self.note = format!("Removed {} from the list (its files are still there).", p.display());
                    }
                }
                Err(None)
            }
            (Mode::Library, Act::Letter('u')) => {
                if let Some(idx) = entry_idx {
                    if let Some(Entry { kind: Kind::Game { path, .. }, .. }) = self.entries.get(idx) {
                        let p = path.clone();
                        let (note, log) = match git_root(&p) {
                            None => ("This game isn't a git checkout: nothing to update from.".to_string(), None),
                            Some(root) => {
                                let origin = git_origin(&root).unwrap_or_default();
                                let mut g = Gauge::new(&format!("Updating {}", repo_title(&root)), "");
                                let (mut res, mut log) = git_pull(&root, self.login_for(&origin).as_ref(), &mut |s| g.step(s));
                                // a repository for signed-in users: ask, and try again
                                if res.as_ref().is_err_and(|e| e.contains(LOGIN_WANTED)) {
                                    if let Some(l) = self.ask_login(&origin) {
                                        let mut g = Gauge::new(&format!("Updating {}", repo_title(&root)), "");
                                        (res, log) = git_pull(&root, Some(&l), &mut |s| g.step(s));
                                        if res.is_ok() {
                                            self.draw();
                                            self.offer_remember(&l);
                                        }
                                    }
                                }
                                (
                                    match res {
                                        Ok(m) => m,
                                        Err(e) => format!("\x1b[31m{}\x1b[0m", e),
                                    },
                                    Some(log),
                                )
                            }
                        };
                        self.note = note;
                        self.git_log = log;
                        let line_sel = self.sel;
                        self.load();
                        self.sel = line_sel.min(self.entry_line.len().saturating_sub(1));
                    }
                }
                Err(None)
            }
            // every game that is a git checkout (each checkout once)
            (Mode::Library, Act::Letter('U')) => {
                let mut roots: Vec<PathBuf> = Vec::new();
                for e in &self.entries {
                    if let Kind::Game { path, ok: true } = &e.kind {
                        if let Some(r) = git_root(path).filter(|r| !roots.contains(r)) {
                            roots.push(r);
                        }
                    }
                }
                if roots.is_empty() {
                    self.note = "None of these games is a git checkout: nothing to update from.".into();
                    return Err(None);
                }
                let (mut updated, mut current, mut failed) = (Vec::new(), 0, Vec::new());
                let mut log = String::new();
                let mut g = Gauge::new("Updating", "");
                for (i, root) in roots.iter().enumerate() {
                    let name = repo_title(root);
                    g.job(i, roots.len(), &format!("Updating {} of {}: {}", i + 1, roots.len(), name));
                    let login = git_origin(root).and_then(|o| self.login_for(&o));
                    let (res, text) = git_pull(root, login.as_ref(), &mut |s| g.step(s));
                    log += &format!("== {} ==\n{}\n", root.display(), text);
                    match res {
                        Ok(m) if m.starts_with("Already") => current += 1,
                        Ok(m) => updated.push(format!("{} ({})", name, m.trim_start_matches("Updated: ").trim_end_matches('.'))),
                        Err(e) => failed.push(format!("{}: {}", name, e.trim_start_matches("Update failed: "))),
                    }
                }
                let mut parts = Vec::new();
                if !updated.is_empty() {
                    parts.push(format!("updated {}", updated.join(", ")));
                }
                if current > 0 {
                    parts.push(format!("{} already up to date", current));
                }
                if !failed.is_empty() {
                    parts.push(format!("\x1b[31mfailed {}\x1b[0m", failed.join("; ")));
                }
                self.note = format!("Update all: {}.", parts.join("; "));
                self.git_log = Some(log);
                let line_sel = self.sel;
                self.load();
                self.sel = line_sel.min(self.entry_line.len().saturating_sub(1));
                Err(None)
            }
            // `g`: the full git output of the update just done, else "top" as usual
            (Mode::Library | Mode::DownloadGame, Act::Letter('g')) => match self.git_log.clone() {
                Some(log) => {
                    self.view_text("Full git output", &log);
                    Err(None)
                }
                None => Ok(Act::Top),
            },
            // ← / Backspace mean nothing on the library
            (Mode::Library, Act::Up) => Err(None),
            (Mode::Settings, Act::Open | Act::Up) => {
                if let Some(idx) = entry_idx {
                    if let Some(Entry { kind: Kind::Setting(i), .. }) = self.entries.get(idx) {
                        let key = crate::settings::ITEMS[*i].key;
                        self.st.cycle(key, if matches!(act, Act::Up) { -1 } else { 1 });
                        self.st.save();
                        self.load();
                        return Err(None);
                    }
                }
                if matches!(act, Act::Up) {
                    return Ok(Act::Cancel);
                }
                Err(None)
            }
            _ => Ok(act),
        }
    }

    /// A full-screen, scrollable text (the full git output). ↑↓ / PgUp / PgDn /
    /// Home / End scroll; q, Esc, Enter or `g` close it.
    fn view_text(&mut self, title: &str, text: &str) {
        let (cols, _) = term_size();
        // Wrap all lines to terminal width
        let mut wrapped_lines: Vec<String> = Vec::new();
        for line in text.lines() {
            let cleaned = line.trim_end_matches('\r').rsplit('\r').next().unwrap_or("").replace('\t', "    ");
            let cleaned: String = cleaned.chars().filter(|c| !c.is_control()).collect();
            wrapped_lines.extend(wrap_text(&cleaned, cols));
        }
        let mut top = 0usize;
        loop {
            let (cols, trows) = term_size();
            let rows = trows.saturating_sub(3).max(1);
            top = top.min(wrapped_lines.len().saturating_sub(rows));
            let last = (top + rows).min(wrapped_lines.len());
            let mut s = String::from("\x1b[H\x1b[0m\x1b[2J");
            s += &format!(
                "\x1b[1m{}\x1b[0m\r\n\x1b[2m{}\x1b[0m\r\n",
                fit(title, cols),
                fit(&format!("lines {}-{} of {}", (top + 1).min(last), last, wrapped_lines.len()), cols)
            );
            for l in wrapped_lines.iter().skip(top).take(rows) {
                s += &format!("{}\r\n", l);
            }
            s += &format!("\x1b[{};1H\x1b[2m{}\x1b[0m", trows, fit("↑↓ PgUp PgDn Home End scroll  q / Esc / Enter / g close", cols));
            out(s.as_bytes());
            match self.next_act(false) {
                Act::Move(d) => top = top.saturating_add_signed(d),
                Act::Page(d) => top = top.saturating_add_signed(d * rows as isize),
                Act::Top => top = 0,
                Act::Bottom => top = wrapped_lines.len().saturating_sub(1),
                Act::Cancel | Act::Activate | Act::Letter('g') => return,
                _ => {}
            }
        }
    }

    /// Show a Debian-style dialog box with buttons. Returns the index of the
    /// selected button, or None if cancelled.
    fn dialog(&mut self, title: &str, message: &str, buttons: &[&str], default: usize) -> Option<usize> {
        let (cols, rows) = term_size();
        // Wrap message to fit in dialog
        let mut wrapped_lines: Vec<String> = Vec::new();
        for line in message.lines() {
            wrapped_lines.extend(wrap_text(line, cols.saturating_sub(10).min(60)));
        }
        // Dialog dimensions
        let inner_w = cols.saturating_sub(12).clamp(30, 72);
        let inner_h = wrapped_lines.len() + 4; // message + blank + buttons + blank
        let h = inner_h + 2; // borders
        let w = inner_w + 2;
        if w < 30 || rows < h + 2 {
            // Terminal too small, fall back to prompt
            return self.prompt(message, "").and_then(|a| {
                let al = a.to_lowercase();
                buttons.iter().position(|b| b.to_lowercase().starts_with(&al))
            });
        }
        let (x, y) = ((cols - w - 2) / 2 + 1, (rows - h - 1) / 2 + 1);
        let face = "\x1b[0;30;47m";
        let shadow = "\x1b[0;40m  ";
        let mut sel = default.min(buttons.len().saturating_sub(1));
        loop {
            let mut s = String::new();
            let mut row = |r: usize, body: &str, shade: bool| {
                s += &format!("\x1b[{};{}H{}{}", y + r, x, face, body);
                if shade {
                    s += shadow;
                }
            };
            // Title bar
            let title_f = fit(title, inner_w);
            let tw: usize = title_f.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            let left = (inner_w - tw - 2) / 2;
            row(0, &format!("┌{} {} {}┐", "─".repeat(left), title_f, "─".repeat(inner_w - tw - 2 - left)), false);
            // Blank line
            row(1, &format!("│{}│", " ".repeat(inner_w)), true);
            // Message lines
            for (i, line) in wrapped_lines.iter().enumerate() {
                row(
                    2 + i,
                    &format!(
                        "│ {}{} │",
                        line,
                        " ".repeat(inner_w.saturating_sub(2) - line.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>())
                    ),
                    true,
                );
            }
            // Blank line before buttons
            let btn_y = 2 + wrapped_lines.len();
            row(btn_y, &format!("│{}│", " ".repeat(inner_w)), true);
            // Buttons - build row with proper width padding, track button positions for mouse
            let mut btn_row = String::from("│");
            let mut visual_width = 0;
            let mut btn_starts: Vec<usize> = Vec::new(); // visual column where each button starts (relative to dialog left edge)
            for (i, btn) in buttons.iter().enumerate() {
                btn_starts.push(visual_width + 1); // +1 for left border "│"
                let btn_text = format!("[ {} ]", btn);
                let btn_w: usize = btn_text.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
                if i == sel {
                    btn_row += &format!("\x1b[7m{}\x1b[0m{}", btn_text, face);
                } else {
                    btn_row += &format!("{}{}", btn_text, face);
                }
                visual_width += btn_w;
                if i < buttons.len() - 1 {
                    btn_row += "  ";
                    visual_width += 2;
                }
            }
            // Pad to fill inner_w
            if visual_width < inner_w {
                btn_row += &" ".repeat(inner_w - visual_width);
            }
            btn_row += "│";
            row(btn_y + 1, &btn_row, true);
            // Bottom border
            row(btn_y + 2, &format!("└{}┘", "─".repeat(inner_w)), true);
            // Shadow
            s += &format!("\x1b[{};{}H{}{}\x1b[0m", y + h, x + 2, shadow, " ".repeat(w - 2));
            out(s.as_bytes());
            // Handle input
            match self.next_act(true) {
                Act::Up => {
                    // Left arrow - previous button
                    sel = sel.saturating_sub(1);
                }
                Act::Open => {
                    // Right arrow - next button
                    sel = (sel + 1).min(buttons.len().saturating_sub(1));
                }
                Act::Move(d) => {
                    // Up/Down arrows - wrap around since single row
                    if d < 0 {
                        sel = 0;
                    } else {
                        sel = buttons.len().saturating_sub(1);
                    }
                }
                Act::Page(d) => {
                    // PgUp/PgDn - first/last
                    if d < 0 {
                        sel = 0;
                    } else {
                        sel = buttons.len().saturating_sub(1);
                    }
                }
                Act::Activate => return Some(sel),
                Act::Cancel => return None,
                Act::Click(screen_x, screen_y) => {
                    // Check if click is on button row
                    if screen_y == y + btn_y + 1 {
                        // Click is on button row, check which button
                        // btn_starts has visual column of each button start (relative to dialog left edge)
                        // Dialog starts at screen column x, so dialog column 0 = screen column x
                        // The button row content starts at dialog column 1 (after "│")
                        // So screen_x - x - 1 gives us the column within the button row
                        let rel_x = screen_x as isize - (x + 1) as isize;
                        if rel_x >= 0 {
                            for (i, &start) in btn_starts.iter().enumerate() {
                                let btn_text = format!("[ {} ]", buttons[i]);
                                let btn_w: usize = btn_text.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
                                let rx = rel_x as usize;
                                if rx >= start && rx < start + btn_w {
                                    return Some(i);
                                }
                            }
                        }
                    }
                }
                Act::Letter(c) => {
                    let cl = c.to_lowercase().to_string();
                    for (i, btn) in buttons.iter().enumerate() {
                        if btn.to_lowercase().starts_with(&cl) {
                            return Some(i);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn start_adding(&mut self) {
        self.adding = true;
        self.mode = Mode::Browse;
        self.dir = browse_start(&self.st);
        self.load();
    }

    /// Ask for a line of text at the bottom of the screen: Enter accepts (an
    /// untouched empty answer gives `default`), Tab puts `default` in the field
    /// to edit (an answer emptied after that stays empty), Esc / Ctrl-C cancel,
    /// Ctrl-U clears. Mouse reports and other escape sequences are ignored.
    fn prompt(&mut self, question: &str, default: &str) -> Option<String> {
        self.prompt_with(question, default, false)
    }

    /// `prompt`, its answer shown as dots when `secret` (a password).
    fn prompt_with(&mut self, question: &str, default: &str, secret: bool) -> Option<String> {
        let question = if default.is_empty() { format!("{}: ", question) } else { format!("{} [{}]: ", question, default) };
        let width = |s: &str| s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>();
        let mut text = String::new();
        let mut edited = false;
        loop {
            // what has been typed so far
            'consume: while let Some(&b) = self.pending.first() {
                match b {
                    b'\r' | b'\n' => {
                        self.pending.drain(..1);
                        let t = text.trim().to_string();
                        return Some(if t.is_empty() && !edited { default.to_string() } else { t });
                    }
                    0x09 => {
                        self.pending.drain(..1);
                        if !edited {
                            text = default.to_string();
                            edited = true;
                        }
                    }
                    0x03 => {
                        self.pending.drain(..1);
                        return None;
                    }
                    0x15 => {
                        self.pending.drain(..1);
                        text.clear();
                    }
                    0x7f | 0x08 => {
                        self.pending.drain(..1);
                        text.pop();
                    }
                    0x1b => {
                        if self.pending.len() == 1 {
                            // a lone ESC cancels unless more bytes arrive right away
                            if crate::sys::poll(30, -1).input {
                                break 'consume;
                            }
                            self.pending.clear();
                            return None;
                        }
                        let skip = if self.pending[1] != b'[' {
                            Some(2)
                        } else if self.pending.get(2) == Some(&b'M') {
                            // X10 mouse: ESC [ M b x y
                            (self.pending.len() >= 6).then_some(6)
                        } else {
                            // arrows, SGR / urxvt mouse…: up to the final byte
                            self.pending[2..].iter().position(|c| (0x40..=0x7e).contains(c)).map(|p| p + 3)
                        };
                        match skip {
                            Some(n) => {
                                self.pending.drain(..n);
                            }
                            None => break 'consume,
                        }
                    }
                    c if c < 0x20 => {
                        self.pending.drain(..1);
                    }
                    _ => {
                        let len = match b {
                            0xc0..=0xdf => 2,
                            0xe0..=0xef => 3,
                            0xf0..=0xf7 => 4,
                            _ => 1,
                        };
                        if self.pending.len() < len {
                            break 'consume;
                        }
                        if let Ok(s) = std::str::from_utf8(&self.pending[..len]) {
                            text.push_str(s);
                        }
                        self.pending.drain(..len);
                    }
                }
            }
            // the question, and the end of the answer if it is longer than the line
            let (cols, rows) = term_size();
            // (a long question is cut so there is always room to type)
            let q = fit(&question, cols.saturating_sub(24).max(cols / 2));
            let room = cols.saturating_sub(width(&q) + 2).max(1);
            let (mut shown, mut used) = (Vec::new(), 0);
            let typed = if secret { "•".repeat(text.chars().count()) } else { text.clone() };
            for c in typed.chars().rev() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > room {
                    break;
                }
                used += cw;
                shown.push(c);
            }
            let shown: String = shown.into_iter().rev().collect();
            out(format!(
                "\x1b[{};1H\x1b[0m\x1b[2K{}{}\x1b[7m \x1b[0m\x1b[{};1H\x1b[2K\x1b[2m{}\x1b[0m",
                rows.saturating_sub(1),
                q,
                shown,
                rows,
                fit(
                    if default.is_empty() || edited {
                        "Enter accept · Esc cancel · Ctrl-U clear"
                    } else {
                        "Enter use the [default] · Tab edit it · Esc cancel"
                    },
                    cols
                )
            )
            .as_bytes());
            let r = crate::sys::poll(500, -1);
            if r.hangup {
                return None;
            }
            if r.input {
                let mut b = [0u8; 256];
                match crate::sys::read_stdin(&mut b) {
                    Ok(n) if n > 0 => self.pending.extend_from_slice(&b[..n]),
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock) => {}
                    _ => return None,
                }
            }
        }
    }

    /// "Clone a game…": ask for the repository, the folder to keep it in and,
    /// optionally, a branch; `git clone` it, add it to the library without
    /// starting it, and mark it "(NEW)" until it is played.
    fn clone_game(&mut self) {
        if which("git").is_none() {
            self.note = "\x1b[31mgit isn't installed (or isn't on PATH): can't clone.\x1b[0m".into();
            return;
        }
        let cancelled = "Clone cancelled.";
        // each field starts from what was used last time
        let last_url = self.st.get("clone_url").to_string();
        let Some(url) = self.prompt("Repository to clone (URL or path)", &last_url).filter(|u| !u.is_empty()) else {
            self.note = cancelled.into();
            return;
        };
        let Some(name) = repo_name(&url) else {
            self.note = format!("\x1b[31mCan't tell a folder name from \"{}\".\x1b[0m", url);
            return;
        };
        let default_dir = match self.st.get("clone_dir") {
            "" => browse_start(&self.st).display().to_string(),
            d => d.to_string(),
        };
        let Some(dir) = self.prompt(&format!("Folder to store it in (it gets its own \"{}\" subfolder)", name), &default_dir) else {
            self.note = cancelled.into();
            return;
        };
        let last_branch = self.st.get("clone_branch").to_string();
        let Some(branch) = self.prompt("Branch to use (optional, empty = the repository's default)", &last_branch) else {
            self.note = cancelled.into();
            return;
        };
        let last_depth = self.st.get("clone_depth").to_string();
        let Some(depth_text) = self.prompt("Depth (optional, 1 = latest commit only, empty = all)", &last_depth) else {
            self.note = cancelled.into();
            return;
        };
        let depth = if depth_text.is_empty() {
            None
        } else {
            match depth_text.parse::<u32>() {
                Ok(d) if d > 0 => Some(d),
                _ => {
                    self.note = "\x1b[31mThe depth must be a whole number of 1 or more (or empty for the full history).\x1b[0m".into();
                    return;
                }
            }
        };
        // remembered now, so a failed or cancelled clone can be retried from them
        for (k, v) in [("clone_url", &url), ("clone_dir", &dir), ("clone_branch", &branch), ("clone_depth", &depth_text)] {
            self.st.set(k, v);
        }
        self.st.save();
        self.clone_into(&url, &name, &expand_home(&dir), branch.trim(), depth);
    }

    /// `--clone URL` (or a URL in place of the game folder): clone it
    /// straight away, without the questions of "Clone a game…": into the
    /// folder it would offer first, on the default branch, the full history.
    fn clone_direct(&mut self, url: &str) {
        if which("git").is_none() {
            self.note = "\x1b[31mgit isn't installed (or isn't on PATH): can't clone.\x1b[0m".into();
            return;
        }
        let Some(name) = repo_name(url) else {
            self.note = format!("\x1b[31mCan't tell a folder name from \"{}\".\x1b[0m", url);
            return;
        };
        let parent = self.clone_parent();
        // (offered by "Clone a game…" next time)
        self.st.set("clone_url", url);
        self.st.save();
        self.clone_into(url, &name, &parent, "", None);
    }

    /// Where clones go without asking: the folder "Clone a game…" was given
    /// last, else next to the last game.
    fn clone_parent(&self) -> PathBuf {
        match self.st.get("clone_dir") {
            "" => browse_start(&self.st),
            d => expand_home(d),
        }
    }

    /// Esc / "← Back": a downloader's game goes back to the list, the other
    /// pages to the library.
    fn back(&mut self) {
        self.mode = if self.mode == Mode::DownloadGame { Mode::Download } else { Mode::Library };
        self.load();
    }

    /// The downloader: the games of the eraGames wiki that link a git
    /// repository (the list is fetched once per launcher session).
    fn open_downloader(&mut self) {
        if self.wiki.is_empty() {
            let mut g = Gauge::new("Download a game", "Esc / Ctrl-C: cancel");
            g.step(Step::Start("Fetching the game list from the eraGames wiki…"));
            let mut games = Vec::new();
            let mut more = String::new();
            // (the API pages its answers; a few requests at most)
            for _ in 0..10 {
                let got = fetch(&crate::download::list_url(&more), &mut g, &mut || self.cancel_pressed());
                match got.and_then(|json| crate::download::add_list(&json, &mut games)) {
                    Ok(m) if m.is_empty() => break,
                    Ok(m) => more = m,
                    Err(e) => {
                        self.note = if e == CANCELLED { e } else { format!("\x1b[31m{}\x1b[0m", e) };
                        return;
                    }
                }
            }
            games.retain(|g| !g.repos.is_empty());
            games.sort_by_key(|g| g.title.to_lowercase());
            if games.is_empty() {
                self.note = "The wiki lists no game with a git repository right now.".into();
                return;
            }
            self.wiki = games;
            self.wiki_game = 0;
        }
        self.mode = Mode::Download;
        self.load();
    }

    /// A downloader's game: its wiki page's facts, then its repositories.
    fn open_wiki_game(&mut self, i: usize) {
        let Some(title) = self.wiki.get(i).map(|g| g.title.clone()) else { return };
        let mut g = Gauge::new(&title, "Esc / Ctrl-C: cancel");
        // (the page's first section came with the list; read alone only
        // when it didn't)
        self.wiki_info = if self.wiki[i].info != Default::default() {
            self.wiki[i].info.clone()
        } else {
            g.step(Step::Start("Reading its page on the eraGames wiki…"));
            let got = fetch(&crate::download::page_url(&title), &mut g, &mut || self.cancel_pressed());
            // without the page the repositories are still there to clone
            match got.and_then(|json| crate::download::details(&json)) {
                Ok(d) => d,
                Err(e) if e == CANCELLED => return,
                Err(e) => {
                    self.note = format!("\x1b[31mCan't read its page: {}\x1b[0m", e);
                    Default::default()
                }
            }
        };
        // which repositories answer, their branches (see `download::options`)
        let repos = self.wiki[i].repos.clone();
        g.step(Step::Start("Asking its repositories…"));
        let mut heads: Vec<(String, Option<crate::download::Heads>)> = Vec::new();
        for r in &repos {
            if heads.iter().any(|(u, _)| *u == r.url) {
                continue;
            }
            let login = self.login_for(&r.url);
            let mut c = git_cmd(login.as_ref());
            c.args(["ls-remote", "--symref", &r.url, "HEAD", "refs/heads/*"]);
            let got = match run_watched(&mut c, &mut |s| g.step(s), &mut || self.cancel_pressed()) {
                Ok((RunEnd::Ok, out, _)) => Some(crate::download::parse_heads(&out)),
                Ok((RunEnd::Cancelled, ..)) => break,
                _ => None,
            };
            heads.push((r.url.clone(), got));
        }
        self.wiki_opts = crate::download::options(&repos, &heads);
        self.wiki_game = i;
        self.mode = Mode::DownloadGame;
        self.load();
    }

    /// The login typed for `url`'s host in this session.
    fn login_for(&self, url: &str) -> Option<Login> {
        let host = url_host(url)?;
        self.logins.iter().find(|l| l.host == host).cloned()
    }

    /// Ask for a login to `url`'s host, after git said the repository wants
    /// one. It is kept for this launcher session (`offer_remember` once it
    /// has worked).
    fn ask_login(&mut self, url: &str) -> Option<Login> {
        let host = url_host(url)?;
        self.draw();
        let l = self.type_login(&host, &format!("{} asks for a login. ", host))?;
        self.logins.retain(|o| o.host != host);
        self.logins.push(l.clone());
        Some(l)
    }

    /// The user name and password for `host`, typed (the password hidden).
    fn type_login(&mut self, host: &str, why: &str) -> Option<Login> {
        let user = self.prompt(&format!("{}Your {} user name", why, host), "").filter(|u| !u.is_empty())?;
        let pass = self.prompt_with(&format!("{}'s password or access token (read_repository) on {}", user, host), "", true).filter(|p| !p.is_empty())?;
        Some(Login { host: host.to_string(), user, pass, remember: false })
    }

    /// After a typed login worked: keep it for the next sessions?
    fn offer_remember(&mut self, l: &Login) {
        if l.remember {
            return;
        }
        let path = logins_path().map_or_else(|| "the settings folder".into(), |p| p.display().to_string());
        let yes = self
            .prompt(&format!("Remember {}'s login on {}? It is kept in {} (plain text, readable only by you) (y/N)", l.user, l.host, path), "")
            .is_some_and(|a| matches!(a.to_lowercase().as_str(), "y" | "yes"));
        for o in self.logins.iter_mut().filter(|o| o.host == l.host) {
            o.remember = yes;
        }
        // (also on "no": a remembered login this one replaced goes)
        if let Err(e) = save_logins(&self.logins) {
            self.note = format!("\x1b[31mCan't keep the login: {}\x1b[0m", e);
        }
    }

    /// The downloader's login entry: who is logged in to gitgud.io.
    fn login_label(&self) -> String {
        match self.login_for(&format!("https://{}/", GITGUD)) {
            Some(l) => format!(
                "🔑 Logged in to {} as {}{} · Enter to log out",
                GITGUD,
                l.user,
                if l.remember { " (remembered)" } else { " (until you leave the launcher)" }
            ),
            None => format!("🔑 Log in to {}… (many era-games repositories are for signed-in users)", GITGUD),
        }
    }

    /// The login entry chosen: log in (checked with git, then offered to be
    /// remembered), or out (forgetting a remembered login too).
    fn login_entry(&mut self) {
        let host = GITGUD;
        if let Some(l) = self.login_for(&format!("https://{}/", host)) {
            let yes = self
                .prompt(&format!("Log out of {}, forgetting {}'s login? (y/N)", host, l.user), "")
                .is_some_and(|a| matches!(a.to_lowercase().as_str(), "y" | "yes"));
            if yes {
                self.logins.retain(|o| o.host != host);
                self.note = match save_logins(&self.logins) {
                    Ok(()) => format!("Logged out of {}.", host),
                    Err(e) => format!("\x1b[31mLogged out, but can't update the saved logins: {}\x1b[0m", e),
                };
            }
        } else if let Some(mut l) = self.type_login(host, "") {
            let mut g = Gauge::new(&format!("Logging in to {}", host), "Esc / Ctrl-C: cancel");
            g.step(Step::Start("Checking the login with git…"));
            match check_login(&l, &mut g, &mut || self.cancel_pressed()) {
                Ok(true) => {
                    self.logins.retain(|o| o.host != host);
                    self.logins.push(l.clone());
                    self.draw();
                    self.offer_remember(&l);
                    l.remember = self.login_for(&format!("https://{}/", host)).is_some_and(|o| o.remember);
                    if self.note.is_empty() {
                        self.note = format!("Logged in to {} as {}{}.", host, l.user, if l.remember { " (remembered)" } else { "" });
                    }
                }
                Ok(false) => {
                    self.note = format!(
                        "\x1b[31m{} refused the login: a wrong user name or password, or one that needs an access token (read_repository).\x1b[0m",
                        host
                    )
                }
                Err(e) if e == CANCELLED => self.note = e,
                Err(e) => self.note = format!("\x1b[31mCan't check the login: {}\x1b[0m", e),
            }
        }
        // a game's repositories are asked again with the new login
        if self.mode == Mode::DownloadGame {
            let (i, note) = (self.wiki_game, std::mem::take(&mut self.note));
            self.open_wiki_game(i);
            if self.note.is_empty() {
                self.note = note;
            }
        } else {
            self.load();
        }
    }

    /// Select game `g` in the library, adding it if it isn't there.
    fn select_game(&mut self, g: &Path) -> PathBuf {
        let g = self.st.add_game(g);
        self.st.save();
        self.mode = Mode::Library;
        self.load();
        // (the first screen line of its entry: long ones wrap)
        let idx = self.entries.iter().position(|e| matches!(&e.kind, Kind::Game { path, .. } if *path == g));
        self.sel = idx.and_then(|i| self.entry_line.iter().position(|&e| e == i)).unwrap_or(0);
        g
    }

    /// `git clone` `url` into `parent`/`name` with the gauge, then add the
    /// game to the library, selected and marked (NEW).
    fn clone_into(&mut self, url: &str, name: &str, parent: &Path, branch: &str, depth: Option<u32>) {
        let target = parent.join(name);
        let in_use = target.exists()
            && match std::fs::read_dir(&target) {
                Ok(mut d) => d.next().is_some(),
                Err(_) => true,
            };
        if in_use {
            // cloned before: that game, to play or update
            let origin = git_origin(&target);
            if let (Some(game), true) = (find_game_dir(&target, 2), origin.as_deref() == Some(url.trim())) {
                let g = self.select_game(&game);
                self.note = format!("Already cloned at {}: Enter to play, u to update.", g.display());
                return;
            }
            self.note = format!("\x1b[31m{} already exists and isn't empty.\x1b[0m", target.display());
            return;
        }
        if let Err(e) = std::fs::create_dir_all(parent) {
            self.note = format!("\x1b[31mCan't create {}: {}\x1b[0m", parent.display(), e);
            return;
        }
        // (the launcher again behind the gauge, without the prompts)
        self.draw();
        let mut login = self.login_for(url);
        let mut asked = false;
        let (end, mut log, mut g) = loop {
            let mut g = Gauge::new(&format!("Cloning {}", name), "Esc / Ctrl-C: cancel the clone");
            let (end, log) = git_clone(url, branch, depth, &target, login.as_ref(), &mut |s| g.step(s), &mut || self.cancel_pressed());
            // a repository for signed-in users: ask once, then again with it
            if matches!(end, CloneEnd::Failed(_)) && needs_login(&log) && !asked {
                asked = true;
                if let Some(l) = self.ask_login(url) {
                    login = Some(l);
                    self.draw();
                    continue;
                }
            }
            break (end, log, g);
        };
        let mut pics = String::new();
        if matches!(end, CloneEnd::Done) {
            g.hint.clear();
            pics = lfs_pull(&target, &mut log, login.as_ref(), &mut |s| g.step(s));
            if let (true, Some(l)) = (asked, &login) {
                self.draw();
                self.offer_remember(l);
            }
        }
        self.git_log = Some(log);
        match end {
            CloneEnd::Done => {}
            CloneEnd::Cancelled => {
                // git removes the folder it was making when it can; if something
                // is left, ask before deleting it (it was missing or empty before
                // we started)
                self.draw();
                let left = target.exists()
                    && match std::fs::read_dir(&target) {
                        Ok(mut d) => d.next().is_some(),
                        Err(_) => false,
                    };
                self.note = if !left {
                    "Clone cancelled: nothing was added (g: git output).".to_string()
                } else {
                    let yes = self
                        .prompt(&format!("Remove the half-cloned \"{}\" folder? (y/N)", name), "")
                        .is_some_and(|a| matches!(a.to_lowercase().as_str(), "y" | "yes"));
                    if yes && std::fs::remove_dir_all(&target).is_ok() {
                        "Clone cancelled: the half-cloned folder was removed, nothing was added (g: git output).".to_string()
                    } else {
                        format!("Clone cancelled: nothing was added; the half-cloned folder is still at {}.", target.display())
                    }
                };
                return;
            }
            CloneEnd::Failed(e) => {
                self.note = format!("\x1b[31m{}\x1b[0m", e);
                return;
            }
        }
        // the game may sit in a subfolder of the repository (see `git_root`)
        let Some(game) = find_game_dir(&target, 2) else {
            self.note = format!("Cloned into {}, but no Emuera game (CSV and ERB folders) is in it, so it wasn't added.", target.display());
            return;
        };
        let g = self.st.add_game(&game);
        self.st.mark_new(&g);
        let g = self.select_game(&g);
        let at = match (git_root(&g).and_then(|r| git_head(&r)), depth) {
            (Some(h), Some(d)) => format!(" ({}, depth {})", h, d),
            (Some(h), None) => format!(" ({})", h),
            (None, Some(d)) => format!(" (depth {})", d),
            (None, None) => String::new(),
        };
        self.note = format!("Cloned {}{}: NEW, Enter to play (g: git output).{}", game_title(&g).unwrap_or_else(|| g.display().to_string()), at, pics);
    }

    /// Has Esc or Ctrl-C been pressed? Reads what was typed meanwhile and
    /// throws it away; mouse reports and other escape sequences don't count.
    fn cancel_pressed(&mut self) -> bool {
        if crate::sys::poll(0, -1).input {
            let mut b = [0u8; 256];
            if let Ok(n) = crate::sys::read_stdin(&mut b) {
                self.pending.extend_from_slice(&b[..n]);
            }
        }
        let b = std::mem::take(&mut self.pending);
        let mut keep = Vec::new();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                0x03 => return true,
                0x1b => {
                    let rest = &b[i..];
                    if rest.len() == 1 {
                        // a lone ESC cancels unless more bytes arrive right away
                        if crate::sys::poll(30, -1).input {
                            keep.push(0x1b);
                            break;
                        }
                        return true;
                    }
                    let end = if rest[1] != b'[' {
                        Some(2)
                    } else if rest.get(2) == Some(&b'M') {
                        // X10 mouse: ESC [ M b x y
                        (rest.len() >= 6).then_some(6)
                    } else {
                        rest[2..].iter().position(|c| (0x40..=0x7e).contains(c)).map(|p| p + 3)
                    };
                    match end {
                        Some(n) => i += n,
                        None => {
                            // a sequence still arriving: look at it next time
                            keep.extend_from_slice(rest);
                            break;
                        }
                    }
                }
                _ => i += 1,
            }
        }
        self.pending = keep;
        false
    }

    /// A game folder was chosen in the browser: add it to the library (when
    /// adding), update a missing game's reference (when updating), or play it.
    fn chosen(&mut self, game: PathBuf) -> Option<PathBuf> {
        // Handle updating a missing game's folder reference
        if let Some(old) = self.updating.take() {
            if is_game_dir(&game) {
                self.st.update_game(&old, &game);
                self.st.save();
                self.mode = Mode::Library;
                self.load();
                self.sel = self.entries.iter().position(|e| matches!(&e.kind, Kind::Game { path, .. } if *path == game)).unwrap_or(0);
                self.note = format!("Updated {} to {} — Enter to play.", game_title(&game).unwrap_or_else(|| game.display().to_string()), game.display());
            } else {
                self.note = "\x1b[31mThe selected folder is not a valid Emuera game (needs CSV/ and ERB/).\x1b[0m".into();
                self.updating = Some(old); // keep it for retry
            }
            return None;
        }
        if !self.adding {
            return Some(game);
        }
        let known = self.st.library.len();
        let g = self.st.add_game(&game);
        // a game that wasn't on the list yet is "(NEW)" until it is played
        if self.st.library.len() > known {
            self.st.mark_new(&g);
        }
        self.st.save();
        self.adding = false;
        self.mode = Mode::Library;
        self.load();
        self.sel = self.entries.iter().position(|e| matches!(&e.kind, Kind::Game { path, .. } if *path == g)).unwrap_or(0);
        self.note = format!("Added {} — Enter to play.", game_title(&g).unwrap_or_else(|| g.display().to_string()));
        None
    }

    fn go_up(&mut self) {
        if let Some(p) = self.dir.parent().map(Path::to_path_buf) {
            self.cd(p);
        }
    }

    /// Enter on the selected entry: play a game, otherwise open the folder.
    fn activate(&mut self) -> Option<PathBuf> {
        let entry_idx = self.entry_line.get(self.sel).copied()?;
        let kind = self.entries.get(entry_idx)?.kind.clone();
        match kind {
            Kind::Game { path, ok: true } => return Some(path),
            Kind::Game { path, ok: false } => {
                // Show Debian-style dialog to update or remove
                let title = game_title(&path).unwrap_or_else(|| "Emuera game".into());
                let msg = format!("{} ({}) is missing.\n\nRemove it from the list or update the folder?", title, path.display());
                match self.dialog("Missing Game", &msg, &["Remove", "Update", "Cancel"], 1) {
                    Some(0) => {
                        // Remove
                        self.st.remove_game(&path);
                        self.st.save();
                        let line_sel = self.sel;
                        self.load();
                        self.sel = line_sel.min(self.entry_line.len().saturating_sub(1));
                        self.note = format!("Removed {} from the list (its files are still there).", path.display());
                    }
                    Some(1) => {
                        // Update: enter browse mode to pick new folder
                        self.updating = Some(path);
                        self.mode = Mode::Browse;
                        self.dir = browse_start(&self.st);
                        self.load();
                    }
                    None => {
                        // Cancelled - show original message
                        self.note = format!("\x1b[31m{} is gone (d removes it from the list)\x1b[0m", path.display());
                    }
                    Some(_) => {}
                }
            }
            Kind::AddGame => self.start_adding(),
            Kind::CloneGame => self.clone_game(),
            Kind::Download => self.open_downloader(),
            Kind::WikiGame(i) => self.open_wiki_game(i),
            Kind::WikiRepo(i) => {
                if let Some(r) = self.wiki_opts.get(i).map(|o| o.repo.clone()) {
                    if which("git").is_none() {
                        self.note = "\x1b[31mgit isn't installed (or isn't on PATH): can't download.\x1b[0m".into();
                    } else {
                        let parent = self.clone_parent();
                        self.clone_into(&r.url, &r.folder(), &parent, &r.branch, Some(1));
                    }
                }
            }
            Kind::Info => {}
            Kind::Login => self.login_entry(),
            Kind::OpenSettings => {
                self.mode = Mode::Settings;
                self.sel = 0;
                self.load();
            }
            Kind::Setting(i) => {
                self.st.cycle(crate::settings::ITEMS[i].key, 1);
                self.st.save();
                self.load();
            }
            Kind::Back => self.back(),
            Kind::PlayHere => {
                let d = self.dir.clone();
                return self.chosen(d);
            }
            Kind::Up => self.go_up(),
            Kind::Dir { path, game: true } => return self.chosen(path),
            Kind::Archive { path } => match self.extract(&path) {
                Ok(g) => return self.chosen(g),
                Err(e) => self.note = format!("\x1b[31m{}\x1b[0m", e),
            },
            Kind::Dir { path, game: false } => {
                self.cd(path);
                if self.entries.iter().all(|e| e.kind == Kind::Up) {
                    self.note = "Nothing here: no folders or archives.".into();
                }
            }
        }
        None
    }

    /// Extract an archive, showing progress in a gauge.
    fn extract(&mut self, archive: &Path) -> Result<PathBuf, String> {
        let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut g = Gauge::new(&format!("Extracting {}", name), "");
        crate::archive::extract_game(archive, &mut |n| g.show(&format!("{} files", n), None))
    }

    /// Wait for input and turn it into an action. If `dialog` is true, mouse
    /// coordinates are raw screen cells (for dialogs); otherwise they are
    /// converted to list-relative coordinates.
    fn next_act(&mut self, dialog: bool) -> Act {
        loop {
            if let Some((a, used)) = self.parse() {
                self.pending.drain(..used);
                return a;
            }
            let pad = if self.st.get("gamepad") != "off" {
                crate::gamepad::start();
                crate::gamepad::fd()
            } else {
                -1
            };
            let r = crate::sys::poll2(500, self.mice.as_ref().map_or(-1, |m| m.fd()), pad);
            if r.hangup {
                return Act::Cancel;
            }
            if !r.input && !r.extra && !r.extra2 {
                // redraw on resize
                return Act::None;
            }
            if r.extra2 {
                // a gamepad: as the keys it stands for here
                use crate::gamepad::Ev;
                for ev in crate::gamepad::read() {
                    self.pending.extend_from_slice(match ev {
                        Ev::Dir(0) | Ev::ScrollUp => b"\x1b[A",
                        Ev::Dir(1) | Ev::ScrollDown => b"\x1b[B",
                        Ev::Dir(2) => b"\x1b[D",
                        Ev::Dir(_) => b"\x1b[C",
                        Ev::A | Ev::Enter => b"\r",
                        Ev::B | Ev::Esc => b"\x1b",
                        Ev::Settings => b"s",
                        Ev::PageUp => b"\x1b[5~",
                        Ev::PageDown => b"\x1b[6~",
                        Ev::Home => b"\x1b[H",
                        Ev::End => b"\x1b[F",
                        Ev::Move(..) => b"",
                    });
                }
            }
            if r.extra {
                let (cols, rows) = term_size();
                let evs = self.mice.as_mut().map(|m| m.read((8, 16), cols, rows)).unwrap_or_default();
                for (btn, x, y, press) in evs {
                    if dialog {
                        // In dialog mode: return raw screen coordinates
                        return Act::Click(x as usize, y as usize);
                    }
                    if let Some(a) = self.mouse(btn, x, y, press) {
                        return a;
                    }
                }
            }
            if r.input {
                let mut b = [0u8; 256];
                match crate::sys::read_stdin(&mut b) {
                    Ok(n) if n > 0 => self.pending.extend_from_slice(&b[..n]),
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock) => {}
                    _ => return Act::Cancel,
                }
            }
        }
    }

    /// A mouse event at 1-based (x, y) screen coordinates.
    fn mouse(&self, btn: u16, x: u16, y: u16, press: bool) -> Option<Act> {
        match btn & !(4 | 8 | 16) {
            64 => Some(Act::Move(-3)),
            65 => Some(Act::Move(3)),
            0 if press => {
                let row = (y as usize).checked_sub(3)?; // list starts on screen row 3
                (row < self.list_rows()).then_some(Act::Click(x as usize, self.top + row))
            }
            _ => None,
        }
    }

    /// Decode one action from `pending`: (action, bytes used), or None if
    /// more bytes are needed.
    fn parse(&self) -> Option<(Act, usize)> {
        let b = &self.pending;
        let first = *b.first()?;
        if first != 0x1b {
            let a = match first {
                b'\r' | b'\n' => Act::Activate,
                0x7f | 0x08 | b'h' => Act::Up,
                b'q' | 0x03 => Act::Cancel,
                b'k' => Act::Move(-1),
                b'j' => Act::Move(1),
                b'l' => Act::Open,
                b'~' => Act::Home,
                b'.' => Act::ToggleHidden,
                // on the library `g` may be the full git output of an update
                // (`mode_act` falls back to "top" when there is none)
                b'g' if self.mode == Mode::Library => Act::Letter('g'),
                b'g' => Act::Top,
                b'G' => Act::Bottom,
                c if c.is_ascii_alphabetic() => Act::Letter(c as char),
                _ => Act::None,
            };
            return Some((a, 1));
        }
        if b.len() == 1 {
            // lone ESC unless more bytes arrive right away
            return if crate::sys::poll(30, -1).input { None } else { Some((Act::Cancel, 1)) };
        }
        if b[1] != b'[' {
            return Some((Act::None, 2));
        }
        // X10 mouse: ESC [ M b x y
        if b.get(2) == Some(&b'M') {
            if b.len() < 6 {
                return None;
            }
            let (cb, x, y) = (b[3].wrapping_sub(32) as u16, b[4].wrapping_sub(32) as u16, b[5].wrapping_sub(32) as u16);
            let release = cb & 3 == 3 && cb & (32 | 64) == 0;
            return Some((self.mouse(if release { 3 } else { cb }, x, y, !release).unwrap_or(Act::None), 6));
        }
        let end = b[2..].iter().position(|c| (0x40..=0x7e).contains(c))? + 2;
        let params = String::from_utf8_lossy(&b[2..end]).to_string();
        let a = match (b[end], params.as_str()) {
            (b'A', _) => Act::Move(-1),
            (b'B', _) => Act::Move(1),
            (b'C', _) => Act::Open,
            (b'D', _) => Act::Up,
            (b'H', _) => Act::Top,
            (b'F', _) => Act::Bottom,
            (b'~', "3") => Act::Delete,
            (b'~', "5") => Act::Page(-1),
            (b'~', "6") => Act::Page(1),
            (b'~', "1") | (b'~', "7") => Act::Top,
            (b'~', "4") | (b'~', "8") => Act::Bottom,
            // the illumos / Solaris console's Home, PgUp, End, PgDn
            (b'z', "214") => Act::Top,
            (b'z', "216") => Act::Page(-1),
            (b'z', "220") => Act::Bottom,
            (b'z', "222") => Act::Page(1),
            // SGR mouse ESC [ < b ; x ; y M/m, urxvt ESC [ b ; x ; y M
            (b'M' | b'm', p) => {
                let sgr = p.starts_with('<');
                let v: Vec<u16> = p.trim_start_matches('<').split(';').filter_map(|x| x.parse().ok()).collect();
                match v.as_slice() {
                    [cb, x, y] if sgr => self.mouse(*cb, *x, *y, b[end] == b'M').unwrap_or(Act::None),
                    [cb, x, y] => {
                        let cb = cb.saturating_sub(32);
                        let release = cb & 3 == 3 && cb & (32 | 64) == 0;
                        self.mouse(cb, *x, *y, !release).unwrap_or(Act::None)
                    }
                    _ => Act::None,
                }
            }
            _ => Act::None,
        };
        Some((a, end + 1))
    }
}

// ------------------------------------------------------------ remembering

/// The game folder chosen last time, if it still looks like a game.
pub fn last_game() -> Option<PathBuf> {
    crate::settings::Settings::load().last.filter(|p| is_game_dir(p))
}

/// Remember `game` for the next run without arguments (and in the library).
pub fn remember(game: &Path) {
    let mut st = crate::settings::Settings::load();
    st.played(game);
    st.save();
}

/// How many games the library lists (folders still there).
pub fn library_size() -> usize {
    crate::settings::Settings::load().library.iter().filter(|g| is_game_dir(g)).count()
}

// ------------------------------------------------------------- installing

/// `--install`: copy this binary to the user binary folder.
pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find this program: {}", e))?;
    install_from(&exe)
}

/// Can we write into `d` (an existing folder)?
fn writable(d: &Path) -> bool {
    crate::sys::dir_writable(d)
}

/// Where `--install` puts the program: a folder on the default `$PATH`, so
/// `emuera-tty` runs from any terminal right away. In order:
/// 1. a writable PATH folder that already has emuera-tty (upgrade in place);
/// 2. the usual user folders when they are on PATH: `$XDG_BIN_HOME`,
///    `~/.local/bin`, `~/bin`;
/// 3. any other writable PATH folder (e.g. `/usr/local/bin` as root), but
///    not tool-managed ones like `~/.cargo/bin`;
/// 4. failing all that, `$XDG_BIN_HOME` or `~/.local/bin` (created; the
///    install message then says how to add it to PATH).
pub fn bin_dir() -> Result<PathBuf, String> {
    let home = crate::sys::home_dir().ok_or("HOME is not set")?;
    let xdg = std::env::var_os("XDG_BIN_HOME").map(PathBuf::from).filter(|p| p.is_absolute());
    let path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
    if let Some(d) = path.iter().find(|d| d.join(crate::sys::EXE_NAME).is_file() && writable(d)) {
        return Ok(d.clone());
    }
    let usual: Vec<PathBuf> = xdg.iter().cloned().chain([home.join(".local").join("bin"), home.join("bin")]).collect();
    if let Some(d) = usual.iter().find(|u| path.contains(u) && (writable(u) || !u.exists())) {
        return Ok(d.clone());
    }
    let managed = [".cargo", ".rustup", ".npm", ".nvm", ".pyenv", ".rbenv", ".local/share", "go/bin", "snap"];
    let is_managed = |d: &Path| d.strip_prefix(&home).is_ok_and(|r| managed.iter().any(|m| r.starts_with(m)));
    if let Some(d) = path.iter().find(|d| writable(d) && !is_managed(d)) {
        return Ok(d.clone());
    }
    Ok(xdg.unwrap_or_else(|| home.join(".local").join("bin")))
}

/// Copy the binary `exe` to the user binary folder ([`bin_dir`]).
fn install_from(exe: &Path) -> Result<String, String> {
    let dir = bin_dir()?;
    let dest = dir.join(crate::sys::EXE_NAME);
    if dest.canonicalize().ok() == exe.canonicalize().ok() {
        return Ok(format!("{} is already installed.", dest.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    // write beside the target and rename, so replacing a running copy is safe
    let tmp = dir.join(".emuera-tty.new");
    // Windows can't replace a running program, but can rename it aside
    #[cfg(windows)]
    if dest.exists() {
        let old = dir.join(".emuera-tty.old.exe");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&dest, &old);
    }
    std::fs::copy(exe, &tmp).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    crate::sys::set_executable(&tmp).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir));
    let mut msg = format!("Installed {}.", dest.display());
    match install_man(&dir) {
        Ok(Some(p)) => msg += &format!("\nManual page: {} (man emuera-tty).", p.display()),
        Ok(None) => {}
        Err(e) => msg += &format!("\n(manual page not installed: {})", e),
    }
    if !on_path {
        if cfg!(windows) {
            msg += &format!("\n{} is not on your PATH; add it with (then open a new terminal):\n  setx PATH \"%PATH%;{}\"", dir.display(), dir.display());
        } else {
            msg += &format!("\n{} is not on your PATH; add this to ~/.bashrc or ~/.profile:\n  export PATH=\"{}:$PATH\"", dir.display(), dir.display());
        }
    }
    Ok(msg)
}

/// The manual page, built in.
pub const MANUAL: &str = include_str!("../doc/emuera-tty.1");

/// Where the manual page goes for a program in `bin`: `bin/../share/man/
/// man1`, which man-db and mandoc search for every PATH folder.
fn man_path(bin: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    Some(bin.parent()?.join("share").join("man").join("man1").join("emuera-tty.1"))
}

/// Install the manual page next to the program in `bin`.
fn install_man(bin: &Path) -> Result<Option<PathBuf>, String> {
    let Some(p) = man_path(bin) else { return Ok(None) };
    let d = p.parent().unwrap();
    std::fs::create_dir_all(d).map_err(|e| format!("{}: {}", d.display(), e))?;
    std::fs::write(&p, MANUAL).map_err(|e| format!("{}: {}", p.display(), e))?;
    Ok(Some(p))
}

/// `--manual`: show the manual page (through man when there is one).
pub fn show_manual() {
    use std::io::Write as _;
    if crate::sys::is_tty() && which("man").is_some() {
        let child = std::process::Command::new("man").args(["-l", "-"]).stdin(std::process::Stdio::piped()).spawn();
        if let Ok(mut c) = child {
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(MANUAL.as_bytes());
            }
            if c.wait().is_ok_and(|s| s.success()) {
                return;
            }
        }
    }
    print!("{}", MANUAL);
}

/// `--uninstall`: remove the installed copy.
pub fn uninstall() -> Result<String, String> {
    // the copy the shell would run, else where --install would put it
    let on_path = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join(crate::sys::EXE_NAME)).find(|f| f.is_file() && f.parent().is_some_and(writable)));
    let dest = match on_path {
        Some(f) => f,
        None => bin_dir()?.join(crate::sys::EXE_NAME),
    };
    match std::fs::remove_file(&dest) {
        Ok(()) => {
            if let Some(m) = dest.parent().and_then(man_path) {
                let _ = std::fs::remove_file(m);
            }
            Ok(format!("Removed {}.", dest.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(format!("{} is not installed.", dest.display())),
        Err(e) => Err(format!("{}: {}", dest.display(), e)),
    }
}

/// `--reset-data`: forget emuera-tty's own settings (the remembered game
/// folder). Game saves live in each game's `sav/` and are not touched.
pub fn reset_data() -> Result<String, String> {
    let cfg = crate::settings::config_path().ok_or("HOME is not set")?;
    let dir = cfg.parent().map(Path::to_path_buf).unwrap_or(cfg);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(format!("Removed {} (settings and the game list); the next start will ask for a game folder.", dir.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("Nothing to reset.".into()),
        Err(e) => Err(format!("{}: {}", dir.display(), e)),
    }
}

/// Where `--upgrade` keeps its checkout: `$XDG_DATA_HOME/emuera-tty/src`.
fn source_dir() -> Option<PathBuf> {
    Some(crate::sys::data_base()?.join("emuera-tty").join("src"))
}

pub const REPO_URL: &str = "https://github.com/astartes-gaming/emuera-tty.git";

/// Run a command, showing its output; error text if it fails.
fn run_cmd(cmd: &mut std::process::Command) -> Result<(), String> {
    let shown = format!("{:?}", cmd);
    eprintln!("$ {}", shown.replace('"', ""));
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{} failed ({})", shown, s)),
        Err(e) => Err(format!("{}: {}", shown, e)),
    }
}

/// `--upgrade`: fetch the latest source (git clone / pull), build it with
/// cargo and install the result to the user binary folder. Needs git and cargo.
pub fn upgrade() -> Result<String, String> {
    use std::process::Command;
    let src = source_dir().ok_or("HOME is not set")?;
    if src.join(".git").is_dir() {
        run_cmd(Command::new("git").arg("-C").arg(&src).args(["pull", "--ff-only"]))?;
    } else {
        if let Some(p) = src.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("{}: {}", p.display(), e))?;
        }
        run_cmd(Command::new("git").args(["clone", "--depth", "1", REPO_URL]).arg(&src))?;
    }
    // cargo from PATH, else rustup's default location
    let cargo = crate::sys::home_dir()
        .map(|h| h.join(".cargo").join("bin").join(if cfg!(windows) { "cargo.exe" } else { "cargo" }))
        .filter(|c| c.is_file() && which("cargo").is_none())
        .unwrap_or_else(|| PathBuf::from("cargo"));
    run_cmd(Command::new(cargo).arg("build").arg("--release").arg("--manifest-path").arg(src.join("Cargo.toml")))?;
    let built = src.join("target").join("release").join(crate::sys::EXE_NAME);
    install_from(&built)
}

/// The git checkout a game folder belongs to: the folder itself or one of
/// the two above it (a game kept in a subfolder of its repository), never
/// further up, so a home folder under git isn't taken for the game's.
fn git_root(game: &Path) -> Option<PathBuf> {
    which("git")?;
    game.ancestors().take(3).find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

/// Why git couldn't reach a repository that asks for a login.
const LOGIN_WANTED: &str = "the repository asks for a login (it is for signed-in users, or isn't at this address any more)";

/// Did git fail for want of a login? (its own words; GitLab answers so for
/// a repository that doesn't exist, too)
fn needs_login(git_said: &str) -> bool {
    ["could not read Username", "could not read Password", "Authentication failed", "HTTP Basic: Access denied"].iter().any(|m| git_said.contains(m))
}

/// The host of an https:// address.
fn url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `git`, with `login` when there is one: handed over by a credential
/// helper of our own that reads it from the environment of this one
/// command (not its command line, where other users could see it), in
/// place of the user's helpers.
fn git_cmd(login: Option<&Login>) -> std::process::Command {
    let mut c = std::process::Command::new("git");
    if let Some(l) = login {
        c.args(["-c", "credential.helper=", "-c"]);
        c.arg(r#"credential.helper=!f() { test "$1" = get && printf 'username=%s\npassword=%s\n' "$EMUERA_GIT_USER" "$EMUERA_GIT_PASS"; }; f"#);
        c.env("EMUERA_GIT_USER", &l.user).env("EMUERA_GIT_PASS", &l.pass);
    }
    c
}

/// Where remembered logins are kept: `git-credentials` next to the
/// settings, in git's credential-store format (`https://user:pass@host`,
/// %-encoded), readable only by the user, as git's own store keeps them.
fn logins_path() -> Option<PathBuf> {
    Some(crate::settings::config_path()?.with_file_name("git-credentials"))
}

/// `s` with its %XX escapes decoded.
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The logins remembered in earlier sessions.
fn load_logins() -> Vec<Login> {
    let text = logins_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    parse_logins(&text)
}

fn parse_logins(text: &str) -> Vec<Login> {
    let mut v: Vec<Login> = Vec::new();
    for l in text.lines().map(str::trim) {
        let Some((cred, host)) = l.strip_prefix("https://").and_then(|r| r.trim_end_matches('/').rsplit_once('@')) else { continue };
        let Some((user, pass)) = cred.split_once(':') else { continue };
        let host = host.to_ascii_lowercase();
        if !host.is_empty() && !v.iter().any(|o| o.host == host) {
            v.push(Login { host, user: pct_decode(user), pass: pct_decode(pass), remember: true });
        }
    }
    v
}

/// Write the remembered ones of `logins` (none: the file goes).
fn save_logins(logins: &[Login]) -> Result<(), String> {
    save_logins_to(&logins_path().ok_or("no settings folder (HOME not set?)")?, logins)
}

fn save_logins_to(path: &Path, logins: &[Login]) -> Result<(), String> {
    let text = logins_text(logins);
    if text.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {}", path.display(), e)),
            _ => Ok(()),
        };
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {}", d.display(), e))?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    // (made before: readable only by the user from now on)
    #[cfg(unix)]
    let _ = std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    f.write_all(text.as_bytes()).map_err(|e| format!("{}: {}", path.display(), e))
}

/// The remembered ones of `logins`, a line each.
fn logins_text(logins: &[Login]) -> String {
    logins.iter().filter(|l| l.remember).map(|l| format!("https://{}:{}@{}\n", crate::download::enc(&l.user), crate::download::enc(&l.pass), l.host)).collect()
}

/// Does `l.host` accept login `l`? git is asked for a repository that
/// can't be there, in the user's own namespace: GitLab answers "not found"
/// to a login it accepts and "access denied" to one it refuses.
fn check_login(l: &Login, g: &mut Gauge, cancel: &mut dyn FnMut() -> bool) -> Result<bool, String> {
    let url = format!("https://{}/{}/emuera-tty-login-check.git", l.host, crate::download::enc(&l.user));
    let mut c = git_cmd(Some(l));
    c.args(["ls-remote", &url]);
    match run_watched(&mut c, &mut |s| g.step(s), cancel)? {
        (RunEnd::Ok, ..) => Ok(true),
        (RunEnd::Cancelled, ..) => Err(CANCELLED.into()),
        (RunEnd::Failed, _, err) if needs_login(&err) => Ok(false),
        (RunEnd::Failed, _, err) if err.contains("not found") => Ok(true),
        (RunEnd::Failed, _, err) => Err(settled(&err).lines().map(str::trim).rev().find(|l| !l.is_empty()).unwrap_or("git failed").to_string()),
    }
}

/// A repository as the downloader lists it.
fn repo_label(r: &crate::download::Repo) -> String {
    if r.branch.is_empty() {
        r.path.clone()
    } else {
        format!("{} · branch {}", r.path, r.branch)
    }
}

/// What `fetch` says when Esc stopped it.
const CANCELLED: &str = "Cancelled.";

/// GET `url` with curl (the downloader's wiki requests), `g` turning
/// meanwhile; `cancel` stops it.
fn fetch(url: &str, g: &mut Gauge, cancel: &mut dyn FnMut() -> bool) -> Result<String, String> {
    if which("curl").is_none() {
        return Err("curl isn't installed (or isn't on PATH): can't reach the wiki.".into());
    }
    let mut c = std::process::Command::new("curl");
    c.args(["--silent", "--show-error", "--location", "--fail", "--connect-timeout", "20", "--max-time", "90"]);
    c.args(["--user-agent", concat!("emuera-tty/", env!("CARGO_PKG_VERSION")), url]);
    match run_watched(&mut c, &mut |s| g.step(s), cancel)? {
        (RunEnd::Ok, out, _) => Ok(out),
        (RunEnd::Cancelled, ..) => Err(CANCELLED.into()),
        (RunEnd::Failed, _, err) => {
            let why = settled(&err).lines().map(str::trim).rev().find(|l| !l.is_empty()).unwrap_or("curl failed").to_string();
            Err(format!("Can't reach the wiki: {}", why.trim_start_matches("curl: ")))
        }
    }
}

/// The address `root` was cloned from (its `origin`), if any.
fn git_origin(root: &Path) -> Option<String> {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The folder `git clone` makes for `url`: its last path part without ".git".
fn repo_name(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let start = u.rfind(['/', ':', '\\']).map_or(0, |i| i + 1);
    let last = &u[start..];
    let name = last.strip_suffix(".git").unwrap_or(last);
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_string())
}

/// `~` and `~/…` mean the home folder.
fn expand_home(p: &str) -> PathBuf {
    let home = crate::sys::home_dir();
    if p == "~" {
        return home.unwrap_or_else(|| PathBuf::from(p));
    }
    match (p.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => PathBuf::from(p),
    }
}

/// The game folder in a fresh clone: `dir` itself or a folder up to `depth`
/// levels below it (hidden folders skipped).
fn find_game_dir(dir: &Path, depth: u8) -> Option<PathBuf> {
    if is_game_dir(dir) {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let mut subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    subs.sort();
    subs.into_iter().find_map(|d| find_game_dir(&d, depth - 1))
}

/// How a `git clone` ended.
enum CloneEnd {
    Done,
    /// stopped by the user (`cancel` said so)
    Cancelled,
    Failed(String),
}

/// Stop a running `git clone`: SIGTERM first (git then removes the folder it
/// was making and stops its helpers), SIGKILL if it is still there after 3
/// seconds. Where there are no signals it is simply killed.
fn stop_git(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // SAFETY: kill(2) on the pid of our own child
        let _ = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        for _ in 0..30 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// How far along a git command is, in percent, from its latest progress line
/// (`--progress`), its phases weighted by how long they usually take: the
/// server counting and compressing, receiving the objects, resolving deltas,
/// writing the files out. A phase not listed (git speaking another
/// language) counts on its own; None for a line without a percentage.
fn git_percent(line: &str) -> Option<u32> {
    const PHASES: [(&str, u32, u32); 9] = [
        ("Counting objects", 0, 2),
        ("Compressing objects", 2, 5),
        ("Receiving objects", 5, 80),
        ("Unpacking objects", 5, 90),
        ("Resolving deltas", 80, 90),
        ("Updating files", 90, 100),
        ("Checking out files", 90, 100),
        ("Filtering content", 90, 100),
        ("Downloading LFS objects", 0, 100),
    ];
    let line = line.trim_start_matches("remote:").trim_start();
    let (phase, rest) = line.split_once(':')?;
    let pct = rest.trim_start().split_once('%')?.0.trim().parse::<u32>().ok()?.min(100);
    let (from, to) = PHASES.iter().find(|p| p.0 == phase.trim()).map_or((0, 100), |p| (p.1, p.2));
    Some(from + (to - from) * pct / 100)
}

/// Git's output as a terminal would have left it: of each line, what comes
/// after its last carriage return (progress lines rewrite themselves).
fn settled(s: &str) -> String {
    s.split('\n').map(|l| l.trim_end_matches('\r').rsplit('\r').next().unwrap_or("")).collect::<Vec<_>>().join("\n")
}

/// How a command run by `run_watched` ended.
enum RunEnd {
    Ok,
    Failed,
    /// stopped by the user (`cancel` said so)
    Cancelled,
}

/// Run `c` (git, or curl for the downloader) without prompts, its progress
/// shown: its stderr is read as it comes and the latest line handed to
/// `progress` (`Quiet` after 100 ms of silence); `cancel` is asked about
/// every 100 ms and stops it when it says true. Returns how it ended, its
/// stdout and its stderr; Err when it can't be started.
fn run_watched(c: &mut std::process::Command, progress: &mut dyn FnMut(Step), cancel: &mut dyn FnMut() -> bool) -> Result<(RunEnd, String, String), String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
    use std::time::Duration;
    // (no password window either: GIT_ASKPASS set but empty stops git from
    // trying core.askPass and SSH_ASKPASS; the launcher asks itself)
    // LANGUAGE: git's messages in English (no catalogue for "en"), as the
    // launcher reads them (progress phases, "not found", a login wanted)
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).env("GIT_TERMINAL_PROMPT", "0").env("GIT_ASKPASS", "").env("LANGUAGE", "en");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        c.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let mut child = c.spawn().map_err(|e| format!("{}: {}", c.get_program().to_string_lossy(), e))?;
    // the pipes are read on threads of their own, so the keyboard can be
    // watched meanwhile. (Git's helpers may keep them open after git itself
    // is gone, so the threads are never waited for.)
    fn pump(mut p: impl Read + Send + 'static, err: bool, tx: Sender<(bool, Vec<u8>)>) {
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = p.read(&mut buf) {
                if n == 0 || tx.send((err, buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
    }
    let (tx, rx) = channel();
    if let Some(p) = child.stdout.take() {
        pump(p, false, tx.clone());
    }
    if let Some(p) = child.stderr.take() {
        pump(p, true, tx);
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut take = |(is_err, chunk): (bool, Vec<u8>), progress: &mut dyn FnMut(Step)| {
        if !is_err {
            out.extend_from_slice(&chunk);
            return;
        }
        err.extend_from_slice(&chunk);
        let tail = String::from_utf8_lossy(&err[err.len().saturating_sub(300)..]);
        if let Some(l) = tail.split(['\r', '\n']).map(str::trim).rev().find(|l| !l.is_empty()) {
            progress(Step::Line(l));
        }
    };
    let mut cancelled = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(m) => take(m, progress),
            Err(RecvTimeoutError::Timeout) => {
                // git is gone but something else holds a pipe: take what is left and go on
                if matches!(child.try_wait(), Ok(Some(_))) {
                    while let Ok(m) = rx.recv_timeout(Duration::from_millis(50)) {
                        take(m, progress);
                    }
                    break;
                }
                progress(Step::Quiet);
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if cancel() {
            cancelled = true;
            break;
        }
    }
    let end = if cancelled {
        stop_git(&mut child);
        RunEnd::Cancelled
    } else if child.wait().is_ok_and(|s| s.success()) {
        RunEnd::Ok
    } else {
        RunEnd::Failed
    };
    Ok((end, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned()))
}

/// `git clone` of `url` into `target` (on `branch` and with only the last
/// `depth` commits, when given), without prompts, its progress handed to
/// `progress`; `cancel` stops it (see `run_watched`). Returns the outcome and
/// everything git said (for the launcher's `g`).
fn git_clone(
    url: &str,
    branch: &str,
    depth: Option<u32>,
    target: &Path,
    login: Option<&Login>,
    progress: &mut dyn FnMut(Step),
    cancel: &mut dyn FnMut() -> bool,
) -> (CloneEnd, String) {
    let mut c = git_cmd(login);
    c.args(["clone", "--progress"]);
    if !branch.is_empty() {
        c.args(["--branch", branch]);
    }
    if let Some(d) = depth {
        c.args(["--depth", &d.to_string()]);
    }
    c.arg("--").arg(url).arg(target);
    let mut log = format!(
        "$ git clone{}{} {}\n",
        if branch.is_empty() { String::new() } else { format!(" --branch {}", branch) },
        depth.map(|d| format!(" --depth {}", d)).unwrap_or_default(),
        url
    );
    progress(Step::Start("Downloading (git clone)…"));
    let (end, _, err) = match run_watched(&mut c, progress, cancel) {
        Ok(r) => r,
        Err(e) => return (CloneEnd::Failed(e), log),
    };
    let text = settled(&err);
    log += &text;
    match end {
        RunEnd::Ok => (CloneEnd::Done, log),
        RunEnd::Cancelled => {
            log += "\n(cancelled)\n";
            (CloneEnd::Cancelled, log)
        }
        RunEnd::Failed if needs_login(&text) => (CloneEnd::Failed(format!("Clone failed: {}", LOGIN_WANTED)), log),
        RunEnd::Failed => {
            let why = text
                .lines()
                .map(str::trim)
                .rev()
                .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
                .and_then(|l| l.split_once(':').map(|x| x.1.trim().to_string()))
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "git clone didn't work".to_string());
            (CloneEnd::Failed(format!("Clone failed: {}", why)), log)
        }
    }
}

/// `root`'s HEAD as "<branch> @ <short hash>" ("detached @ <short hash>" when
/// no branch is checked out); None if it can't be read (no commit yet…).
fn git_head(root: &Path) -> Option<String> {
    let run = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    let hash = run(&["rev-parse", "--short", "HEAD"])?;
    Some(match run(&["symbolic-ref", "--short", "-q", "HEAD"]) {
        Some(branch) => format!("{} @ {}", branch, hash),
        None => format!("detached @ {}", hash),
    })
}

/// `git pull --ff-only` in `root`, without prompts (no password or editor
/// can be asked for here). The result is Ok: what changed (with the
/// "(<previous hash> -> <new hash>)" of an update); Err: why it didn't. The
/// second part is git's full output, for the launcher's `g`.
fn git_pull(root: &Path, login: Option<&Login>, progress: &mut dyn FnMut(Step)) -> (Result<String, String>, String) {
    let mut log = String::new();
    let res = git_pull_logged(root, &mut log, login, progress);
    if log.is_empty() {
        if let Err(e) = &res {
            log = e.clone();
        }
    }
    (res, log)
}

/// Games that keep their pictures in Git LFS (pops-tw): without `git lfs
/// pull` the pictures are pointer files, and scripts that measure them fail
/// (SPRITEWIDTH 0, a division by zero). Fetches them in the checkout at
/// `root` when its .gitattributes uses LFS; returns "" or a sentence on what
/// went wrong (to append to the launcher's note).
fn lfs_pull(root: &Path, log: &mut String, login: Option<&Login>, progress: &mut dyn FnMut(Step)) -> String {
    let lfs = std::fs::read_to_string(root.join(".gitattributes")).is_ok_and(|a| a.contains("filter=lfs"));
    if !lfs {
        return String::new();
    }
    progress(Step::Start("Fetching its pictures (git lfs pull)…"));
    let mut c = git_cmd(login);
    // (git-lfs shows progress only on a terminal unless told to)
    c.arg("-C").arg(root).args(["lfs", "pull"]).env("GIT_LFS_FORCE_PROGRESS", "1");
    let (end, out, err) = match run_watched(&mut c, progress, &mut || false) {
        Ok(r) => r,
        Err(e) => return format!(" Pictures (git lfs pull) failed: {}", e),
    };
    let err = settled(&err);
    log.push_str("$ git lfs pull\n");
    log.push_str(&err);
    log.push_str(&out);
    if matches!(end, RunEnd::Ok) {
        return String::new();
    }
    if err.contains("not a git command") {
        " Its pictures are in Git LFS: install git-lfs and update again.".into()
    } else {
        format!(" Pictures (git lfs pull) failed: {}", err.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("?"))
    }
}

fn git_pull_logged(root: &Path, log: &mut String, login: Option<&Login>, progress: &mut dyn FnMut(Step)) -> Result<String, String> {
    use std::process::{Command, Stdio};
    let git = |args: &[&str]| {
        let mut c = Command::new("git");
        c.arg("-C").arg(root).args(args).stdin(Stdio::null()).env("GIT_TERMINAL_PROMPT", "0");
        if std::env::var_os("GIT_SSH_COMMAND").is_none() {
            c.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
        }
        c.output().map_err(|e| format!("git: {}", e))
    };
    let head = |o: std::process::Output| String::from_utf8_lossy(&o.stdout).trim().to_string();
    let before = head(git(&["rev-parse", "HEAD"])?);
    progress(Step::Start("Downloading the changes (git pull)…"));
    let (end, out, err) = run_watched(git_cmd(login).arg("-C").arg(root).args(["pull", "--ff-only", "--progress"]), progress, &mut || false)?;
    // everything git said (fetch's "From …" and ref lines come on stderr,
    // the merge summary on stdout)
    let err = settled(&err);
    log.push_str("$ git pull --ff-only\n");
    log.push_str(&err);
    log.push_str(&out);
    if !matches!(end, RunEnd::Ok) && needs_login(&err) {
        return Err(format!("Update failed: {}", LOGIN_WANTED));
    }
    if !matches!(end, RunEnd::Ok) {
        // git's own explanation (its error: / fatal: line and the files
        // listed under it), not the fetch progress or hints
        let lines: Vec<&str> = err.lines().collect();
        let why = match lines.iter().position(|l| l.starts_with("error:") || l.starts_with("fatal:")) {
            Some(k) => {
                let mut w = lines[k].split_once(':').map_or("", |x| x.1).trim().to_string();
                for f in lines[k + 1..].iter().take_while(|l| l.starts_with('\t') || l.starts_with("  ")).take(3) {
                    w += " ";
                    w += f.trim();
                }
                w
            }
            None => lines.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("hint:")).unwrap_or("").to_string(),
        };
        return Err(format!("Update failed: {}", if why.is_empty() { "git pull didn't work".to_string() } else { why }));
    }
    let pics = lfs_pull(root, log, login, progress);
    let after = head(git(&["rev-parse", "HEAD"])?);
    if before == after {
        return Ok(format!("Already up to date.{}", pics));
    }
    let n = head(git(&["rev-list", "--count", &format!("{}..{}", before, after)])?);
    let files = head(git(&["diff", "--shortstat", &before, &after])?);
    let short = |h: &str| -> Result<String, String> { Ok(head(git(&["rev-parse", "--short", h])?)) };
    Ok(format!("Updated: {} new commit{} ({}) ({} -> {}).{}", n, if n == "1" { "" } else { "s" }, files, short(&before)?, short(&after)?, pics))
}

/// First `name` on PATH.
fn which(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{}.exe", name) } else { name.to_string() };
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(&exe)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_progress_lines() {
        // the phases weighted into one rising percentage
        assert_eq!(git_percent("remote: Counting objects:  50% (5/10)"), Some(1));
        assert_eq!(git_percent("Receiving objects:   0% (0/273)"), Some(5));
        assert_eq!(git_percent("Receiving objects:  60% (164/273), 1.20 MiB | 500.00 KiB/s"), Some(50));
        assert_eq!(git_percent("Resolving deltas: 100% (40/40), done."), Some(90));
        assert_eq!(git_percent("Updating files: 100% (20/20), done."), Some(100));
        assert_eq!(git_percent("Downloading LFS objects:  45% (9/20), 12 MB | 3.2 MB/s"), Some(45));
        // another language: its own percentage
        assert_eq!(git_percent("オブジェクトを受信中:  33% (1/3)"), Some(33));
        assert_eq!(git_percent("remote: Enumerating objects: 5, done."), None);
        assert_eq!(git_percent("From https://example.org/era"), None);
    }

    #[test]
    fn git_urls() {
        for u in ["https://gitgud.io/eracorrection/eraflappytengu.git", "git@github.com:a/b.git", "ssh://git@host/x", "file:///srv/game.git"] {
            assert!(is_git_url(u), "{}", u);
        }
        for p in ["eraTW", "/home/me/games/eraTW", "C:\\games\\eraTW", "https://example.org/eraTW.zip", "./a@b:c/d"] {
            assert!(!is_git_url(p), "{}", p);
        }
    }

    #[test]
    fn logins_reach_git() {
        assert_eq!(url_host("https://gitgud.io/era-games/eraAS.git").as_deref(), Some("gitgud.io"));
        assert_eq!(url_host("https://me@GitGud.io/x").as_deref(), Some("gitgud.io"));
        assert_eq!(url_host("git@gitgud.io:x/y.git"), None);
        assert!(needs_login("fatal: could not read Username for 'https://gitgud.io': terminal prompts disabled"));
        assert!(!needs_login("fatal: repository 'x' not found"));
        if which("git").is_none() {
            return;
        }
        // git asks our helper, which answers from the environment
        let l = Login { host: "example.org".into(), user: "me".into(), pass: "p a'ss\"$x".into(), remember: false };
        let mut c = git_cmd(Some(&l));
        c.args(["credential", "fill"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).env("GIT_TERMINAL_PROMPT", "0");
        let mut ch = c.spawn().unwrap();
        std::io::Write::write_all(ch.stdin.as_mut().unwrap(), b"protocol=https\nhost=example.org\n\n").unwrap();
        let out = String::from_utf8(ch.wait_with_output().unwrap().stdout).unwrap();
        assert!(out.contains("username=me\n") && out.contains("password=p a'ss\"$x\n"), "{}", out);
    }

    #[test]
    fn remembered_logins() {
        let ls = [
            Login { host: "gitgud.io".into(), user: "me@x".into(), pass: "p:a/s s%".into(), remember: true },
            Login { host: "github.com".into(), user: "u".into(), pass: "t".into(), remember: false },
        ];
        let text = logins_text(&ls);
        assert_eq!(text, "https://me%40x:p%3Aa%2Fs%20s%25@gitgud.io\n");
        let back = parse_logins(&text);
        assert_eq!(back.len(), 1);
        assert_eq!((back[0].host.as_str(), back[0].user.as_str(), back[0].pass.as_str(), back[0].remember), ("gitgud.io", "me@x", "p:a/s s%", true));
        // git's own store lines read too; junk is skipped
        assert_eq!(parse_logins("https://a:b@GitGud.io/\nnot a url\nhttps://nopass@x\n").len(), 1);
        assert_eq!(pct_decode("%E6%97%A5%zz"), "日%zz");
        // the file: readable only by the user, gone when nothing is remembered
        let dir = std::env::temp_dir().join(format!("emuera-tty-logins-{}", std::process::id()));
        let f = dir.join("git-credentials");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&f, "old").unwrap();
        save_logins_to(&f, &ls).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), text);
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&f).unwrap().permissions()) & 0o777, 0o600);
        save_logins_to(&f, &ls[1..]).unwrap();
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_output_settles() {
        assert_eq!(settled("Receiving objects:  50% (1/2)\rReceiving objects: 100% (2/2), done.\nok\r\n"), "Receiving objects: 100% (2/2), done.\nok\n");
        assert_eq!(padded("ab", 4), "ab  ");
        assert_eq!(padded("サンプル", 5), "サン ");
    }
}
