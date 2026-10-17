//! First-run game picker: a small full-screen directory browser for
//! choosing an Emuera game folder (one with `CSV/` and `ERB/` inside).
//!
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
    kind: Kind,
}

#[derive(Clone, PartialEq)]
enum Kind {
    /// play the current folder (it is a game)
    PlayHere,
    Up,
    Dir { path: PathBuf, game: bool },
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
    Click(usize),
    None,
}

struct Picker {
    dir: PathBuf,
    entries: Vec<Entry>,
    sel: usize,
    top: usize,
    hidden: bool,
    note: String,
    mice: Option<crate::mice::Mice>,
    pending: Vec<u8>,
}

/// Run the picker starting in `start`. Returns the chosen game folder, or
/// None if the user cancelled. Needs a terminal on stdin/stdout.
pub fn pick(start: &Path) -> Option<PathBuf> {
    let mut orig: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::isatty(0) == 0 || libc::isatty(1) == 0 || libc::tcgetattr(0, &mut orig) != 0 } {
        return None;
    }
    let mut raw = orig;
    unsafe {
        libc::cfmakeraw(&mut raw);
        libc::tcsetattr(0, libc::TCSANOW, &raw);
    }
    let term = std::env::var("TERM").unwrap_or_default();
    let is_vt = term == "linux" || term.starts_with("fbterm");
    let mice = if crate::mice::Mice::wanted(is_vt) { crate::mice::Mice::open().ok() } else { None };
    out(b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1015h\x1b[?1006h");
    let mut p = Picker { dir: start.to_path_buf(), entries: Vec::new(), sel: 0, top: 0, hidden: false, note: String::new(), mice, pending: Vec::new() };
    p.load();
    let chosen = p.run();
    out(b"\x1b[?1006l\x1b[?1015l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
    unsafe { libc::tcsetattr(0, libc::TCSANOW, &orig) };
    chosen
}

fn out(b: &[u8]) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(b);
    let _ = o.flush();
}

fn term_size() -> (usize, usize) {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
    if ws.ws_col == 0 { (80, 24) } else { (ws.ws_col as usize, ws.ws_row as usize) }
}

/// `s` cut to `w` display cells.
fn fit(s: &str, w: usize) -> String {
    let mut used = 0;
    s.chars()
        .take_while(|c| {
            used += UnicodeWidthChar::width(*c).unwrap_or(0);
            used <= w
        })
        .collect()
}

impl Picker {
    /// List the current folder.
    fn load(&mut self) {
        self.entries.clear();
        if is_game_dir(&self.dir) {
            let t = game_title(&self.dir).unwrap_or_else(|| "this game".into());
            self.entries.push(Entry { label: format!("▶ Play {}", t), kind: Kind::PlayHere });
        }
        if self.dir.parent().is_some() {
            self.entries.push(Entry { label: "../".into(), kind: Kind::Up });
        }
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default();
        dirs.retain(|d| self.hidden || !d.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
        dirs.sort_by_key(|d| d.file_name().map(|n| n.to_string_lossy().to_lowercase()));
        for d in dirs {
            let name = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let game = is_game_dir(&d);
            let label = if game {
                format!("★ {}/   ({})", name, game_title(&d).unwrap_or_else(|| "Emuera game".into()))
            } else {
                format!("  {}/", name)
            };
            self.entries.push(Entry { label, kind: Kind::Dir { path: d, game } });
        }
        // start on "Play" or the first folder rather than on ../
        self.sel = self.entries.iter().position(|e| e.kind != Kind::Up).unwrap_or(0);
        self.top = 0;
    }

    fn cd(&mut self, to: PathBuf) {
        let from = self.dir.clone();
        self.dir = to;
        self.load();
        // coming back up: keep the folder we came from selected
        if let Some(i) = self.entries.iter().position(|e| matches!(&e.kind, Kind::Dir { path, .. } if *path == from)) {
            self.sel = i;
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
        s += &format!("\x1b[1mChoose an Emuera game folder\x1b[0m  (★ = game)\r\n\x1b[2m{}\x1b[0m\r\n", fit(&self.dir.display().to_string(), cols));
        for (i, e) in self.entries.iter().enumerate().skip(self.top).take(rows) {
            let line = fit(&e.label, cols.saturating_sub(1));
            let pad = cols.saturating_sub(line.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>() + 1);
            let color = match e.kind {
                Kind::PlayHere | Kind::Dir { game: true, .. } => "\x1b[33m",
                _ => "",
            };
            if i == self.sel {
                s += &format!("{}\x1b[7m {}{}\x1b[0m\r\n", color, line, " ".repeat(pad));
            } else {
                s += &format!("{} {}\x1b[0m\r\n", color, line);
            }
        }
        if self.entries.is_empty() {
            s += "  (empty)\r\n";
        }
        let help = "↑↓ move  Enter play/open  → open  ← up  ~ home  . hidden  q quit";
        let (_, trows) = term_size();
        s += &format!("\x1b[{};1H\x1b[0m{}\x1b[{};1H\x1b[2m{}\x1b[0m", trows.saturating_sub(1), fit(&self.note, cols), trows, fit(help, cols));
        out(s.as_bytes());
    }

    fn run(&mut self) -> Option<PathBuf> {
        loop {
            self.draw();
            let act = self.next_act();
            self.note.clear();
            let n = self.entries.len();
            match act {
                Act::Move(d) => self.sel = (self.sel as isize + d).clamp(0, n.saturating_sub(1) as isize) as usize,
                Act::Page(d) => {
                    let r = self.list_rows() as isize;
                    self.sel = (self.sel as isize + d * r).clamp(0, n.saturating_sub(1) as isize) as usize;
                }
                Act::Top => self.sel = 0,
                Act::Bottom => self.sel = n.saturating_sub(1),
                Act::Click(i) if i < n => {
                    if i == self.sel {
                        if let Some(p) = self.activate() {
                            return Some(p);
                        }
                    } else {
                        self.sel = i;
                    }
                }
                Act::Activate => {
                    if let Some(p) = self.activate() {
                        return Some(p);
                    }
                }
                Act::Open => {
                    if let Some(Entry { kind: Kind::Dir { path, .. }, .. }) = self.entries.get(self.sel) {
                        let p = path.clone();
                        self.cd(p);
                    } else if self.entries.get(self.sel).is_some_and(|e| e.kind == Kind::Up) {
                        self.go_up();
                    }
                }
                Act::Up => self.go_up(),
                Act::Home => {
                    if let Some(h) = std::env::var_os("HOME") {
                        self.cd(PathBuf::from(h));
                    }
                }
                Act::ToggleHidden => {
                    self.hidden = !self.hidden;
                    self.load();
                }
                Act::Cancel => return None,
                _ => {}
            }
        }
    }

    fn go_up(&mut self) {
        if let Some(p) = self.dir.parent().map(Path::to_path_buf) {
            self.cd(p);
        }
    }

    /// Enter on the selected entry: play a game, otherwise open the folder.
    fn activate(&mut self) -> Option<PathBuf> {
        let kind = self.entries.get(self.sel)?.kind.clone();
        match kind {
            Kind::PlayHere => return Some(self.dir.clone()),
            Kind::Up => self.go_up(),
            Kind::Dir { path, game: true } => return Some(path),
            Kind::Dir { path, game: false } => {
                self.cd(path);
                if self.entries.iter().all(|e| !matches!(e.kind, Kind::PlayHere | Kind::Dir { .. })) {
                    self.note = "No folders here.".into();
                }
            }
        }
        None
    }

    /// Wait for input and turn it into an action.
    fn next_act(&mut self) -> Act {
        loop {
            if let Some((a, used)) = self.parse() {
                self.pending.drain(..used);
                return a;
            }
            let mut pfd = [
                libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.mice.as_ref().map_or(-1, |m| m.fd()), events: libc::POLLIN, revents: 0 },
            ];
            if unsafe { libc::poll(pfd.as_mut_ptr(), 2, 500) } <= 0 {
                // redraw on resize
                return Act::None;
            }
            if pfd[1].revents & libc::POLLIN != 0 {
                let (cols, rows) = term_size();
                let evs = self.mice.as_mut().map(|m| m.read((8, 16), cols, rows)).unwrap_or_default();
                for (btn, _x, y, press) in evs {
                    if let Some(a) = self.mouse(btn, y, press) {
                        return a;
                    }
                }
            }
            if pfd[0].revents & libc::POLLIN != 0 {
                let mut b = [0u8; 256];
                let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
                if n <= 0 {
                    return Act::Cancel;
                }
                self.pending.extend_from_slice(&b[..n as usize]);
            }
        }
    }

    /// A mouse event at 1-based row `y`.
    fn mouse(&self, btn: u16, y: u16, press: bool) -> Option<Act> {
        match btn & !(4 | 8 | 16) {
            64 => Some(Act::Move(-3)),
            65 => Some(Act::Move(3)),
            0 if press => {
                let row = (y as usize).checked_sub(3)?; // list starts on screen row 3
                (row < self.list_rows()).then_some(Act::Click(self.top + row))
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
                b'g' => Act::Top,
                b'G' => Act::Bottom,
                _ => Act::None,
            };
            return Some((a, 1));
        }
        if b.len() == 1 {
            // lone ESC unless more bytes arrive right away
            let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
            return if unsafe { libc::poll(&mut pfd, 1, 30) } > 0 { None } else { Some((Act::Cancel, 1)) };
        }
        if b[1] != b'[' {
            return Some((Act::None, 2));
        }
        // X10 mouse: ESC [ M b x y
        if b.get(2) == Some(&b'M') {
            if b.len() < 6 {
                return None;
            }
            let (cb, y) = (b[3].wrapping_sub(32) as u16, b[5].wrapping_sub(32) as u16);
            let release = cb & 3 == 3 && cb & (32 | 64) == 0;
            return Some((self.mouse(if release { 3 } else { cb }, y, !release).unwrap_or(Act::None), 6));
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
            (b'~', "5") => Act::Page(-1),
            (b'~', "6") => Act::Page(1),
            (b'~', "1") | (b'~', "7") => Act::Top,
            (b'~', "4") | (b'~', "8") => Act::Bottom,
            // SGR mouse ESC [ < b ; x ; y M/m, urxvt ESC [ b ; x ; y M
            (b'M' | b'm', p) => {
                let sgr = p.starts_with('<');
                let v: Vec<u16> = p.trim_start_matches('<').split(';').filter_map(|x| x.parse().ok()).collect();
                match v.as_slice() {
                    [cb, _x, y] if sgr => self.mouse(*cb, *y, b[end] == b'M').unwrap_or(Act::None),
                    [cb, _x, y] => {
                        let cb = cb.saturating_sub(32);
                        let release = cb & 3 == 3 && cb & (32 | 64) == 0;
                        self.mouse(cb, *y, !release).unwrap_or(Act::None)
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

/// `$XDG_CONFIG_HOME/emuera-tty/config` (default `~/.config/…`).
fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("emuera-tty").join("config"))
}

/// The game folder chosen last time, if it still looks like a game.
pub fn last_game() -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    let p = PathBuf::from(text.lines().find_map(|l| l.strip_prefix("game="))?.trim());
    is_game_dir(&p).then_some(p)
}

/// Remember `game` for the next run without arguments.
pub fn remember(game: &Path) {
    let Some(cfg) = config_path() else { return };
    let game = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
    if let Some(d) = cfg.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&cfg, format!("game={}\n", game.display()));
}

// ------------------------------------------------------------- installing

/// `--install`: copy this binary to `~/.local/bin/emuera-tty`.
pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find this program: {}", e))?;
    install_from(&exe)
}

/// Copy the binary `exe` to `~/.local/bin/emuera-tty`.
fn install_from(exe: &Path) -> Result<String, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let dir = PathBuf::from(home).join(".local").join("bin");
    let dest = dir.join("emuera-tty");
    if dest.canonicalize().ok() == exe.canonicalize().ok() {
        return Ok(format!("{} is already installed.", dest.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    // write beside the target and rename, so replacing a running copy is safe
    let tmp = dir.join(".emuera-tty.new");
    std::fs::copy(exe, &tmp).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == dir));
    let mut msg = format!("Installed {}.", dest.display());
    if !on_path {
        msg += &format!("\n{} is not on your PATH; add this to ~/.bashrc or ~/.profile:\n  export PATH=\"$HOME/.local/bin:$PATH\"", dir.display());
    }
    Ok(msg)
}

/// `--uninstall`: remove `~/.local/bin/emuera-tty`.
pub fn uninstall() -> Result<String, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let dest = PathBuf::from(home).join(".local").join("bin").join("emuera-tty");
    match std::fs::remove_file(&dest) {
        Ok(()) => Ok(format!("Removed {}.", dest.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(format!("{} is not installed.", dest.display())),
        Err(e) => Err(format!("{}: {}", dest.display(), e)),
    }
}

/// `--reset-data`: forget emuera-tty's own settings (the remembered game
/// folder). Game saves live in each game's `sav/` and are not touched.
pub fn reset_data() -> Result<String, String> {
    let cfg = config_path().ok_or("HOME is not set")?;
    let dir = cfg.parent().map(Path::to_path_buf).unwrap_or(cfg);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(format!("Removed {}; the next start will ask for a game folder.", dir.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("Nothing to reset.".into()),
        Err(e) => Err(format!("{}: {}", dir.display(), e)),
    }
}

/// Where `--upgrade` keeps its checkout: `$XDG_DATA_HOME/emuera-tty/src`.
fn source_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
    Some(base.join("emuera-tty").join("src"))
}

pub const REPO_URL: &str = "https://github.com/diegosace2000/emuera-tty.git";

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
/// cargo and install the result to `~/.local/bin`. Needs git and cargo.
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
    let cargo = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cargo").join("bin").join("cargo"))
        .filter(|c| c.is_file() && which("cargo").is_none())
        .unwrap_or_else(|| PathBuf::from("cargo"));
    run_cmd(Command::new(cargo).arg("build").arg("--release").arg("--manifest-path").arg(src.join("Cargo.toml")))?;
    let built = src.join("target").join("release").join("emuera-tty");
    install_from(&built)
}

/// First `name` on PATH.
fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}
