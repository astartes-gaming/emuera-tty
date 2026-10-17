//! Full-screen terminal frontend: raw-mode input with mouse support, a
//! redraw-per-frame renderer for the Emuera console, and inline images via
//! the kitty / sixel / framebuffer backends.

use crate::ast::Align;
use crate::console::{Console, Line, Rgb, Seg, Style};
use crate::frontend::{Frontend, InputKind, InputReq, InputResult};
use crate::gfx::{self, Backend, Placement};
use crate::resources::Resources;
use crate::vm::value::Value;
use std::io::Write;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, PartialEq)]
enum ColorMode {
    True,
    C256,
    C16,
}

#[derive(Clone)]
struct Cell {
    ch: char,
    w: u8,
    style: Style,
    button: Option<usize>,
}

struct ImgAt {
    name: String,
    col: usize,
    w: f32,
    h: f32,
    ypos: f32,
}

#[derive(Default)]
struct DRow {
    cells: Vec<Cell>,
    imgs: Vec<ImgAt>,
}

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
    mouse: Option<(usize, usize)>,
    /// per screen row: (col_start, col_end, button id)
    btn_map: Vec<Vec<(usize, usize, usize)>>,
    buttons: Vec<Value>,
    hover: Option<usize>,
    active: bool,
    fb_cell_fixed: bool,
}

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

fn write_all(b: &[u8]) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b);
    let _ = out.flush();
}

impl Tty {
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
            mouse: None,
            btn_map: Vec::new(),
            buttons: Vec::new(),
            hover: None,
            active: true,
            fb_cell_fixed: false,
        };
        if xp > 0 && yp > 0 {
            t.cell = (xp / cols as u32, yp / rows as u32);
        }
        t.setup_gfx(gfx_pref, &term);
        // alt screen, hide cursor, mouse (SGR, any-motion)
        write_all(b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1003h\x1b[?1006h\x1b[2J");
        Ok(t)
    }

    fn read_bytes(&mut self, timeout_ms: i32) -> bool {
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r <= 0 {
            return false;
        }
        let mut b = [0u8; 4096];
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
        if n > 0 {
            self.pending.extend_from_slice(&b[..n as usize]);
            true
        } else {
            false
        }
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

    fn next_key(&mut self, timeout_ms: i32) -> Option<Key> {
        if self.pending.is_empty() && !self.read_bytes(timeout_ms) {
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
                    if b.get(2) == Some(&b'<') {
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
    fn layout_line(&mut self, line: &Line, cur_gen: u64, scale: f32) -> Vec<DRow> {
        let width = self.cols.max(1);
        let mut rows = vec![DRow::default()];
        let cw = self.cell.0.max(1) as f32;
        let mut col = 0usize;
        let active = line.gen == cur_gen;
        let mut push_btn = |v: &Option<Value>, buttons: &mut Vec<Value>| -> Option<usize> {
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
                        let w = if ch == '\t' { 4 } else { UnicodeWidthChar::width(ch).unwrap_or(0) };
                        if w == 0 {
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
                            r.cells.push(Cell { ch, w: w as u8, style: style.clone(), button: bid });
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
        // alignment
        if line.align != Align::Left {
            for r in rows.iter_mut() {
                let used: usize = r.cells.iter().map(|c| c.w as usize).sum();
                let pad = match line.align {
                    Align::Center => (width.saturating_sub(used)) / 2,
                    _ => width.saturating_sub(used),
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
                if bright && self.color == ColorMode::C16 {
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
        let need = view_h + self.scroll;
        let cur_line = if con.cur.is_empty() {
            None
        } else {
            Some(Line { segs: con.cur.clone(), align: con.cur_align.unwrap_or(con.align), gen: con.gen, temporary: false })
        };
        let mut chunks: Vec<Vec<DRow>> = Vec::new();
        let mut count = 0;
        if let Some(l) = &cur_line {
            let r = self.layout_line(l, con.gen, scale);
            count += r.len();
            chunks.push(r);
        }
        for l in con.lines.iter().rev() {
            if count >= need {
                break;
            }
            let r = self.layout_line(l, con.gen, scale);
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
        for (sr, row) in visible.iter().enumerate() {
            let _ = write!(out, "\x1b[{};1H", sr + 1);
            let mut col = 0usize;
            let mut last_style: Option<(Style, Option<Rgb>)> = None;
            let mut btn_start: Option<(usize, usize)> = None;
            for cell in &row.cells {
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
                out.extend_from_slice(cell.ch.encode_utf8(&mut tmp).as_bytes());
                col += cell.w as usize;
            }
            if let Some((s, b)) = btn_start {
                self.btn_map[sr].push((s, col, b));
            }
            out.extend_from_slice(b"\x1b[0m\x1b[K");
            // images on this row
            for im in &row.imgs {
                let pw = (im.w * scale).round().max(1.0) as u32;
                let ph = (im.h * scale).round().max(1.0) as u32;
                let yoff = (im.ypos * scale).round() as i32;
                let avail_px = ((view_h - sr) as i32 * self.cell.1 as i32 - yoff.max(0)).max(0) as u32;
                let crop_h = ph.min(avail_px);
                if crop_h == 0 {
                    continue;
                }
                if let Some(img) = res.scaled(&im.name, pw, ph) {
                    placements.push(Placement {
                        key: format!("{}@{}x{}", im.name, pw, ph),
                        img,
                        col: im.col as i32,
                        row: sr as i32,
                        y_off: yoff.max(0),
                        crop_top: 0,
                        crop_h,
                    });
                }
            }
        }
        for sr in visible.len()..view_h {
            let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[K", sr + 1);
        }
        // status / input line
        let _ = write!(out, "\x1b[{};1H\x1b[0m\x1b[K", self.rows);
        let prompt = match req {
            Some(r) if r.kind == InputKind::Wait => "\x1b[2m▼ (Enter)\x1b[0m".to_string(),
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

    fn button_at(&self, x: usize, y: usize) -> Option<usize> {
        let row = self.btn_map.get(y)?;
        row.iter().find(|(s, e, _)| x >= *s && x < *e).map(|(_, _, b)| *b)
    }
}

fn rgb256(c: Rgb) -> u8 {
    let q = |v: u8| -> u8 { if v < 48 { 0 } else if v < 115 { 1 } else { (v - 35) / 40 } };
    16 + 36 * q(c.0) + 6 * q(c.1) + q(c.2)
}

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

    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        self.buf.clear();
        self.scroll = 0;
        self.render(con, res, Some(req));
        let deadline = req.timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        let (mut last_cols, mut last_rows) = (self.cols, self.rows);
        loop {
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
                    match btn {
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
                            if self.button_at(x, y) != self.hover {
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
            if redraw {
                self.render(con, res, Some(req));
            }
        }
    }

    fn cols(&self) -> usize {
        self.cols
    }

    fn shutdown(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = Vec::new();
        self.gfx.shutdown(&mut out);
        out.extend_from_slice(b"\x1b[?1006l\x1b[?1003l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
        write_all(&out);
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
