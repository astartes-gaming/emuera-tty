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
    /// `<div>` content anchored on this row, painted over the rows at its
    /// offset once the screen is laid out
    divs: Vec<DivAt>,
}

/// A laid-out `<div>`: its rows, drawn from column `col`, `dy` rows below
/// (negative: above) the row it is anchored on.
struct DivAt {
    col: usize,
    dy: i32,
    rows: Vec<DRow>,
}

/// Paint `cells` over `row` from screen column `col`: covered cells (and
/// wide characters cut in half) give way, the row is padded if short.
fn paint_cells(row: &mut DRow, col: usize, cells: Vec<Cell>) {
    if cells.is_empty() {
        return;
    }
    let width: usize = cells.iter().map(|c| c.w.max(1) as usize).sum();
    let end = col + width;
    let blank = |st: &Style| Cell { ch: ' ', w: 1, style: st.clone(), button: None };
    let plain = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut c = 0usize;
    for cell in std::mem::take(&mut row.cells) {
        let w = cell.w.max(1) as usize;
        if c + w <= col {
            before.push(cell);
        } else if c >= end {
            after.push(cell);
        } else {
            // partly covered: keep the uncovered columns blank
            for k in c..c + w {
                if k < col {
                    before.push(blank(&cell.style));
                } else if k >= end {
                    after.push(blank(&cell.style));
                }
            }
        }
        c += w;
    }
    while c < col {
        before.push(blank(&plain));
        c += 1;
    }
    before.extend(cells);
    before.extend(after);
    row.cells = before;
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
    Left,
    Right,
    /// F1 or Ctrl-O: the settings overlay
    Settings,
    Mouse { btn: u16, x: u16, y: u16, press: bool },
    Other,
}

/// What the settings overlay asks the input loop to do next.
enum OverlayExit {
    Close,
    SwitchGame,
    Quit,
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
    /// the last frame, for moving the console pointer without a full
    /// redraw (which makes images flicker): per screen row (col, cell,
    /// hover colour), the images, the pointer cell and the input cursor
    frame: Vec<Vec<(usize, Cell, Option<Rgb>)>>,
    frame_imgs: Vec<Placement>,
    drawn_pointer: Option<(usize, usize)>,
    frame_cursor: Option<(usize, usize)>,
    /// the window background for this frame (None: the terminal's own)
    screen_bg: Option<Rgb>,
    /// minimum contrast for coloured text (1: off), see `console::readable`
    pub contrast: f32,
    /// lines per mouse-wheel step
    pub wheel: usize,
    /// frames drawn without images (under the settings overlay)
    no_images: bool,
    /// the colour mode detected at startup, for the "auto" setting
    auto_color: ColorMode,
    /// the `--gfx` preference and $TERM, to set the image backend up again
    gfx_pref: String,
    term: String,
    /// fingerprint of the last frame sent (an identical one is skipped)
    last_frame: u64,
    force_frame: bool,
    /// CBG pictures scaled to the terminal, by (fingerprint, size)
    cbg_scaled: std::collections::HashMap<(u64, u32, u32), std::rc::Rc<image::RgbaImage>>,
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
        let forced = match std::env::var("EMUERA_COLORS").as_deref() {
            Ok("truecolor") | Ok("24bit") => Some(ColorMode::True),
            Ok("256") => Some(ColorMode::C256),
            Ok("16") => Some(ColorMode::C16),
            _ => None,
        };
        let detected = if term == "linux" || term.starts_with("fbterm") {
            ColorMode::C16
        } else if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("kitty") || term.contains("foot") || term.contains("direct") {
            ColorMode::True
        } else if term.contains("256") {
            ColorMode::C256
        } else {
            ColorMode::True
        };
        let color = forced.unwrap_or(detected);
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
            frame: Vec::new(),
            frame_imgs: Vec::new(),
            drawn_pointer: None,
            frame_cursor: None,
            screen_bg: None,
            contrast: 3.0,
            wheel: 3,
            no_images: false,
            auto_color: detected,
            gfx_pref: gfx_pref.to_string(),
            term: term.clone(),
            last_frame: 0,
            force_frame: false,
            cbg_scaled: std::collections::HashMap::new(),
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
                                    // F1: xterm-style, and the Linux console's ESC [ [ A
                                    (p, b'~') if p == b"11" => Key::Settings,
                                    (p, b'A') if p == b"[" => Key::Settings,
                                    (_, b'A') => Key::Up,
                                    (_, b'B') => Key::Down,
                                    (_, b'C') => Key::Right,
                                    (_, b'D') => Key::Left,
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
                    (match b[2] {
                        b'P' => Key::Settings,
                        b'A' => Key::Up,
                        b'B' => Key::Down,
                        b'C' => Key::Right,
                        b'D' => Key::Left,
                        _ => Key::Other,
                    }, 3)
                } else {
                    (Key::Esc, 1)
                }
            }
            b'\r' | b'\n' => (Key::Enter, 1),
            0x7f | 0x08 => (Key::Backspace, 1),
            0x03 => (Key::CtrlC, 1),
            0x0c => (Key::CtrlL, 1),
            0x0f => (Key::Settings, 1),
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
        let screen = self.cols.max(1);
        self.layout_line_in(line, cur_gen, scale, screen)
    }

    /// Lay out a line; `align_w` is the width center / right alignment
    /// works in (the screen, or a `<div>`).
    fn layout_line_in(&mut self, line: &Line, cur_gen: u64, scale: f32, align_w: usize) -> Vec<DRow> {
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
                Seg::Div { dx, dy, cols, rows: max_rows, lines, .. } => {
                    let mut inner = Vec::new();
                    for l in lines {
                        inner.extend(self.layout_line_in(l, cur_gen, scale, *cols));
                    }
                    if let Some(m) = max_rows {
                        inner.truncate(*m);
                    }
                    let at = (col as i64 + *dx as i64).max(0) as usize;
                    rows.last_mut().unwrap().divs.push(DivAt { col: at, dy: *dy, rows: inner });
                }
            }
        }
        // alignment, within the screen (or div) as it is now
        if line.align != Align::Left {
            let screen = align_w.max(1);
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
                    for d in r.divs.iter_mut() {
                        d.col += pad;
                    }
                }
            }
        }
        rows
    }

    /// Emit the SGR sequence for a style in the terminal's colour mode.
    /// The background is the text's own, else the window's (SETBGCOLOR);
    /// the foreground is made readable against it (see `contrast`).
    fn sgr(&self, out: &mut Vec<u8>, st: &Style, fg_override: Option<Rgb>) {
        let bg = st.bg.or(self.screen_bg);
        let fg = crate::console::readable(fg_override.unwrap_or(st.fg), bg.unwrap_or(Rgb(0, 0, 0)), self.contrast);
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
                if let Some(b) = bg {
                    let _ = write!(out, ";48;2;{};{};{}", b.0, b.1, b.2);
                }
            }
            ColorMode::C256 => {
                let _ = write!(out, ";38;5;{}", rgb256(fg));
                if let Some(b) = bg {
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
                if let Some(b) = bg {
                    let _ = write!(out, ";{}", 40 + rgb16_bg(b));
                }
            }
        }
        out.push(b'm');
    }

    /// Reset attributes to the window background and clear to the end of
    /// the line with it.
    fn clear_eol(&self, out: &mut Vec<u8>) {
        match self.screen_bg {
            None => out.extend_from_slice(b"\x1b[0m\x1b[K"),
            Some(b) => {
                let _ = match self.color {
                    ColorMode::True => write!(out, "\x1b[0;48;2;{};{};{}m\x1b[K", b.0, b.1, b.2),
                    ColorMode::C256 => write!(out, "\x1b[0;48;5;{}m\x1b[K", rgb256(b)),
                    ColorMode::C16 => write!(out, "\x1b[0;{}m\x1b[K", 40 + rgb16_bg(b)),
                };
            }
        }
    }

    /// Draw a full frame: text rows, images, the prompt / status line.
    fn render(&mut self, con: &mut Console, res: &mut Resources, req: Option<&InputReq>) {
        // the window background, unless it is the game's default (then the
        // terminal's own background shows)
        self.screen_bg = (con.screen_bg != con.default_bg).then_some(con.screen_bg);
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
        // <div> content: painted over the rows it reaches (Emuera draws a
        // div on top of whatever lines are there)
        for idx in 0..all.len() {
            if all[idx].divs.is_empty() {
                continue;
            }
            for d in std::mem::take(&mut all[idx].divs) {
                for (k, r) in d.rows.into_iter().enumerate() {
                    let t = idx as i64 + d.dy as i64 + k as i64;
                    if t < 0 || t as usize >= all.len() {
                        continue;
                    }
                    let t = t as usize;
                    for mut im in r.imgs {
                        im.col += d.col;
                        all[t].imgs.push(im);
                    }
                    paint_cells(&mut all[t], d.col, r.cells);
                }
            }
        }
        // a picture menu (CBG layers) may draw anywhere on the screen:
        // give it rows to draw text into below short content
        if !res.cbg.is_empty() && self.scroll == 0 {
            while all.len() < view_h {
                all.push(DRow::default());
            }
        }
        let max_scroll = all.len().saturating_sub(view_h);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        let end = all.len() - self.scroll.min(all.len());
        let start = end.saturating_sub(view_h);
        let cbg_places = if res.cbg.is_empty() { Vec::new() } else { self.cbg_layers(con, res, &mut all[start..end]) };
        let visible = &all[start..end];

        let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(b"\x1b[?25l");
        self.gfx.begin(&mut out);
        // hover button id (from the previous frame's map; ids are stable)
        let hover = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        self.hover = hover;
        self.btn_map = vec![Vec::new(); self.rows];
        self.frame = vec![Vec::new(); self.rows];
        let mut placements = cbg_places;
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
                self.frame[sr].push((col, cell.clone(), ov));
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
            self.clear_eol(&mut out);
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
                if self.no_images {
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
            let _ = write!(out, "\x1b[{};1H", sr + 1);
            self.clear_eol(&mut out);
        }
        if let (Some((px, py)), false) = (pointer, pointer_drawn) {
            // pointer over blank space
            if py < view_h {
                let _ = write!(out, "\x1b[{};{}H\x1b[0;7m \x1b[0m", py + 1, px + 1);
            }
        }
        // status / input line
        let _ = write!(out, "\x1b[{};1H", self.rows);
        self.clear_eol(&mut out);
        let prompt = match req {
            Some(r) if r.kind == InputKind::Wait => "\x1b[2m▼ (Enter)\x1b[0m".to_string(),
            Some(_) if self.buf.is_empty() && self.notice.is_some() => format!("\x1b[1m>\x1b[0m \x1b[2m({})\x1b[0m", self.notice.as_deref().unwrap_or("")),
            Some(_) => format!("\x1b[1m>\x1b[0m {}", self.buf),
            None => String::new(),
        };
        out.extend_from_slice(prompt.as_bytes());
        if self.scroll == 0 && cols > 40 && req.is_some() && !self.no_images {
            let hint = "F1 settings";
            let used = 2 + self.buf.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>() + 20;
            if used + hint.len() < cols {
                let _ = write!(out, "\x1b[{};{}H\x1b[0;2m{}\x1b[0m", self.rows, cols - hint.len(), hint);
            }
        }
        if self.scroll > 0 {
            let tag = format!(" [scroll -{}] ", self.scroll);
            let _ = write!(out, "\x1b[{};{}H\x1b[7m{}\x1b[0m", self.rows, cols.saturating_sub(tag.len()), tag);
        }
        self.gfx.draw(&mut out, &placements, self.cell);
        self.drawn_pointer = pointer.filter(|p| p.1 < view_h);
        self.frame_cursor = None;
        // cursor at end of input
        if req.map(|r| r.kind != InputKind::Wait).unwrap_or(false) {
            let w: usize = self.buf.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", self.rows, 3 + w);
            self.frame_cursor = Some((self.rows, 3 + w));
        }
        // a frame exactly like the last one (a script redrawing a picture
        // menu every few milliseconds) isn't sent again
        let fp = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            out.hash(&mut h);
            for p in &placements {
                p.key.hash(&mut h);
            }
            h.finish()
        };
        if fp == self.last_frame && !self.force_frame {
            self.frame_imgs = placements;
            con.dirty = false;
            return;
        }
        if std::env::var_os("EMUERA_DEBUG_FRAMES").is_some() {
            eprintln!("frame {:x} len {} keys {:?}", fp, out.len(), placements.iter().map(|p| p.key.clone()).collect::<Vec<_>>());
        }
        self.last_frame = fp;
        self.force_frame = false;
        write_all(&out);
        unsafe {
            libc::tcdrain(1);
        }
        self.gfx.after_flush(&placements, self.cell);
        self.frame_imgs = placements;
        con.dirty = false;
    }

    /// The client background layers (CBG*): as images where the terminal
    /// shows them, else the text drawn into them (GDRAWTEXT) painted into
    /// the screen `rows`, so a menu drawn as a picture still works.
    /// Returns their image placements, back to front.
    fn cbg_layers(&mut self, con: &Console, res: &mut Resources, rows: &mut [DRow]) -> Vec<Placement> {
        use crate::resources::CbgSrc;
        let cw = (con.font_size / 2.0).max(1.0);
        let lh = con.line_height.max(1.0);
        let client_h = self.rows as f32 * lh;
        // the button under the mouse (button map colour)
        let hovered = self.mouse.and_then(|(c, r)| {
            let x = (c as f32 * cw + cw / 2.0) as i64;
            let y = (r as f32 * lh + lh / 2.0 - client_h) as i64;
            res.cbg_button_at(x, y)
        });
        let images = self.gfx.name() != "none" && !self.no_images;
        let (sx, sy) = (self.cell.0 as f32 / cw, self.cell.1 as f32 / lh);
        let screen_px = (self.rows as u32 * self.cell.1.max(1)) as i32;
        let mut out = Vec::new();
        for l in res.cbg.clone() {
            let src = match &l.button {
                Some((v, hover)) if hovered == Some(*v) => match hover {
                    Some(h) => h.clone(),
                    None => continue,
                },
                _ => l.src.clone(),
            };
            if let CbgSrc::Sprite(n) = &src {
                if n.is_empty() {
                    continue;
                }
            }
            if !images {
                // (only the size is needed: no pixel fingerprint)
                let CbgSrc::G(gid) = src else { continue };
                let Some(g) = res.graphics.get(&gid) else { continue };
                let top_e = client_h + l.y as f32 - g.img.height() as f32;
                // text fallback: what GDRAWTEXT wrote into the buffer
                for t in &g.texts {
                    // text drawn larger than the console font takes more
                    // pixels per character than a cell: keep its middle
                    // where it was (menus are centred), not its left edge
                    let text = t.text.trim();
                    let lead = t.text.len() - t.text.trim_start().len();
                    let half = t.size as f32 / 2.0; // px per half-width char
                    let w_px = crate::loader::sjis_len(text) as f32 * half;
                    let mid = l.x as f32 + t.x as f32 + lead as f32 * half + w_px / 2.0;
                    let st = Style { fg: Rgb(t.color[0], t.color[1], t.color[2]), bg: None, bold: t.size as f32 > con.font_size, italic: false, underline: false, strike: false };
                    let cells = self.text_cells(text, &st);
                    let width: usize = cells.iter().map(|c| c.w as usize).sum();
                    let col = (mid / cw - width as f32 / 2.0).round().max(0.0);
                    let row = ((top_e + t.y as f32 + t.size as f32 / 2.0) / lh).floor();
                    if row < 0.0 || row as usize >= rows.len() || text.is_empty() {
                        continue;
                    }
                    paint_cells(&mut rows[row as usize], col as usize, cells);
                }
                continue;
            }
            let Some((img, fp)) = res.cbg_pixels(&src) else { continue };
            let (w, h) = (img.width() as f32, img.height() as f32);
            let top_e = client_h + l.y as f32 - h;
            let (pw, ph) = ((w * sx).round().max(1.0) as u32, (h * sy).round().max(1.0) as u32);
            let (left, top) = ((l.x as f32 * sx).round() as i32, (top_e * sy).round() as i32);
            if left < 0 || top + ph as i32 <= 0 || top >= screen_px {
                continue;
            }
            let crop_top = (-top).max(0) as u32;
            let top_c = top.max(0);
            let crop_h = (ph - crop_top.min(ph)).min((screen_px - top_c) as u32);
            if crop_h == 0 {
                continue;
            }
            let key = (fp, pw, ph);
            let scaled = match self.cbg_scaled.get(&key) {
                Some(i) => i.clone(),
                None => {
                    if self.cbg_scaled.len() > 8 {
                        self.cbg_scaled.clear();
                    }
                    let i = if (pw, ph) == img.dimensions() {
                        img.clone()
                    } else {
                        std::rc::Rc::new(image::imageops::resize(&*img, pw, ph, image::imageops::FilterType::Triangle))
                    };
                    self.cbg_scaled.insert(key, i.clone());
                    i
                }
            };
            let ch = self.cell.1.max(1) as i32;
            out.push(Placement {
                key: format!("cbg{:x}@{}x{}", fp, pw, ph),
                img: scaled,
                col: left / self.cell.0.max(1) as i32,
                row: top_c / ch,
                y_off: top_c % ch,
                crop_top,
                crop_h,
            });
        }
        out
    }

    /// Cells for a run of text in one style (for text painted over rows).
    fn text_cells(&self, text: &str, st: &Style) -> Vec<Cell> {
        let mut v = Vec::new();
        for ch in text.chars() {
            let (tw, extra, fill) = char_cells(ch, self.ambi, self.widths.get(&ch).copied());
            if tw == 0 {
                continue;
            }
            v.push(Cell { ch, w: tw as u8, style: st.clone(), button: None });
            for _ in 0..extra {
                v.push(Cell { ch: fill, w: 1, style: st.clone(), button: None });
            }
        }
        v
    }

    /// After a mouse move, repaint only what changed from the last frame:
    /// the old and new console pointer cells and the cells of the button
    /// that lost or gained the hover highlight (re-blitting framebuffer
    /// images under them). A full redraw makes the images flicker.
    fn repaint_hover(&mut self, focus: Rgb) {
        let new_ptr = if self.mice.is_some() { self.mouse.filter(|p| p.1 < self.rows.saturating_sub(1)) } else { None };
        let new_hover = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        let old_hover = self.hover;
        if new_ptr == self.drawn_pointer && new_hover == old_hover {
            return;
        }
        let hits = |pos: Option<(usize, usize)>, r: usize, c: usize, w: usize| pos.is_some_and(|(px, py)| py == r && px >= c && px < c + w);
        let mut out: Vec<u8> = Vec::with_capacity(1024);
        out.extend_from_slice(b"\x1b[?25l");
        let mut touched = Vec::new();
        let mut covered = [false, false];
        for r in 0..self.frame.len() {
            for i in 0..self.frame[r].len() {
                let (c, ref cell, _) = self.frame[r][i];
                let w = cell.w.max(1) as usize;
                let on_old = hits(self.drawn_pointer, r, c, w);
                let on_new = hits(new_ptr, r, c, w);
                covered[0] |= on_old;
                covered[1] |= on_new;
                let btn_changed = new_hover != old_hover && cell.button.is_some() && (cell.button == old_hover || cell.button == new_hover);
                if !(on_old || on_new || btn_changed) {
                    continue;
                }
                let ov = match (cell.button, new_hover) {
                    (Some(b), Some(h)) if b == h => Some(focus),
                    _ => None,
                };
                let _ = write!(out, "\x1b[{};{}H\x1b[0m", r + 1, c + 1);
                self.sgr(&mut out, &cell.style, ov);
                if on_new {
                    out.extend_from_slice(b"\x1b[7m");
                }
                let mut tmp = [0u8; 4];
                if self.ambi == Ambi::Osc66 && cell.w == 2 && UnicodeWidthChar::width(cell.ch) == Some(1) {
                    let _ = write!(out, "\x1b]66;w=2;{}\x1b\\", cell.ch);
                } else {
                    out.extend_from_slice(cell.ch.encode_utf8(&mut tmp).as_bytes());
                }
                out.extend_from_slice(b"\x1b[0m");
                touched.push((c, r, w));
                self.frame[r][i].2 = ov;
            }
        }
        // pointer over blank space past the end of a row
        for (k, (pos, inverse)) in [(self.drawn_pointer, false), (new_ptr, true)].into_iter().enumerate() {
            if let (Some((px, py)), false) = (pos, covered[k]) {
                let _ = write!(out, "\x1b[{};{}H", py + 1, px + 1);
                // a blank in the window background, inverted for the pointer
                let blank = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
                self.sgr(&mut out, &blank, None);
                out.extend_from_slice(if inverse { b"\x1b[7m \x1b[0m" } else { b" \x1b[0m" });
                touched.push((px, py, 1));
            }
        }
        if let Some((r, c)) = self.frame_cursor {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", r, c);
        }
        write_all(&out);
        unsafe {
            libc::tcdrain(1);
        }
        self.drawn_pointer = new_ptr;
        self.hover = new_hover;
        // images the repainted cells cut into (only the framebuffer draws
        // over text: terminal images sit above it)
        let (cw, ch) = (self.cell.0.max(1) as i32, self.cell.1.max(1) as i32);
        let hit: Vec<Placement> = self
            .frame_imgs
            .iter()
            .filter(|p| {
                let (x0, y0) = (p.col * cw, p.row * ch + p.y_off);
                let (x1, y1) = (x0 + p.img.width() as i32, y0 + p.crop_h as i32);
                touched.iter().any(|&(c, r, w)| {
                    let (cx0, cy0) = (c as i32 * cw, r as i32 * ch);
                    cx0 < x1 && cx0 + w as i32 * cw > x0 && cy0 < y1 && cy0 + ch > y0
                })
            })
            .cloned()
            .collect();
        if !hit.is_empty() {
            self.gfx.after_flush(&hit, self.cell);
        }
    }

    /// The settings overlay (F1 / Ctrl-O): a box over the game listing
    /// `settings::ITEMS`, changed with ← / → / Enter and saved at once;
    /// most apply to the running game. Images are hidden while it is up
    /// (kitty would draw them over it).
    fn settings_overlay(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> OverlayExit {
        use crate::settings::{Settings, ITEMS};
        let mut st = Settings::load();
        let extra = ["Switch game…  (unsaved progress is lost)", "Quit emuera-tty", "Close  (Esc)"];
        let n = ITEMS.len() + extra.len();
        let mut sel = 0usize;
        let mut note = String::new();
        let exit = loop {
            self.no_images = true;
            self.render(con, res, Some(req));
            let geo = self.draw_overlay(&st, &extra, sel, &note);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    break OverlayExit::Quit;
                }
                continue;
            };
            note.clear();
            // a click picks a row (and acts on it)
            let mut act = 0i32; // 1 next / Enter, -1 previous
            match k {
                Key::Up | Key::Char('k') => sel = (sel + n - 1) % n,
                Key::Down | Key::Char('j') => sel = (sel + 1) % n,
                Key::Esc | Key::Settings | Key::Char('q') => break OverlayExit::Close,
                Key::CtrlC => break OverlayExit::Quit,
                Key::Enter | Key::Right | Key::Char(' ') | Key::Char('l') => act = 1,
                Key::Left | Key::Char('h') | Key::Backspace => act = -1,
                Key::Mouse { btn, y, press: true, .. } => match btn & !(4 | 8 | 16) {
                    64 => sel = (sel + n - 1) % n,
                    65 => sel = (sel + 1) % n,
                    0 => {
                        let row = y as usize;
                        if row > geo.0 && row <= geo.0 + n {
                            sel = row - geo.0 - 1;
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
            if sel < ITEMS.len() {
                let it = &ITEMS[sel];
                let v = st.cycle(it.key, act).to_string();
                st.save();
                if it.live {
                    self.apply_setting(it.key, &v, &st);
                } else {
                    note = format!("{} takes effect the next time emuera-tty starts", it.label);
                }
            } else if act == 1 {
                match sel - ITEMS.len() {
                    0 => break OverlayExit::SwitchGame,
                    1 => break OverlayExit::Quit,
                    _ => break OverlayExit::Close,
                }
            }
        };
        self.no_images = false;
        exit
    }

    /// Apply a changed setting to the running terminal.
    fn apply_setting(&mut self, key: &str, v: &str, st: &crate::settings::Settings) {
        match key {
            "contrast" => self.contrast = st.contrast_ratio(),
            "wheel" => self.wheel = v.parse().unwrap_or(3),
            "colors" => {
                self.color = match v {
                    "truecolor" => ColorMode::True,
                    "256" => ColorMode::C256,
                    "16" => ColorMode::C16,
                    _ => self.auto_color,
                }
            }
            "wide" => {
                if v == "auto" {
                    std::env::remove_var("EMUERA_AMBIGUOUS");
                } else {
                    std::env::set_var("EMUERA_AMBIGUOUS", v);
                }
                self.ambi = self.detect_ambiguous();
            }
            "images" => {
                let mut out = Vec::new();
                self.gfx.shutdown(&mut out);
                write_all(&out);
                self.gfx = Box::new(gfx::NoGfx);
                self.fb_cell_fixed = false;
                let pref = if v == "auto" && self.gfx_pref != "auto" { self.gfx_pref.clone() } else { v.to_string() };
                let term = self.term.clone();
                self.setup_gfx(&pref, &term);
            }
            _ => {}
        }
    }

    /// Draw the overlay box; returns its top row (1-based, the title row)
    /// for mapping clicks.
    fn draw_overlay(&self, st: &crate::settings::Settings, extra: &[&str], sel: usize, note: &str) -> (usize,) {
        use crate::settings::ITEMS;
        let n = ITEMS.len() + extra.len();
        let w = self.cols.saturating_sub(4).clamp(30, 72);
        let h = n + 4; // title, rows, help, borders
        let left = (self.cols.saturating_sub(w)) / 2 + 1;
        let top = (self.rows.saturating_sub(h)) / 2 + 1;
        let mut out: Vec<u8> = Vec::new();
        let inner = w - 2;
        // fixed colours where the terminal has them: palette colours vary
        // by theme (Konsole's blue is bright, and bright cyan on it vanishes)
        let rgb = |fg: (u8, u8, u8), bg: (u8, u8, u8), bold: bool| -> String {
            let b = if bold { "1;" } else { "" };
            match self.color {
                ColorMode::True => format!("\x1b[0;{}38;2;{};{};{};48;2;{};{};{}m", b, fg.0, fg.1, fg.2, bg.0, bg.1, bg.2),
                ColorMode::C256 => format!(
                    "\x1b[0;{}38;5;{};48;5;{}m",
                    b,
                    rgb256(Rgb(fg.0, fg.1, fg.2)),
                    rgb256(Rgb(bg.0, bg.1, bg.2))
                ),
                // 16 colours: white / yellow on black, black on white
                ColorMode::C16 => {
                    let f = if fg == (0, 0, 0) { 30 } else if fg.2 < 100 { 93 } else { 97 };
                    let k = if bg == (0, 0, 0) || bg.0 < 128 { 40 } else { 47 };
                    format!("\x1b[0;{}{};{}m", b, f, k)
                }
            }
        };
        let box_bg = (22, 32, 58);
        let frame = rgb((150, 170, 210), box_bg, false);
        let title = rgb((255, 255, 255), box_bg, true);
        let normal = rgb((235, 235, 235), box_bg, false);
        let action = rgb((140, 220, 255), box_bg, false);
        let hilite = rgb((0, 0, 0), (230, 230, 230), false);
        let helpc = rgb((255, 220, 90), box_bg, false);
        let line = |out: &mut Vec<u8>, row: usize, text: &str, style: &str| {
            let mut t: String = String::new();
            let mut used = 0;
            for c in text.chars() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > inner {
                    break;
                }
                t.push(c);
                used += cw;
            }
            let _ = write!(out, "\x1b[{};{}H{}│{}{}{}{}│", row, left, frame, style, t, " ".repeat(inner - used), frame);
        };
        let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
        line(&mut out, top + 1, " emuera-tty settings  (F1 / Ctrl-O)", &title);
        for (i, it) in ITEMS.iter().enumerate() {
            let later = if it.live { "" } else { "  (next start)" };
            let text = format!("  {:<18} ‹ {} ›{}", it.label, st.get(it.key), later);
            line(&mut out, top + 2 + i, &text, if i == sel { &hilite } else { &normal });
        }
        for (k, e) in extra.iter().enumerate() {
            let i = ITEMS.len() + k;
            line(&mut out, top + 2 + i, &format!("  {}", e), if i == sel { &hilite } else { &action });
        }
        let help = if !note.is_empty() {
            note.to_string()
        } else if sel < ITEMS.len() {
            ITEMS[sel].help.to_string()
        } else {
            "↑↓ choose  ←→ / Enter change  Esc close".to_string()
        };
        line(&mut out, top + 2 + n, &format!(" {}", help), &helpc);
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 3 + n, left, frame, "─".repeat(inner));
        write_all(&out);
        (top + 1,)
    }

    /// The button id at a 0-based screen cell, from the last frame.
    fn button_at(&self, x: usize, y: usize) -> Option<usize> {
        let row = self.btn_map.get(y)?;
        row.iter().find(|(s, e, _)| x >= *s && x < *e).map(|(_, _, b)| *b)
    }
}

/// Nearest xterm-256 colour: the 6×6×6 cube or, for greys, the 24-step
/// grey ramp (the cube has only black below 95, so dark map tiles and
/// shading would all turn black).
fn rgb256(c: Rgb) -> u8 {
    let q = |v: u8| -> u8 { if v < 48 { 0 } else if v < 115 { 1 } else { (v - 35) / 40 } };
    let lv = |k: u8| -> i32 { if k == 0 { 0 } else { 55 + 40 * k as i32 } };
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    let cube = (q(c.0), q(c.1), q(c.2));
    let d_cube = (lv(cube.0) - r).pow(2) + (lv(cube.1) - g).pow(2) + (lv(cube.2) - b).pow(2);
    // grey ramp 232..=255 is 8, 18, ..., 238
    let avg = (r + g + b) / 3;
    let gi = ((avg - 3) / 10).clamp(0, 23);
    let gv = 8 + 10 * gi;
    let d_grey = (gv - r).pow(2) + (gv - g).pow(2) + (gv - b).pow(2);
    if d_grey < d_cube && avg >= 4 {
        232 + gi as u8
    } else {
        16 + 36 * cube.0 + 6 * cube.1 + cube.2
    }
}

/// One of the 8 ANSI colours for an RGB colour, and whether it should be
/// the bright one. Hues are kept even when dark (Emuera games draw dim
/// teal walls or brown floors on black, which nearest-distance would turn
/// into black); greys step through black, dark grey, light grey, white.
pub(crate) fn rgb16(c: Rgb) -> (u8, bool) {
    let (r, g, b) = (c.0 as u32, c.1 as u32, c.2 as u32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 24 {
        return (0, false);
    }
    // greyish: little colour compared with the brightness
    if (max - min) * 4 < max {
        return match max {
            0..=39 => (0, false),
            40..=127 => (0, true),
            128..=210 => (7, false),
            _ => (7, true),
        };
    }
    // which channels stand out within the colour's own range
    let t = min + (max - min) / 3;
    let code = ((r > t) as u8) | ((g > t) as u8) << 1 | ((b > t) as u8) << 2;
    (code, max > 200)
}

/// An ANSI background colour for a window background: only the strongest
/// channels count, so a dark navy stays blue (not cyan) and very dark
/// colours stay black.
fn rgb16_bg(c: Rgb) -> u8 {
    let (r, g, b) = (c.0 as u32, c.1 as u32, c.2 as u32);
    let max = r.max(g).max(b);
    if max < 40 {
        return 0;
    }
    let t = max * 2 / 3;
    ((r > t) as u8) | ((g > t) as u8) << 1 | ((b > t) as u8) << 2
}

#[cfg(test)]
mod colour_tests {
    use super::*;

    fn row(text: &str) -> DRow {
        let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
        DRow { cells: text.chars().map(|ch| Cell { ch, w: if ch.is_ascii() { 1 } else { 2 }, style: st.clone(), button: None }).collect(), ..Default::default() }
    }

    fn text(r: &DRow) -> String {
        r.cells.iter().map(|c| c.ch).collect()
    }

    #[test]
    fn div_cells_paint_over_rows() {
        let mut r = row("abcdef");
        paint_cells(&mut r, 2, row("XY").cells);
        assert_eq!(text(&r), "abXYef");
        // past the end: padded
        let mut r = row("ab");
        paint_cells(&mut r, 4, row("Z").cells);
        assert_eq!(text(&r), "ab  Z");
        // a wide character cut in half leaves a blank
        let mut r = row("a漢b");
        paint_cells(&mut r, 2, row("Q").cells);
        assert_eq!(text(&r), "a Qb");
    }

    #[test]
    fn dark_tints_stay_visible() {
        // dim teal / brown map tiles keep their hue
        assert_eq!(rgb16(Rgb(0x01, 0x3d, 0x3d)), (6, false));
        assert_eq!(rgb16(Rgb(0x47, 0x29, 0x1f)), (1, false));
        assert_eq!(rgb16(Rgb(0xc9, 0xa1, 0x79)), (3, true));
        assert_eq!(rgb16(Rgb(0xff, 0x66, 0xcc)), (5, true));
        assert_eq!(rgb16(Rgb(0x50, 0xa0, 0xf0)), (6, true));
        // the grey vignette fades out through dark grey to black
        assert_eq!(rgb16(Rgb(0x79, 0x79, 0x79)), (0, true));
        assert_eq!(rgb16(Rgb(0x29, 0x29, 0x29)), (0, true));
        assert_eq!(rgb16(Rgb(0x0b, 0x0b, 0x0b)), (0, false));
        assert_eq!(rgb16(Rgb(0xc0, 0xc0, 0xc0)), (7, false));
        assert_eq!(rgb16(Rgb(0xff, 0xff, 0xff)), (7, true));
        assert_eq!(rgb16(Rgb(0xff, 0xff, 0x00)), (3, true));
        // 256 colours: dark greys use the grey ramp, not cube black
        assert_eq!(rgb256(Rgb(0x15, 0x15, 0x15)), 233);
        assert_eq!(rgb256(Rgb(0xff, 0x00, 0x00)), 196);
        assert_eq!(rgb256(Rgb(0, 0, 0)), 16);
    }
}

impl Frontend for Tty {
    fn flush(&mut self, con: &mut Console, res: &mut Resources) {
        if con.redraw {
            self.render(con, res, None);
        }
    }

    fn refresh(&mut self, con: &mut Console, res: &mut Resources) {
        self.render(con, res, None);
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
        // only the console pointer moved: repaint just its cells
        let mut pointer_owed = false;
        loop {
            if self.hangup {
                return InputResult::Quit;
            }
            if (owed || pointer_owed) && self.queued.is_empty() && self.pending.is_empty() && !self.input_waiting() {
                if owed {
                    self.render(con, res, Some(req));
                } else {
                    self.repaint_hover(con.focus_fg);
                }
                owed = false;
                pointer_owed = false;
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
                Key::CtrlL => {
                    self.force_frame = true;
                    redraw = true;
                }
                Key::Settings => match self.settings_overlay(con, res, req) {
                    OverlayExit::Close => redraw = true,
                    OverlayExit::Quit => return InputResult::Quit,
                    OverlayExit::SwitchGame => {
                        // start over on the launcher (this game's unsaved
                        // progress goes, as the overlay warns)
                        self.shutdown();
                        if let Ok(exe) = std::env::current_exe() {
                            use std::os::unix::process::CommandExt;
                            let e = std::process::Command::new(exe).arg("--choose").exec();
                            eprintln!("emuera-tty: can't restart: {}", e);
                        }
                        return InputResult::Quit;
                    }
                },
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
                            self.scroll += self.wheel;
                            redraw = true;
                        }
                        65 => {
                            self.scroll = self.scroll.saturating_sub(self.wheel);
                            redraw = true;
                        }
                        b if b & 32 != 0 => {
                            // motion: update hover highlight
                            self.mouse = Some((x, y));
                            // the console pointer is drawn by us: move it, but
                            // only once a burst of motion has been read
                            if self.mice.is_some() || self.button_at(x, y) != self.hover {
                                pointer_owed = true;
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
                    if !matches!(k, Key::Other | Key::Left | Key::Right) {
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

    fn mouse_cell(&self) -> Option<(usize, usize)> {
        self.mouse
    }

    /// INPUTMOUSEKEY: the next click, wheel step or key (mouse moves only
    /// update the pointer, which MOUSEX / MOUSEY read), or the timeout.
    fn input_raw(&mut self, con: &mut Console, res: &mut Resources, timeout_ms: Option<u64>) -> crate::frontend::RawInput {
        use crate::frontend::{char_keycode, RawInput};
        self.scroll = 0;
        self.buf.clear();
        self.render(con, res, None);
        let deadline = timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        let mut moved = false;
        let cbg_buttons = res.cbg.iter().any(|l| l.button.is_some());
        loop {
            if self.hangup {
                return RawInput::Quit;
            }
            if moved && self.queued.is_empty() && self.pending.is_empty() && !self.input_waiting() {
                // a picture button may light up under the pointer
                if cbg_buttons {
                    self.render(con, res, None);
                } else {
                    self.repaint_hover(con.focus_fg);
                }
                moved = false;
            }
            let wait = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return RawInput::Timeout;
                    }
                    ((d - now).as_millis() as i32).clamp(1, 200)
                }
                None => 200,
            };
            let Some(k) = self.next_key(wait) else { continue };
            let key = |code: i64| RawInput::Key { code, data: code };
            match k {
                Key::CtrlC => return RawInput::Quit,
                Key::Settings => {
                    match self.settings_overlay(con, res, &InputReq { kind: InputKind::Wait, one: false, timeout_ms: None, mouse: true }) {
                        OverlayExit::Quit => return RawInput::Quit,
                        _ => {
                            self.force_frame = true;
                            self.render(con, res, None);
                        }
                    }
                }
                Key::CtrlL => {
                    self.force_frame = true;
                    self.render(con, res, None);
                }
                Key::Mouse { btn, x, y, press } => {
                    let cell = (x.saturating_sub(1) as usize, y.saturating_sub(1) as usize);
                    match btn & !(4 | 8 | 16) {
                        64 => return RawInput::Wheel { up: true, cell },
                        65 => return RawInput::Wheel { up: false, cell },
                        b if b & 32 != 0 => {
                            self.mouse = Some(cell);
                            moved = true;
                        }
                        b @ 0..=2 if press => {
                            self.mouse = Some(cell);
                            let button = self.button_at(cell.0, cell.1).map(|i| self.buttons[i].clone());
                            return RawInput::Click { btn: b as u8, cell, button };
                        }
                        _ => {}
                    }
                }
                // Windows key codes (System.Windows.Forms.Keys)
                Key::Enter => return key(13),
                Key::Backspace => return key(8),
                Key::Esc => return key(27),
                Key::PageUp => return key(33),
                Key::PageDown => return key(34),
                Key::End => return key(35),
                Key::Home => return key(36),
                Key::Left => return key(37),
                Key::Up => return key(38),
                Key::Right => return key(39),
                Key::Down => return key(40),
                Key::Char(c) => {
                    let (code, data) = char_keycode(c);
                    return RawInput::Key { code, data };
                }
                Key::Other => {}
            }
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
