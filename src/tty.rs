//! Full-screen terminal frontend: raw-mode input with mouse support, a
//! redraw-per-frame renderer for the Emuera console, and inline images via
//! the kitty / sixel / framebuffer backends.
//!
//! Each frame is rebuilt from the [`Console`]: lines are laid out bottom-up
//! into screen rows (`layout_line`, which wraps and aligns), the visible
//! window is chosen from the scroll offset, and the whole screen is written
//! in one go. While drawing, the positions of buttons are recorded
//! (`btn_map`) so mouse clicks and hover can be mapped back to values.
//!
//! Input is read raw from stdin and decoded into [`Key`]s: UTF-8 text,
//! cursor/paging keys, and mouse reports in SGR, urxvt or X10 encoding.
//! On the Linux console the mouse comes from `/dev/input/mice` instead
//! (`crate::mice`). Colours are downgraded to 256 or 16 when the terminal
//! can't do 24-bit.

use crate::ast::Align;
use crate::console::{Console, Line, Rgb, Seg, Style};
use crate::frontend::{Frontend, InputKind, InputReq, InputResult};
use crate::gfx::{self, Backend, Placement};
use crate::resources::Resources;
use crate::vm::value::Value;
use std::io::Write;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

/// How many rows above / below the view are searched for images that
/// reach into it (portraits are about 15 rows tall).
const IMG_REACH: usize = 40;

/// How many colours the terminal can show (picked from TERM / COLORTERM).
#[derive(Clone, Copy, PartialEq)]
enum ColorMode {
    True,
    C256,
    C16,
}

/// One screen cell after layout; `w` is 2 for wide (CJK) characters.
#[derive(Clone)]
struct Cell {
    ch: char,
    w: u8,
    style: Style,
    button: Option<usize>,
}

/// An image anchored at a column of a screen row.
struct ImgAt {
    name: String,
    col: usize,
    w: f32,
    h: f32,
    ypos: f32,
}

/// One laid-out screen row.
#[derive(Default)]
struct DRow {
    cells: Vec<Cell>,
    imgs: Vec<ImgAt>,
}

/// A decoded input event. Mouse `btn` uses xterm codes: 0/1/2 buttons,
/// +32 motion, 64/65 wheel; `x`/`y` are 1-based cells.
#[derive(Debug)]
enum Key {
    Char(char),
    Enter,
    Backspace,
    Esc,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    CtrlC,
    CtrlL,
    Mouse { btn: u16, x: u16, y: u16, press: bool },
    Other,
}

pub struct Tty {
    orig: libc::termios,
    gfx: Box<dyn Backend>,
    cell: (u32, u32),
    cols: usize,
    rows: usize,
    color: ColorMode,
    scroll: usize,
    buf: String,
    pending: Vec<u8>,
    /// keys decoded but not yet handed out (console mouse events)
    queued: std::collections::VecDeque<Key>,
    /// pointer on the Linux console via /dev/input/mice (no terminal reports)
    mice: Option<crate::mice::Mice>,
    mouse: Option<(usize, usize)>,
    /// How characters Emuera draws two columns wide but the terminal draws
    /// one column wide are handled (detected at startup, see `char_cells`).
    ambi: Ambi,
    /// Widths this terminal actually draws, for characters where tables
    /// disagree (measured at startup; only entries that differ from
    /// unicode-width are kept).
    widths: std::collections::HashMap<char, u8>,
    /// per screen row: (col_start, col_end, button id)
    btn_map: Vec<Vec<(usize, usize, usize)>>,
    buttons: Vec<Value>,
    hover: Option<usize>,
    active: bool,
    /// the terminal went away (hangup / end of input): quit instead of
    /// waiting forever on a dead stdin
    hangup: bool,
    /// a setup problem the user should see (e.g. no console mouse)
    notice: Option<String>,
    fb_cell_fixed: bool,
}

/// Terminal size: (cols, rows, pixel width, pixel height); pixels are 0
/// when the terminal doesn't report them.
fn winsize() -> (usize, usize, u32, u32) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            (ws.ws_col as usize, ws.ws_row as usize, ws.ws_xpixel as u32, ws.ws_ypixel as u32)
        } else {
            (80, 24, 0, 0)
        }
    }
}

/// Decode an X10 / urxvt button byte (already minus 32). Releases come as
/// button 3 without saying which button: report them as a left release.
fn x10_mouse(cb: u16, x: u16, y: u16) -> Key {
    let release = cb & 3 == 3 && cb & (32 | 64) == 0;
    Key::Mouse { btn: if release { cb & !3 } else { cb }, x, y, press: !release }
}

/// How to draw one character so the screen matches Emuera's layout:
/// (cells the terminal draws it in, filler cells to add, filler char).
///
/// Emuera games are laid out for a Japanese font where every Shift_JIS
/// double-byte character is two columns wide, including box drawing
/// (`━┃┏`), blocks (`■□`) and symbols (`※○→`). Most terminals draw those
/// "East Asian ambiguous" characters one column wide, which skews ASCII
/// maps. The missing column is filled: line and block characters continue
/// themselves (`━━`, `┏━`, `■■`) so walls stay solid; others get a space.
/// With [`Ambi::Osc66`] such a character is instead drawn as one 2-cell
/// glyph (no filler); with [`Ambi::Wide`] the terminal already draws it wide.
/// `measured`: the width this terminal was seen to draw `ch` at (see
/// `Tty::measure_widths`), which beats any table.
fn char_cells(ch: char, ambi: Ambi, measured: Option<u8>) -> (usize, usize, char) {
    let tw = match measured {
        Some(w) => w as usize,
        None => if ambi == Ambi::Wide { UnicodeWidthChar::width_cjk(ch) } else { UnicodeWidthChar::width(ch) }.unwrap_or(0),
    };
    // a terminal with no double-width cells (the Linux console before
    // 6.15) draws every character in one cell
    let tw = if ambi == Ambi::Narrow { tw.min(1) } else { tw };
    if tw == 0 || ch.is_ascii() {
        return (tw, 0, ' ');
    }
    let mut buf = [0u8; 4];
    let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(ch.encode_utf8(&mut buf));
    let emuera = if unmappable { tw } else { bytes.len() };
    let fill = match ch {
        '━' | '┏' | '┗' | '┣' | '┳' | '┻' | '╋' | '┯' | '┷' | '┿' | '┝' | '┍' | '┕' => '━',
        '─' | '┌' | '└' | '├' | '┬' | '┴' | '┼' | '┠' | '┰' | '┸' | '╂' | '┎' | '┖' => '─',
        '═' | '╔' | '╚' | '╠' | '╦' | '╩' | '╬' => '═',
        '┄' | '┅' | '┈' | '┉' | '╌' | '╍' | '■' | '█' | '▓' | '▒' | '░' | '―' | '‐' | '〓' | '≡' | 'Ⅱ' | 'Ⅲ' => ch,
        _ => ' ',
    };
    let extra = emuera.saturating_sub(tw);
    if ambi == Ambi::Osc66 && extra > 0 {
        return (tw + extra, 0, ' ');
    }
    (tw, extra, fill)
}

/// Strategy for East Asian "ambiguous width" characters (`━ ■ ※ …`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ambi {
    /// draw one cell, then a filler cell (works everywhere)
    Pad,
    /// the terminal already draws them two cells wide
    Wide,
    /// the terminal draws even CJK in one cell (Linux console): every wide
    /// character is padded to its Emuera width
    Narrow,
    /// kitty's text sizing protocol: `OSC 66 ; w=2 ; ch` draws the glyph
    /// across two cells (opt-in: kitty's rendering of it doesn't match
    /// Emuera's font well)
    Osc66,
}

/// Write to the terminal and flush.
fn write_all(b: &[u8]) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b);
    let _ = out.flush();
}

impl Tty {
    /// Enter raw mode and the alternate screen, pick colour and graphics
    /// support, open the console mouse if needed, and enable mouse reports.
    pub fn new(gfx_pref: &str) -> Result<Tty, String> {
        unsafe {
            if libc::isatty(0) == 0 || libc::isatty(1) == 0 {
                return Err("stdin/stdout is not a terminal".into());
            }
        }
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(0, &mut orig) } != 0 {
            return Err("tcgetattr failed".into());
        }
        let mut raw = orig;
        unsafe { libc::cfmakeraw(&mut raw) };
        raw.c_oflag |= libc::OPOST | libc::ONLCR;
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) };
        let term = std::env::var("TERM").unwrap_or_default();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let color = if term == "linux" || term.starts_with("fbterm") {
            ColorMode::C16
        } else if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("kitty") || term.contains("foot") || term.contains("direct") {
            ColorMode::True
        } else if term.contains("256") {
            ColorMode::C256
        } else {
            ColorMode::True
        };
        let (cols, rows, xp, yp) = winsize();
        let mut t = Tty {
            orig,
            gfx: Box::new(gfx::NoGfx),
            cell: (8, 16),
            cols,
            rows,
            color,
            scroll: 0,
            buf: String::new(),
            pending: Vec::new(),
            queued: std::collections::VecDeque::new(),
            mice: None,
            mouse: None,
            ambi: Ambi::Pad,
            widths: std::collections::HashMap::new(),
            btn_map: Vec::new(),
            buttons: Vec::new(),
            hover: None,
            active: true,
            hangup: false,
            notice: None,
            fb_cell_fixed: false,
        };
        if xp > 0 && yp > 0 {
            t.cell = (xp / cols as u32, yp / rows as u32);
        }
        t.setup_gfx(gfx_pref, &term);
        t.ambi = t.detect_ambiguous();
        // (every width is 1 there: nothing to measure)
        if t.ambi != Ambi::Narrow {
            t.widths = t.measure_widths();
        }
        let is_vt = term == "linux" || term.starts_with("fbterm");
        if crate::mice::Mice::wanted(is_vt) {
            // gpm first (works without extra permissions), else the device
            let gpm = if std::env::var("EMUERA_MOUSE").as_deref() == Ok("mice") { Err(String::new()) } else { crate::mice::Mice::open_gpm() };
            match gpm.or_else(|_| crate::mice::Mice::open()) {
                Ok(m) => t.mice = Some(m),
                Err(e) => {
                    // shown on the input line (and again on exit): the
                    // startup text is gone once the screen is drawn
                    t.notice = Some(format!("no mouse ({}): install gpm, or add yourself to the input group", e));
                }
            }
        }
        // alt screen, hide cursor; mouse reports: buttons, drag, any motion,
        // in SGR (1006) or urxvt (1015) form where supported, else X10
        write_all(b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h\x1b[2J");
        Ok(t)
    }

    /// Wait up to `timeout_ms` for input on stdin (and the console mouse);
    /// returns whether anything arrived.
    fn read_bytes(&mut self, timeout_ms: i32) -> bool {
        let mut pfd = [
            libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: self.mice.as_ref().map_or(-1, |m| m.fd()), events: libc::POLLIN, revents: 0 },
        ];
        let r = unsafe { libc::poll(pfd.as_mut_ptr(), 2, timeout_ms) };
        if r <= 0 {
            return false;
        }
        if pfd[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 && pfd[0].revents & libc::POLLIN == 0 {
            self.hangup = true;
            return false;
        }
        if pfd[1].revents & libc::POLLIN != 0 {
            let (cell, cols, rows) = (self.cell, self.cols, self.rows);
            if let Some(m) = &mut self.mice {
                for (btn, x, y, press) in m.read(cell, cols, rows) {
                    self.queued.push_back(Key::Mouse { btn, x, y, press });
                }
            }
        }
        if pfd[0].revents & libc::POLLIN == 0 {
            return !self.queued.is_empty();
        }
        let mut b = [0u8; 4096];
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
        if n > 0 {
            self.pending.extend_from_slice(&b[..n as usize]);
            true
        } else {
            // 0: end of input; an error other than "try again"
            let e = std::io::Error::last_os_error().raw_os_error();
            if n == 0 || !matches!(e, Some(libc::EINTR) | Some(libc::EAGAIN)) {
                self.hangup = true;
            }
            false
        }
    }

    /// Is unread input waiting on stdin right now?
    fn input_waiting(&self) -> bool {
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut pfd, 1, 0) > 0 && pfd.revents & libc::POLLIN != 0 }
    }

    /// Send a query and collect the reply until `done` or timeout.
    fn query(&mut self, q: &[u8], done: impl Fn(&[u8]) -> bool, ms: u64) -> Vec<u8> {
        self.pending.clear();
        write_all(q);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            self.read_bytes(20);
            if done(&self.pending) {
                break;
            }
        }
        std::mem::take(&mut self.pending)
    }

    /// Ask the terminal how wide it draws each character that is two columns
    /// in Emuera but whose width terminals disagree on (Shift_JIS
    /// double-byte symbols, box drawing, Greek/Cyrillic, kana; ~800). All
    /// probes go out in one write — `\r ch CSI 6n` each — and the replies'
    /// cursor columns give the widths. Terminals that don't answer leave the
    /// table empty. `EMUERA_MEASURE=0` skips this.
    fn measure_widths(&mut self) -> std::collections::HashMap<char, u8> {
        let mut out = std::collections::HashMap::new();
        if std::env::var("EMUERA_MEASURE").as_deref() == Ok("0") {
            return out;
        }
        let chars: Vec<char> = (0xA0u32..0x3400)
            .chain(0xFF00..0xFFF0)
            .filter_map(char::from_u32)
            .filter(|c| {
                let mut b = [0u8; 4];
                let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut b));
                !bad && bytes.len() == 2
            })
            .collect();
        let mut q = String::with_capacity(chars.len() * 8);
        for c in &chars {
            q.push('\r');
            q.push(*c);
            q.push_str("\x1b[6n");
        }
        q.push_str("\r\x1b[K");
        self.pending.clear();
        write_all(q.as_bytes());
        // collect replies until all arrived or the terminal goes quiet
        let count = |b: &[u8]| b.iter().filter(|&&x| x == b'R').count();
        let start = Instant::now();
        while count(&self.pending) < chars.len() && start.elapsed() < Duration::from_millis(1500) {
            if !self.read_bytes(150) && count(&self.pending) == 0 && start.elapsed() > Duration::from_millis(400) {
                break; // no replies at all: terminal doesn't answer
            }
        }
        let text = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).to_string();
        let cols: Vec<u32> = text
            .split('R')
            .filter_map(|r| r.rfind("\x1b[").and_then(|i| r[i + 2..].split(';').nth(1)).and_then(|c| c.parse().ok()))
            .collect();
        if cols.len() == chars.len() {
            for (c, col) in chars.iter().zip(cols) {
                let w = col.saturating_sub(1).min(2) as u8;
                // 0 = drew nothing / unknown: keep the table's answer
                if w > 0 && UnicodeWidthChar::width(*c) != Some(w as usize) {
                    out.insert(*c, w);
                }
            }
        }
        out
    }

    /// Pick the [`Ambi`] strategy: print `■` and ask where the cursor went
    /// (CSI 6n). Column 3 means the terminal already draws it wide; else pad.
    /// `EMUERA_AMBIGUOUS=wide|narrow|osc66` overrides. OSC 66 is opt-in only:
    /// kitty draws box drawing stretched (lines look doubled) and blocks
    /// small with a gap, so padding looks closer to Emuera.
    fn detect_ambiguous(&mut self) -> Ambi {
        match std::env::var("EMUERA_AMBIGUOUS").as_deref() {
            Ok("wide") => return Ambi::Wide,
            Ok("narrow") => return Ambi::Pad,
            Ok("osc66") => return Ambi::Osc66,
            Ok("single") => return Ambi::Narrow,
            _ => {}
        }
        let cursor_col = |t: &mut Tty, probe: &str| -> Option<u32> {
            let r = t.query(format!("\r\x1b[K{}\x1b[6n", probe).as_bytes(), |b| b.contains(&b'R'), 300);
            write_all(b"\r\x1b[K");
            let s = String::from_utf8_lossy(&r).to_string();
            s.rfind("\x1b[").and_then(|i| s[i + 2..].trim_end_matches('R').split(';').nth(1).and_then(|c| c.parse::<u32>().ok()))
        };
        // a kanji that moves the cursor one cell: no double-width at all
        if cursor_col(self, "漢") == Some(2) {
            return Ambi::Narrow;
        }
        if cursor_col(self, "■") == Some(3) {
            Ambi::Wide
        } else {
            Ambi::Pad
        }
    }

    /// Choose the image backend: framebuffer on a VT, otherwise probe for
    /// kitty graphics, then sixel (DA1 attribute 4); also learn the cell
    /// size in pixels so images can be scaled.
    fn setup_gfx(&mut self, pref: &str, term: &str) {
        let is_vt = term == "linux" || term.starts_with("fbterm");
        let try_fb = |t: &mut Tty| -> bool {
            match gfx::fb::Fb::open() {
                Ok(fb) => {
                    t.cell = ((fb.xres / t.cols as u32).max(1), (fb.yres / t.rows as u32).max(1));
                    t.fb_cell_fixed = true;
                    t.gfx = Box::new(fb);
                    true
                }
                Err(e) => {
                    eprintln!("framebuffer unavailable: {}", e);
                    false
                }
            }
        };
        match pref {
            "none" => return,
            "kitty" => self.gfx = Box::new(gfx::kitty::Kitty::new()),
            "sixel" => self.gfx = Box::new(gfx::sixel::Sixel::new()),
            "fb" => {
                try_fb(self);
            }
            _ => {
                if is_vt {
                    try_fb(self);
                } else {
                    // kitty graphics query + primary device attributes
                    let r = self.query(
                        b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c",
                        |b| {
                            let s = String::from_utf8_lossy(b);
                            s.contains("\x1b[?") && s.trim_end().ends_with('c')
                        },
                        400,
                    );
                    let s = String::from_utf8_lossy(&r).to_string();
                    if s.contains("_Gi=31;OK") {
                        self.gfx = Box::new(gfx::kitty::Kitty::new());
                    } else if let Some(i) = s.find("\x1b[?") {
                        let da = &s[i + 3..];
                        let da = &da[..da.find('c').unwrap_or(da.len())];
                        if da.split(';').any(|x| x == "4") {
                            self.gfx = Box::new(gfx::sixel::Sixel::new());
                        }
                    }
                }
            }
        }
        if self.gfx.name() != "fb" && self.gfx.name() != "none" {
            let (_, _, xp, yp) = winsize();
            if xp == 0 || yp == 0 {
                // CSI 16 t -> CSI 6 ; h ; w t
                let r = self.query(b"\x1b[16t", |b| b.ends_with(b"t"), 300);
                let s = String::from_utf8_lossy(&r).to_string();
                if let Some(i) = s.find("\x1b[6;") {
                    let p: Vec<u32> = s[i + 4..].trim_end_matches('t').split(';').filter_map(|x| x.parse().ok()).collect();
                    if p.len() == 2 && p[0] > 0 && p[1] > 0 {
                        self.cell = (p[1], p[0]);
                    }
                }
            }
        }
        eprintln!("graphics backend: {} (cell {}x{} px)", self.gfx.name(), self.cell.0, self.cell.1);
    }

    /// Decode the next key or mouse event, waiting up to `timeout_ms`.
    /// Incomplete escape sequences wait briefly for their remaining bytes.
    fn next_key(&mut self, timeout_ms: i32) -> Option<Key> {
        if let Some(k) = self.queued.pop_front() {
            return Some(k);
        }
        if self.pending.is_empty() && !self.read_bytes(timeout_ms) {
            return None;
        }
        if let Some(k) = self.queued.pop_front() {
            return Some(k);
        }
        if self.pending.is_empty() {
            return None;
        }
        let b = &self.pending;
        let (k, n) = match b[0] {
            0x1b => {
                if b.len() == 1 {
                    // lone ESC? wait briefly for the rest of a sequence
                    if !self.read_bytes(30) {
                        self.pending.clear();
                        return Some(Key::Esc);
                    }
                    return self.next_key(0);
                }
                let b = self.pending.clone();
                if b[1] == b'[' {
                    if b.get(2) == Some(&b'M') {
                        // X10 / normal mouse: ESC [ M Cb Cx Cy, each value + 32
                        if b.len() < 6 {
                            self.read_bytes(30);
                            if self.pending.len() < 6 {
                                self.pending.clear();
                                return Some(Key::Other);
                            }
                            return self.next_key(0);
                        }
                        let (cb, cx, cy) = (b[3].wrapping_sub(32) as u16, b[4].wrapping_sub(32) as u16, b[5].wrapping_sub(32) as u16);
                        (x10_mouse(cb, cx, cy), 6)
                    } else if b.get(2) == Some(&b'<') {
                        // SGR mouse: ESC [ < b ; x ; y (M|m)
                        match b.iter().position(|&c| c == b'M' || c == b'm') {
                            Some(end) => {
                                let s = String::from_utf8_lossy(&b[3..end]).to_string();
                                let p: Vec<u16> = s.split(';').filter_map(|x| x.parse().ok()).collect();
                                let k = if p.len() == 3 { Key::Mouse { btn: p[0], x: p[1], y: p[2], press: b[end] == b'M' } } else { Key::Other };
                                (k, end + 1)
                            }
                            None => {
                                self.read_bytes(30);
                                return self.next_key(0);
                            }
                        }
                    } else {
                        let end = b[2..].iter().position(|c| (0x40..=0x7e).contains(c)).map(|i| i + 2);
                        match end {
                            Some(e) => {
                                let k = match (&b[2..e], b[e]) {
                                    // urxvt 1015 mouse: ESC [ Cb ; x ; y M  (Cb + 32, decimal)
                                    (p, b'M') if p.iter().filter(|&&c| c == b';').count() == 2 => {
                                        let v: Vec<u16> = String::from_utf8_lossy(p).split(';').filter_map(|x| x.parse().ok()).collect();
                                        if v.len() == 3 {
                                            x10_mouse(v[0].saturating_sub(32), v[1], v[2])
                                        } else {
                                            Key::Other
                                        }
                                    }
                                    (_, b'A') => Key::Up,
                                    (_, b'B') => Key::Down,
                                    (_, b'H') => Key::Home,
                                    (_, b'F') => Key::End,
                                    (p, b'~') if p == b"5" => Key::PageUp,
                                    (p, b'~') if p == b"6" => Key::PageDown,
                                    (p, b'~') if p == b"1" || p == b"7" => Key::Home,
                                    (p, b'~') if p == b"4" || p == b"8" => Key::End,
                                    _ => Key::Other,
                                };
                                (k, e + 1)
                            }
                            None => (Key::Other, b.len()),
                        }
                    }
                } else if b[1] == b'O' && b.len() >= 3 {
                    (Key::Other, 3)
                } else {
                    (Key::Esc, 1)
                }
            }
            b'\r' | b'\n' => (Key::Enter, 1),
            0x7f | 0x08 => (Key::Backspace, 1),
            0x03 => (Key::CtrlC, 1),
            0x0c => (Key::CtrlL, 1),
            c if c < 0x20 => (Key::Other, 1),
            c => {
                let len = if c >= 0xf0 {
                    4
                } else if c >= 0xe0 {
                    3
                } else if c >= 0xc0 {
                    2
                } else {
                    1
                };
                if b.len() < len {
                    self.read_bytes(30);
                    if self.pending.len() < len {
                        self.pending.clear();
                        return Some(Key::Other);
                    }
                }
                let s = String::from_utf8_lossy(&self.pending[..len]).to_string();
                (Key::Char(s.chars().next().unwrap_or('?')), len)
            }
        };
        let n = n.min(self.pending.len());
        self.pending.drain(..n);
        Some(k)
    }

    // ------------------------------------------------------------ layout
    /// Wrap one console line into screen rows at the width it was printed
    /// with (`Line::cols`) and apply its alignment. Rows wider than the
    /// current screen are clipped by `render`. Buttons get ids (indices into
    /// `self.buttons`) only if the line belongs to the current input generation.
    fn layout_line(&mut self, line: &Line, cur_gen: u64, scale: f32) -> Vec<DRow> {
        let width = if line.cols > 0 { line.cols } else { self.cols.max(1) };
        let mut rows = vec![DRow::default()];
        let cw = self.cell.0.max(1) as f32;
        let mut col = 0usize;
        let active = line.gen == cur_gen;
        let push_btn = |v: &Option<Value>, buttons: &mut Vec<Value>| -> Option<usize> {
            match v {
                Some(v) if active => {
                    buttons.push(v.clone());
                    Some(buttons.len() - 1)
                }
                _ => None,
            }
        };
        for seg in &line.segs {
            match seg {
                Seg::Text { text, style, button, .. } => {
                    let bid = push_btn(button, &mut self.buttons);
                    for ch in text.chars() {
                        let (tw, extra, fill) = if ch == '\t' { (4, 0, ' ') } else { char_cells(ch, self.ambi, self.widths.get(&ch).copied()) };
                        let w = tw + extra;
                        if tw == 0 {
                            continue;
                        }
                        if col + w > width {
                            rows.push(DRow::default());
                            col = 0;
                        }
                        let r = rows.last_mut().unwrap();
                        if ch == '\t' {
                            for _ in 0..4 {
                                r.cells.push(Cell { ch: ' ', w: 1, style: style.clone(), button: bid });
                            }
                        } else {
                            r.cells.push(Cell { ch, w: tw as u8, style: style.clone(), button: bid });
                            for _ in 0..extra {
                                r.cells.push(Cell { ch: fill, w: 1, style: style.clone(), button: bid });
                            }
                        }
                        col += w;
                    }
                }
                Seg::Image { name, w, h, ypos, button } => {
                    let bid = push_btn(button, &mut self.buttons);
                    let cells = ((w * scale) / cw).round().max(1.0) as usize;
                    if col + cells > width && col > 0 {
                        rows.push(DRow::default());
                        col = 0;
                    }
                    let r = rows.last_mut().unwrap();
                    r.imgs.push(ImgAt { name: name.clone(), col, w: *w, h: *h, ypos: *ypos });
                    let st = Style { fg: Rgb(90, 90, 90), bg: None, bold: false, italic: false, underline: false, strike: false };
                    let fill = if self.gfx.name() == "none" { '░' } else { ' ' };
                    for _ in 0..cells.min(width) {
                        r.cells.push(Cell { ch: fill, w: 1, style: st.clone(), button: bid });
                    }
                    col += cells.min(width);
                }
                Seg::Space { w } => {
                    let cells = ((w * scale) / cw).round().max(0.0) as usize;
                    let r = rows.last_mut().unwrap();
                    let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
                    for _ in 0..cells.min(width.saturating_sub(col)) {
                        r.cells.push(Cell { ch: ' ', w: 1, style: st.clone(), button: None });
                    }
                    col = (col + cells).min(width);
                }
                Seg::Rect { w, color, button, .. } => {
                    let bid = push_btn(button, &mut self.buttons);
                    let cells = ((w * scale) / cw).round().max(1.0) as usize;
                    if col + cells > width && col > 0 {
                        rows.push(DRow::default());
                        col = 0;
                    }
                    let r = rows.last_mut().unwrap();
                    let st = Style { fg: *color, bg: None, bold: false, italic: false, underline: false, strike: false };
                    for _ in 0..cells.min(width) {
                        r.cells.push(Cell { ch: '█', w: 1, style: st.clone(), button: bid });
                    }
                    col += cells.min(width);
                }
            }
        }
        // alignment, within the screen as it is now
        if line.align != Align::Left {
            let screen = self.cols.max(1);
            for r in rows.iter_mut() {
                let used: usize = r.cells.iter().map(|c| c.w as usize).sum();
                let pad = match line.align {
                    Align::Center => (screen.saturating_sub(used)) / 2,
                    _ => screen.saturating_sub(used),
                };
                if pad > 0 {
                    let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
                    let mut nc: Vec<Cell> = (0..pad).map(|_| Cell { ch: ' ', w: 1, style: st.clone(), button: None }).collect();
                    nc.extend(r.cells.drain(..));
                    r.cells = nc;
                    for im in r.imgs.iter_mut() {
                        im.col += pad;
                    }
                }
            }
        }
        rows
    }

    /// Emit the SGR sequence for a style in the terminal's colour mode.
    fn sgr(&self, out: &mut Vec<u8>, st: &Style, fg_override: Option<Rgb>) {
        let fg = fg_override.unwrap_or(st.fg);
        out.extend_from_slice(b"\x1b[0");
        if st.bold {
            out.extend_from_slice(b";1");
        }
        if st.italic {
            out.extend_from_slice(b";3");
        }
        if st.underline {
            out.extend_from_slice(b";4");
        }
        if st.strike {
            out.extend_from_slice(b";9");
        }
        match self.color {
            ColorMode::True => {
                let _ = write!(out, ";38;2;{};{};{}", fg.0, fg.1, fg.2);
                if let Some(b) = st.bg {
                    let _ = write!(out, ";48;2;{};{};{}", b.0, b.1, b.2);
                }
            }
            ColorMode::C256 => {
                let _ = write!(out, ";38;5;{}", rgb256(fg));
                if let Some(b) = st.bg {
                    let _ = write!(out, ";48;5;{}", rgb256(b));
                }
            }
            ColorMode::C16 => {
                let (c, bright) = rgb16(fg);
                let _ = write!(out, ";{}", if bright { 90 + c } else { 30 + c });
                // the Linux console and fbterm brighten via bold, not 90-97
                if bright {
                    out.extend_from_slice(b";1");
                }
                if let Some(b) = st.bg {
                    let (c, _) = rgb16(b);
                    let _ = write!(out, ";{}", 40 + c);
                }
            }
        }
        out.push(b'm');
    }

    /// Draw a full frame: text rows, images, the prompt / status line.
    fn render(&mut self, con: &mut Console, res: &mut Resources, req: Option<&InputReq>) {
        let (cols, rows, xp, yp) = winsize();
        self.cols = cols;
        self.rows = rows.max(2);
        if !self.fb_cell_fixed && xp > 0 && yp > 0 {
            self.cell = ((xp / cols as u32).max(1), (yp / rows as u32).max(1));
        }
        con.width = cols.saturating_sub(1).max(20);
        let scale = self.cell.1 as f32 / con.line_height.max(1.0);
        let view_h = self.rows - 1;
        self.buttons.clear();
        // layout from the bottom up
        let mut all: Vec<DRow> = Vec::new();
        // (plus some rows above the view: images there can reach into it)
        let need = view_h + self.scroll + IMG_REACH;
        let cur_line = if con.cur.is_empty() {
            None
        } else {
            Some(Line { segs: con.cur.clone(), align: con.cur_align.unwrap_or(con.align), gen: con.gen, temporary: false, cols: self.cols })
        };
        let mut chunks: Vec<Vec<DRow>> = Vec::new();
        let mut count = 0;
        if let Some(l) = &cur_line {
            let r = self.layout_line(l, con.valid_gen, scale);
            count += r.len();
            chunks.push(r);
        }
        for l in con.lines.iter().rev() {
            if count >= need {
                break;
            }
            let r = self.layout_line(l, con.valid_gen, scale);
            count += r.len();
            chunks.push(r);
        }
        for c in chunks.into_iter().rev() {
            all.extend(c);
        }
        let max_scroll = all.len().saturating_sub(view_h);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        let end = all.len() - self.scroll.min(all.len());
        let start = end.saturating_sub(view_h);
        let visible = &all[start..end];

        let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(b"\x1b[?25l");
        self.gfx.begin(&mut out);
        // hover button id (from the previous frame's map; ids are stable)
        let hover = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        self.hover = hover;
        self.btn_map = vec![Vec::new(); self.rows];
        let mut placements = Vec::new();
        // the console mouse (gpm or the raw device) has no pointer of its
        // own on screen: draw one
        let pointer = if self.mice.is_some() { self.mouse } else { None };
        let mut pointer_drawn = false;
        for (sr, row) in visible.iter().enumerate() {
            let _ = write!(out, "\x1b[{};1H", sr + 1);
            let mut col = 0usize;
            let mut last_style: Option<(Style, Option<Rgb>)> = None;
            let mut btn_start: Option<(usize, usize)> = None;
            for cell in &row.cells {
                // clip rows printed when the screen was wider
                if col + cell.w as usize > cols {
                    break;
                }
                if let Some((s, b)) = btn_start {
                    if cell.button != Some(b) {
                        self.btn_map[sr].push((s, col, b));
                        btn_start = None;
                    }
                }
                if let Some(b) = cell.button {
                    if btn_start.is_none() {
                        btn_start = Some((col, b));
                    }
                }
                let ov = match (cell.button, hover) {
                    (Some(b), Some(h)) if b == h => Some(con.focus_fg),
                    _ => None,
                };
                let key = (cell.style.clone(), ov);
                if last_style.as_ref() != Some(&key) {
                    self.sgr(&mut out, &cell.style, ov);
                    last_style = Some(key);
                }
                let mut tmp = [0u8; 4];
                let at_pointer = pointer.is_some_and(|(px, py)| py == sr && px >= col && px < col + cell.w.max(1) as usize);
                if at_pointer {
                    out.extend_from_slice(b"\x1b[7m");
                    pointer_drawn = true;
                }
                if self.ambi == Ambi::Osc66 && cell.w == 2 && UnicodeWidthChar::width(cell.ch) == Some(1) {
                    // a one-column glyph drawn across two cells (see char_cells)
                    let _ = write!(out, "\x1b]66;w=2;{}\x1b\\", cell.ch);
                } else {
                    out.extend_from_slice(cell.ch.encode_utf8(&mut tmp).as_bytes());
                }
                if at_pointer {
                    out.extend_from_slice(b"\x1b[27m");
                }
                col += cell.w as usize;
            }
            if let Some((s, b)) = btn_start {
                self.btn_map[sr].push((s, col, b));
            }
            out.extend_from_slice(b"\x1b[0m\x1b[K");
            // images on this row (skipping any that start past the edge)
        }
        // images: placed by pixel, so a picture can start rows above or
        // below its own line (negative `ypos` stacks portrait layers, each
        // printed on the next line, back onto the first one) and is cropped
        // to the view
        let (ch, view_px) = (self.cell.1.max(1) as i32, (view_h as u32 * self.cell.1.max(1)) as i32);
        for idx in start.saturating_sub(IMG_REACH)..all.len().min(end + IMG_REACH) {
            let sr = idx as i32 - start as i32;
            for im in all[idx].imgs.iter().filter(|im| im.col < cols) {
                let pw = (im.w * scale).round().max(1.0) as u32;
                let ph = (im.h * scale).round().max(1.0) as u32;
                let top = sr * ch + (im.ypos * scale).round() as i32;
                if top + ph as i32 <= 0 || top >= view_px {
                    continue;
                }
                let crop_top = (-top).max(0) as u32;
                let top = top.max(0);
                let crop_h = (ph - crop_top).min((view_px - top) as u32);
                if crop_h == 0 {
                    continue;
                }
                if let Some(img) = res.scaled(&im.name, pw, ph) {
                    placements.push(Placement {
                        key: format!("{}@{}x{}#{}", im.name, pw, ph, res.version(&im.name)),
                        img,
                        col: im.col as i32,
                        row: top / ch,
                        y_off: top % ch,
                        crop_top,
                        crop_h,
                    });
                }
            }
        }
        for sr in visible.len()..view_h {
            let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[K", sr + 1);
        }
        if let (Some((px, py)), false) = (pointer, pointer_drawn) {
            // pointer over blank space
            if py < view_h {
                let _ = write!(out, "\x1b[{};{}H\x1b[0;7m \x1b[0m", py + 1, px + 1);
            }
        }
        // status / input line
        let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[K", self.rows);
        let prompt = match req {
            Some(r) if r.kind == InputKind::Wait => "\x1b[2m▼ (Enter)\x1b[0m".to_string(),
            Some(_) if self.buf.is_empty() && self.notice.is_some() => format!("\x1b[1m>\x1b[0m \x1b[2m({})\x1b[0m", self.notice.as_deref().unwrap_or("")),
            Some(_) => format!("\x1b[1m>\x1b[0m {}", self.buf),
            None => String::new(),
        };
        out.extend_from_slice(prompt.as_bytes());
        if self.scroll > 0 {
            let tag = format!(" [scroll -{}] ", self.scroll);
            let _ = write!(out, "\x1b[{};{}H\x1b[7m{}\x1b[0m", self.rows, cols.saturating_sub(tag.len()), tag);
        }
        self.gfx.draw(&mut out, &placements, self.cell);
        // cursor at end of input
        if req.map(|r| r.kind != InputKind::Wait).unwrap_or(false) {
            let w: usize = self.buf.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", self.rows, 3 + w);
        }
        write_all(&out);
        unsafe {
            libc::tcdrain(1);
        }
        self.gfx.after_flush(&placements, self.cell);
        con.dirty = false;
    }

    /// The button id at a 0-based screen cell, from the last frame.
    fn button_at(&self, x: usize, y: usize) -> Option<usize> {
        let row = self.btn_map.get(y)?;
        row.iter().find(|(s, e, _)| x >= *s && x < *e).map(|(_, _, b)| *b)
    }
}

/// Nearest colour in the xterm 6×6×6 cube.
fn rgb256(c: Rgb) -> u8 {
    let q = |v: u8| -> u8 { if v < 48 { 0 } else if v < 115 { 1 } else { (v - 35) / 40 } };
    16 + 36 * q(c.0) + 6 * q(c.1) + q(c.2)
}

/// Nearest of the 8 ANSI colours, and whether it should be the bright one.
fn rgb16(c: Rgb) -> (u8, bool) {
    let (r, g, b) = (c.0 as u32, c.1 as u32, c.2 as u32);
    let max = r.max(g).max(b);
    if max < 60 {
        return (0, false);
    }
    let t = max / 2;
    let code = ((r > t) as u8) | ((g > t) as u8) << 1 | ((b > t) as u8) << 2;
    let bright = max > 200;
    if code == 7 && max <= 200 {
        return (7, false);
    }
    if code == 7 && max > 200 {
        return (7, true);
    }
    (code, bright)
}

impl Frontend for Tty {
    fn flush(&mut self, con: &mut Console, res: &mut Resources) {
        if con.redraw {
            self.render(con, res, None);
        }
    }

    /// Show the console and wait for input: typed text + Enter, a button
    /// click, any key/click for WAIT, or a timeout. Scrolling keys and the
    /// wheel move through the log meanwhile.
    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        self.buf.clear();
        self.scroll = 0;
        self.render(con, res, Some(req));
        let deadline = req.timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        let (mut last_cols, mut last_rows) = (self.cols, self.rows);
        // a redraw owed for input already handled: done once the burst of
        // input (mouse motion, a held key) has been read, not per event
        let mut owed = false;
        loop {
            if self.hangup {
                return InputResult::Quit;
            }
            if owed && self.queued.is_empty() && self.pending.is_empty() && !self.input_waiting() {
                self.render(con, res, Some(req));
                owed = false;
            }
            let wait = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return InputResult::Timeout;
                    }
                    ((d - now).as_millis() as i32).min(200)
                }
                None => 200,
            };
            let Some(k) = self.next_key(wait) else {
                let (c, r, _, _) = winsize();
                if (c, r) != (last_cols, last_rows) {
                    last_cols = c;
                    last_rows = r;
                    self.render(con, res, Some(req));
                }
                continue;
            };
            let mut redraw = false;
            match k {
                Key::CtrlC => return InputResult::Quit,
                Key::CtrlL => redraw = true,
                Key::PageUp | Key::Up if req.kind == InputKind::Wait || self.buf.is_empty() => {
                    self.scroll += if matches!(k, Key::PageUp) { self.rows / 2 } else { 1 };
                    redraw = true;
                }
                Key::PageDown | Key::Down => {
                    self.scroll = self.scroll.saturating_sub(if matches!(k, Key::PageDown) { self.rows / 2 } else { 1 });
                    redraw = true;
                }
                Key::Home => {
                    self.scroll = usize::MAX / 2;
                    redraw = true;
                }
                Key::End => {
                    self.scroll = 0;
                    redraw = true;
                }
                Key::Mouse { btn, x, y, press } => {
                    let (x, y) = (x.saturating_sub(1) as usize, y.saturating_sub(1) as usize);
                    // ignore Shift / Alt / Ctrl so modified clicks still work
                    match btn & !(4 | 8 | 16) {
                        64 => {
                            self.scroll += 3;
                            redraw = true;
                        }
                        65 => {
                            self.scroll = self.scroll.saturating_sub(3);
                            redraw = true;
                        }
                        b if b & 32 != 0 => {
                            // motion: update hover highlight
                            self.mouse = Some((x, y));
                            // the console pointer is drawn by us: move it, but
                            // only once a burst of motion has been read
                            let more = self.queued.iter().any(|k| matches!(k, Key::Mouse { .. }));
                            if (self.mice.is_some() && !more) || self.button_at(x, y) != self.hover {
                                redraw = true;
                            }
                        }
                        0 if press => {
                            if req.kind == InputKind::Wait {
                                return InputResult::Text(String::new());
                            }
                            if let Some(b) = self.button_at(x, y) {
                                let v = self.buttons[b].clone();
                                return InputResult::Text(v.to_string());
                            }
                        }
                        2 if press && req.kind == InputKind::Wait => return InputResult::Text(String::new()),
                        _ => {}
                    }
                }
                _ if req.kind == InputKind::Wait => {
                    if !matches!(k, Key::Other) {
                        return InputResult::Text(String::new());
                    }
                }
                Key::Enter => {
                    let s = std::mem::take(&mut self.buf);
                    return InputResult::Text(s);
                }
                Key::Backspace => {
                    self.buf.pop();
                    redraw = true;
                }
                Key::Esc => {
                    self.buf.clear();
                    redraw = true;
                }
                Key::Char(c) => {
                    if req.one {
                        return InputResult::Text(c.to_string());
                    }
                    self.buf.push(c);
                    redraw = true;
                }
                _ => {}
            }
            owed |= redraw;
        }
    }

    fn cols(&self) -> usize {
        self.cols
    }

    fn rows(&self) -> usize {
        self.rows
    }

    fn is_fullscreen(&self) -> bool {
        true
    }

    /// Ctrl+C typed (or the terminal gone) while a script runs. Other keys
    /// read meanwhile stay pending for the next input.
    fn poll_interrupt(&mut self) -> bool {
        if self.input_waiting() {
            self.read_bytes(0);
        }
        if let Some(i) = self.pending.iter().position(|&b| b == 0x03) {
            self.pending.drain(..=i);
            return true;
        }
        self.hangup
    }

    /// Restore the terminal (mouse off, main screen, cooked mode).
    fn shutdown(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = Vec::new();
        self.gfx.shutdown(&mut out);
        out.extend_from_slice(b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
        write_all(&out);
        if let Some(n) = &self.notice {
            eprintln!("emuera-tty: {}", n);
        }
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.orig);
        }
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{char_cells, Ambi};

    #[test]
    fn osc66_draws_one_two_cell_glyph() {
        assert_eq!(char_cells('━', Ambi::Osc66, None), (2, 0, ' '));
        assert_eq!(char_cells('林', Ambi::Osc66, None), (2, 0, ' '));
        assert_eq!(char_cells('a', Ambi::Osc66, None), (1, 0, ' '));
    }

    #[test]
    fn emuera_widths_on_a_narrow_terminal() {
        assert_eq!(char_cells('a', Ambi::Pad, None), (1, 0, ' '));
        assert_eq!(char_cells('林', Ambi::Pad, None), (2, 0, ' ')); // already wide
        assert_eq!(char_cells('━', Ambi::Pad, None), (1, 1, '━')); // line continues
        assert_eq!(char_cells('┏', Ambi::Pad, None), (1, 1, '━'));
        assert_eq!(char_cells('┃', Ambi::Pad, None), (1, 1, ' ')); // no rightward stroke
        assert_eq!(char_cells('■', Ambi::Pad, None), (1, 1, '■'));
        assert_eq!(char_cells('※', Ambi::Pad, None), (1, 1, ' '));
        assert_eq!(char_cells('―', Ambi::Pad, None), (1, 1, '―')); // horizontal bar continues
        assert_eq!(char_cells('ｱ', Ambi::Pad, None), (1, 0, ' ')); // half-width kana: 1 in Emuera too
    }

    #[test]
    fn measured_width_wins() {
        // a terminal seen drawing ━ two cells wide needs no filler
        assert_eq!(char_cells('━', Ambi::Pad, Some(2)), (2, 0, '━'));
        // one that draws ￤ narrow (tables say 2) gets a filler instead
        assert_eq!(char_cells('￤', Ambi::Pad, Some(1)), (1, 1, ' '));
    }

    #[test]
    fn linux_console_draws_everything_in_one_cell() {
        // kanji, full-width space and letters: one cell plus a filler
        assert_eq!(char_cells('漢', Ambi::Narrow, None), (1, 1, ' '));
        assert_eq!(char_cells('　', Ambi::Narrow, None), (1, 1, ' '));
        assert_eq!(char_cells('Ａ', Ambi::Narrow, None), (1, 1, ' '));
        assert_eq!(char_cells('━', Ambi::Narrow, None), (1, 1, '━'));
        assert_eq!(char_cells('a', Ambi::Narrow, None), (1, 0, ' '));
    }

    #[test]
    fn no_filler_when_terminal_draws_ambiguous_wide() {
        assert_eq!(char_cells('━', Ambi::Wide, None), (2, 0, '━'));
        assert_eq!(char_cells('■', Ambi::Wide, None), (2, 0, '■'));
    }
}
