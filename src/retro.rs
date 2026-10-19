//! Frontend for retro and dumb terminals: real VT52 / VT100 / ADM-3A /
//! Wyse / Teletype hardware, on the local tty or a serial line.
//!
//! Everything the terminal can do comes from its terminfo entry: cursor
//! addressing, clearing, attributes, the line-drawing set, keys. Output
//! is plain ASCII: box drawing uses the terminal's alternate character
//! set when it has one, kana are romanised and other wide characters are
//! drawn as two ASCII cells, so maps and menus keep Emuera's column
//! layout. Only what changed since the last frame is sent, which keeps a
//! 9600-baud line usable. Terminals without cursor addressing get a line
//! mode that prints the console as it scrolls.

use crate::ast::Align;
use crate::console::{Console, Line, Seg};
use crate::frontend::{Frontend, InputKind, InputReq, InputResult};
use crate::resources::Resources;
use crate::terminfo::{self as ti, TermInfo};
use std::time::{Duration, Instant};

/// Where the terminal is: our own stdin/stdout, or a serial device.
#[cfg(unix)]
pub struct Port {
    in_fd: i32,
    out_fd: i32,
    /// line speed, for delay padding and how long to wait for a key's rest
    pub baud: u32,
    orig: Option<libc::termios>,
    /// a serial device we opened (closed on drop)
    owned: bool,
}

/// Serial line settings for [`Port::serial`].
pub struct SerialOpts {
    pub baud: u32,
    /// data bits, parity ('N', 'E', 'O'), stop bits: "8N1", "7E1"…
    pub format: String,
    /// "xon" (XON/XOFF, the usual for terminals), "rts" (RTS/CTS), "none"
    pub flow: String,
}

/// RTS/CTS flow control (Redox has none).
#[cfg(all(unix, not(target_os = "redox")))]
const CRTSCTS: libc::tcflag_t = libc::CRTSCTS;
#[cfg(target_os = "redox")]
const CRTSCTS: libc::tcflag_t = 0;

#[cfg(unix)]
fn baud_const(b: u32) -> Option<libc::speed_t> {
    // the BSDs and macOS: a speed is the baud rate itself
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly"))]
    if [50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600].contains(&b) {
        return Some(b as libc::speed_t);
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly"))]
    return None;
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly")))]
    Some(match b {
        50 => libc::B50,
        75 => libc::B75,
        110 => libc::B110,
        134 => libc::B134,
        150 => libc::B150,
        200 => libc::B200,
        300 => libc::B300,
        600 => libc::B600,
        1200 => libc::B1200,
        1800 => libc::B1800,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        #[cfg(not(target_os = "haiku"))]
        460800 => libc::B460800,
        #[cfg(not(target_os = "haiku"))]
        921600 => libc::B921600,
        _ => return None,
    })
}

#[cfg(unix)]
fn speed_baud(s: libc::speed_t) -> u32 {
    for b in [50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600] {
        if baud_const(b) == Some(s) {
            return b;
        }
    }
    // pseudo terminals report 38400; anything unknown is fast
    38400
}

#[cfg(unix)]
impl Port {
    /// The terminal we were started on, in raw mode.
    pub fn local() -> Result<Port, String> {
        unsafe {
            if libc::isatty(0) == 0 || libc::isatty(1) == 0 {
                return Err("stdin/stdout is not a terminal".into());
            }
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut orig) != 0 {
                return Err("tcgetattr failed".into());
            }
            let mut raw = orig;
            libc::cfmakeraw(&mut raw);
            // keep the line's XON/XOFF: old terminals pause us with it
            raw.c_iflag |= orig.c_iflag & (libc::IXON | libc::IXOFF);
            libc::tcsetattr(0, libc::TCSANOW, &raw);
            Ok(Port { in_fd: 0, out_fd: 1, baud: speed_baud(libc::cfgetospeed(&orig)), orig: Some(orig), owned: false })
        }
    }

    /// Open and configure a serial device (`/dev/ttyS0`, `/dev/ttyUSB0`…).
    pub fn serial(dev: &str, o: &SerialOpts) -> Result<Port, String> {
        let path = std::ffi::CString::new(dev).map_err(|_| "bad device name".to_string())?;
        let speed = baud_const(o.baud).ok_or_else(|| format!("unsupported baud rate {}", o.baud))?;
        let f = o.format.to_ascii_uppercase();
        let fb: Vec<char> = f.chars().collect();
        if fb.len() != 3 || !matches!(fb[0], '5'..='8') || !matches!(fb[1], 'N' | 'E' | 'O') || !matches!(fb[2], '1' | '2') {
            return Err(format!("bad serial format {:?} (want e.g. 8N1, 7E1)", o.format));
        }
        unsafe {
            // O_NONBLOCK so a line without carrier doesn't hang the open
            let fd = libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC);
            if fd < 0 {
                return Err(format!("{}: {}", dev, std::io::Error::last_os_error()));
            }
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut orig) != 0 {
                libc::close(fd);
                return Err(format!("{}: not a serial port", dev));
            }
            let mut t = orig;
            libc::cfmakeraw(&mut t);
            libc::cfsetispeed(&mut t, speed);
            libc::cfsetospeed(&mut t, speed);
            t.c_cflag |= libc::CLOCAL | libc::CREAD;
            t.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::PARODD | libc::CSTOPB | CRTSCTS);
            t.c_cflag |= match fb[0] {
                '5' => libc::CS5,
                '6' => libc::CS6,
                '7' => libc::CS7,
                _ => libc::CS8,
            };
            match fb[1] {
                'E' => t.c_cflag |= libc::PARENB,
                'O' => t.c_cflag |= libc::PARENB | libc::PARODD,
                _ => {}
            }
            if fb[2] == '2' {
                t.c_cflag |= libc::CSTOPB;
            }
            if fb[0] != '8' {
                t.c_iflag |= libc::ISTRIP;
            }
            match o.flow.as_str() {
                "rts" | "rtscts" | "hw" => t.c_cflag |= CRTSCTS,
                "none" | "off" => {}
                _ => t.c_iflag |= libc::IXON | libc::IXOFF,
            }
            if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
                libc::close(fd);
                return Err(format!("{}: can't configure the line", dev));
            }
            // writes block from here on (reads are polled)
            let fl = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, fl & !libc::O_NONBLOCK);
            libc::tcflush(fd, libc::TCIOFLUSH);
            Ok(Port { in_fd: fd, out_fd: fd, baud: o.baud, orig: Some(orig), owned: true })
        }
    }

    fn write(&self, b: &[u8]) {
        let mut off = 0;
        while off < b.len() {
            let n = unsafe { libc::write(self.out_fd, b[off..].as_ptr() as *const libc::c_void, b.len() - off) };
            if n < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            off += n as usize;
        }
    }

    /// Bytes waiting within `timeout_ms`; None when the line is gone.
    fn read(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        let r = crate::sys::poll_fd(self.in_fd, timeout_ms, -1);
        if r.hangup {
            return None;
        }
        if !r.input {
            return Some(Vec::new());
        }
        let mut b = [0u8; 256];
        let n = unsafe { libc::read(self.in_fd, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
        if n == 0 {
            return None;
        }
        if n < 0 {
            let e = std::io::Error::last_os_error().kind();
            return if e == std::io::ErrorKind::WouldBlock || e == std::io::ErrorKind::Interrupted { Some(Vec::new()) } else { None };
        }
        Some(b[..n as usize].to_vec())
    }

    /// Terminal size from the driver (0 when it doesn't know, e.g. serial).
    fn winsize(&self) -> (usize, usize) {
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(self.out_fd, libc::TIOCGWINSZ, &mut ws) == 0 {
                (ws.ws_col as usize, ws.ws_row as usize)
            } else {
                (0, 0)
            }
        }
    }

    fn restore(&mut self) {
        if let Some(o) = self.orig.take() {
            unsafe {
                libc::tcdrain(self.out_fd);
                libc::tcsetattr(self.in_fd, libc::TCSANOW, &o);
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Port {
    fn drop(&mut self) {
        self.restore();
        if self.owned {
            unsafe { libc::close(self.in_fd) };
        }
    }
}

/// Where the terminal is: the console, or a COM port.
#[cfg(windows)]
pub struct Port {
    /// a COM port we opened (closed on drop); None: the console
    com: Option<usize>,
    raw: Option<crate::sys::RawMode>,
    /// line speed, for delay padding and how long to wait for a key's rest
    pub baud: u32,
    /// a serial line rather than the console
    owned: bool,
}

#[cfg(windows)]
impl Port {
    /// The console we were started on, in raw mode.
    pub fn local() -> Result<Port, String> {
        let raw = crate::sys::RawMode::enter_with(true)?;
        Ok(Port { com: None, baud: 38400, raw: Some(raw), owned: false })
    }

    /// Open and configure a COM port (`COM1`, `\\.\COM10`…).
    pub fn serial(dev: &str, o: &SerialOpts) -> Result<Port, String> {
        use windows_sys::Win32::Devices::Communication::*;
        use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING};
        let f = o.format.to_ascii_uppercase();
        let fb: Vec<char> = f.chars().collect();
        if fb.len() != 3 || !matches!(fb[0], '5'..='8') || !matches!(fb[1], 'N' | 'E' | 'O') || !matches!(fb[2], '1' | '2') {
            return Err(format!("bad serial format {:?} (want e.g. 8N1, 7E1)", o.format));
        }
        let name = if dev.starts_with(r"\\.\") { dev.to_string() } else { format!(r"\\.\{}", dev) };
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let h = CreateFileW(wide.as_ptr(), GENERIC_READ | GENERIC_WRITE, 0, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                return Err(format!("{}: {}", dev, std::io::Error::last_os_error()));
            }
            let mut dcb: DCB = std::mem::zeroed();
            dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
            if GetCommState(h, &mut dcb) == 0 {
                CloseHandle(h);
                return Err(format!("{}: not a serial port", dev));
            }
            dcb.BaudRate = o.baud;
            dcb.ByteSize = fb[0] as u8 - b'0';
            dcb.Parity = match fb[1] {
                'E' => EVENPARITY,
                'O' => ODDPARITY,
                _ => NOPARITY,
            };
            dcb.StopBits = if fb[2] == '2' { TWOSTOPBITS } else { ONESTOPBIT };
            // bit fields: fBinary 0, fParity 1, fOutxCtsFlow 2, fDtrControl
            // 4-5, fOutX 8, fInX 9, fRtsControl 12-13
            let mut bits: u32 = 1 | (1 << 4);
            if fb[1] != 'N' {
                bits |= 1 << 1;
            }
            match o.flow.as_str() {
                "rts" | "rtscts" | "hw" => bits |= (1 << 2) | (2 << 12),
                "none" | "off" => bits |= 1 << 12,
                _ => {
                    bits |= (1 << 8) | (1 << 9) | (1 << 12);
                    dcb.XonChar = 0x11;
                    dcb.XoffChar = 0x13;
                    dcb.XonLim = 64;
                    dcb.XoffLim = 64;
                }
            }
            dcb._bitfield = bits;
            if SetCommState(h, &dcb) == 0 {
                CloseHandle(h);
                return Err(format!("{}: can't configure the line (baud {}?)", dev, o.baud));
            }
            Ok(Port { com: Some(h as usize), baud: o.baud, raw: None, owned: true })
        }
    }

    fn write(&self, b: &[u8]) {
        match self.com {
            Some(h) => {
                use windows_sys::Win32::Storage::FileSystem::WriteFile;
                let mut off = 0;
                while off < b.len() {
                    let mut n = 0u32;
                    if unsafe { WriteFile(h as _, b[off..].as_ptr(), (b.len() - off) as u32, &mut n, std::ptr::null_mut()) } == 0 || n == 0 {
                        return;
                    }
                    off += n as usize;
                }
            }
            None => {
                use std::io::Write;
                let mut o = std::io::stdout().lock();
                let _ = o.write_all(b);
                let _ = o.flush();
            }
        }
    }

    /// Bytes waiting within `timeout_ms`; None when the line is gone.
    fn read(&self, timeout_ms: i32) -> Option<Vec<u8>> {
        let mut b = [0u8; 256];
        match self.com {
            Some(h) => {
                use windows_sys::Win32::Devices::Communication::{SetCommTimeouts, COMMTIMEOUTS};
                use windows_sys::Win32::Storage::FileSystem::ReadFile;
                // return as soon as a byte is there, else after the timeout
                let t = COMMTIMEOUTS {
                    ReadIntervalTimeout: u32::MAX,
                    ReadTotalTimeoutMultiplier: u32::MAX,
                    ReadTotalTimeoutConstant: if timeout_ms < 0 { u32::MAX - 1 } else { (timeout_ms as u32).max(1) },
                    WriteTotalTimeoutMultiplier: 0,
                    WriteTotalTimeoutConstant: 0,
                };
                let mut n = 0u32;
                unsafe {
                    SetCommTimeouts(h as _, &t);
                    if ReadFile(h as _, b.as_mut_ptr(), b.len() as u32, &mut n, std::ptr::null_mut()) == 0 {
                        return None;
                    }
                }
                Some(b[..n as usize].to_vec())
            }
            None => {
                let r = crate::sys::poll(timeout_ms, -1);
                if r.hangup {
                    return None;
                }
                if !r.input {
                    return Some(Vec::new());
                }
                match crate::sys::read_stdin(&mut b) {
                    Ok(0) => None,
                    Ok(n) => Some(b[..n].to_vec()),
                    Err(_) => Some(Vec::new()),
                }
            }
        }
    }

    /// Console size (0 on a serial line: unknown).
    fn winsize(&self) -> (usize, usize) {
        match self.com {
            Some(_) => (0, 0),
            None => crate::sys::winsize().map_or((0, 0), |(c, r, _, _)| (c, r)),
        }
    }

    fn restore(&mut self) {
        if let Some(r) = self.raw.take() {
            r.restore();
        }
    }
}

#[cfg(windows)]
impl Drop for Port {
    fn drop(&mut self) {
        self.restore();
        if let Some(h) = self.com.take() {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(h as _) };
        }
    }
}

// cell attributes
const A_BOLD: u8 = 1;
const A_UL: u8 = 2;
const A_REV: u8 = 4;

/// One screen cell: an ASCII byte, from the alternate (line drawing) set
/// or not, attributes and an ANSI colour 1-6 (0: default).
#[derive(Clone, Copy, PartialEq, Debug)]
struct RCell {
    ch: u8,
    acs: bool,
    attr: u8,
    color: u8,
}

const BLANK: RCell = RCell { ch: b' ', acs: false, attr: 0, color: 0 };

/// Emuera's width of a character: its Shift_JIS byte count.
fn emu_width(c: char) -> usize {
    if c.is_ascii() || ('\u{ff61}'..='\u{ff9f}').contains(&c) {
        1
    } else {
        let mut b = [0u8; 4];
        crate::loader::sjis_len(c.encode_utf8(&mut b)).clamp(1, 2)
    }
}

// Kunrei romaji for ぁ (U+3041) … ゖ (U+3096); katakana are 0x60 above.
const KANA: [&str; 86] = [
    "a ", "a ", "i ", "i ", "u ", "u ", "e ", "e ", "o ", "o ", "ka", "ga", "ki", "gi", "ku", "gu", "ke", "ge", "ko", "go",
    "sa", "za", "si", "zi", "su", "zu", "se", "ze", "so", "zo", "ta", "da", "ti", "di", "' ", "tu", "du", "te", "de", "to",
    "do", "na", "ni", "nu", "ne", "no", "ha", "ba", "pa", "hi", "bi", "pi", "hu", "bu", "pu", "he", "be", "pe", "ho", "bo",
    "po", "ma", "mi", "mu", "me", "mo", "ya", "ya", "yu", "yu", "yo", "yo", "ra", "ri", "ru", "re", "ro", "wa", "wa", "wi",
    "we", "wo", "n ", "vu", "ka", "ke",
];

// half-width katakana ｦ (U+FF66) … ﾝ (U+FF9D): the sound's first letter
const HALF_KANA: &[u8; 56] = b"waiueoyyyt-aiueokkkkkssssstttttnnnnnhhhhhmmmmmyyyrrrrrwn";

/// The arms of a box-drawing character: up, down, left, right bits.
fn box_arms(c: char) -> Option<u8> {
    const U: u8 = 1;
    const D: u8 = 2;
    const L: u8 = 4;
    const R: u8 = 8;
    let u = c as u32;
    Some(match u {
        0x2500 | 0x2501 | 0x2504 | 0x2505 | 0x2508 | 0x2509 | 0x254c | 0x254d | 0x2550 | 0x2574 | 0x2576 | 0x2578 | 0x257a | 0x257c | 0x257e => L | R,
        0x2502 | 0x2503 | 0x2506 | 0x2507 | 0x250a | 0x250b | 0x254e | 0x254f | 0x2551 | 0x2575 | 0x2577 | 0x2579 | 0x257b | 0x257d | 0x257f => U | D,
        0x250c..=0x250f | 0x2552..=0x2554 | 0x256d => D | R,
        0x2510..=0x2513 | 0x2555..=0x2557 | 0x256e => D | L,
        0x2514..=0x2517 | 0x2558..=0x255a | 0x2570 => U | R,
        0x2518..=0x251b | 0x255b..=0x255d | 0x256f => U | L,
        0x251c..=0x2523 | 0x255e..=0x2560 => U | D | R,
        0x2524..=0x252b | 0x2561..=0x2563 => U | D | L,
        0x252c..=0x2533 | 0x2564..=0x2566 => D | L | R,
        0x2534..=0x253b | 0x2567..=0x2569 => U | L | R,
        0x253c..=0x254b | 0x256a..=0x256c => U | D | L | R,
        _ => return None,
    })
}

/// What a terminal can draw beyond ASCII: its alternate character set.
struct Charset {
    /// VT100 ACS name → the byte to send in the alternate set
    acs: [u8; 128],
    has_acs: bool,
}

impl Charset {
    fn new(t: &TermInfo) -> Charset {
        let mut acs = [0u8; 128];
        let mut has_acs = false;
        if let (Some(map), Some(_)) = (t.str(ti::S_ACSC), t.str(ti::S_SMACS)) {
            for pair in map.chunks(2) {
                if let [name, ch] = *pair {
                    if (name as usize) < 128 {
                        acs[name as usize] = ch;
                        has_acs = true;
                    }
                }
            }
        }
        Charset { acs, has_acs }
    }

    /// The alternate-set cell for VT100 ACS name `n`, if the terminal has it.
    fn alt(&self, n: u8) -> Option<RCell> {
        let c = *self.acs.get(n as usize)?;
        (self.has_acs && c != 0).then_some(RCell { ch: c, acs: true, attr: 0, color: 0 })
    }

    /// `c`, `w` cells wide in Emuera, as cells this terminal can show.
    fn glyph(&self, c: char, w: usize) -> Vec<RCell> {
        let a = |s: &str| -> Vec<RCell> { s.bytes().map(|b| RCell { ch: b, ..BLANK }).collect() };
        let mut v: Vec<RCell> = if c.is_ascii_graphic() || c == ' ' {
            a(&c.to_string())
        } else if let Some(arms) = box_arms(c) {
            let name = match arms {
                12 => b'q',
                3 => b'x',
                10 => b'l',
                6 => b'k',
                9 => b'm',
                5 => b'j',
                11 => b't',
                7 => b'u',
                14 => b'w',
                13 => b'v',
                _ => b'n',
            };
            let ascii = match arms {
                12 => "-",
                3 => "|",
                _ => "+",
            };
            let first = self.alt(name).map(|x| vec![x]).unwrap_or_else(|| a(ascii));
            let mut v = first;
            // a line going right carries on through the second cell
            if w > 1 {
                if arms & 8 != 0 {
                    v.extend(self.alt(b'q').map(|x| vec![x]).unwrap_or_else(|| a("-")));
                } else {
                    v.push(BLANK);
                }
            }
            v
        } else {
            let u = c as u32;
            match c {
                '\u{3000}' => a("  "),
                '\u{ff01}'..='\u{ff5e}' => a(&format!("{} ", char::from_u32(u - 0xfee0).unwrap_or('?'))),
                '\u{3041}'..='\u{3096}' => a(KANA[(u - 0x3041) as usize]),
                '\u{30a1}'..='\u{30f6}' => a(KANA[(u - 0x30a1) as usize]),
                'ー' | '―' | '‐' => a("--"),
                '\u{ff66}'..='\u{ff9d}' => a(&(HALF_KANA[(u - 0xff66) as usize] as char).to_string()),
                'ﾞ' => a("\""),
                'ﾟ' => a("o"),
                '｡' => a("."),
                '｢' => a("["),
                '｣' => a("]"),
                '､' => a(","),
                '･' => a("."),
                '█' | '▉' | '▇' | '▓' | '■' | '▮' => {
                    let cell = self.alt(b'0').map(|x| vec![x]).unwrap_or_else(|| a("#"));
                    std::iter::repeat_n(cell, w).flatten().collect()
                }
                '▒' => self.alt(b'a').map(|x| vec![x; w]).unwrap_or_else(|| a(&":".repeat(w))),
                '░' => self.alt(b'h').or(self.alt(b'a')).map(|x| vec![x; w]).unwrap_or_else(|| a(&".".repeat(w))),
                '□' | '▢' => a("[]"),
                '◆' | '♦' => self.alt(b'`').map(|x| vec![x]).unwrap_or_else(|| a("<>")),
                '◇' | '◊' => a("<>"),
                '●' | '◉' => a("()"),
                '○' | '◯' | '◎' => a("o "),
                '★' => a("**"),
                '☆' | '※' | '✱' => a("* "),
                '・' | '·' | '•' => self.alt(b'~').map(|x| vec![x]).unwrap_or_else(|| a(". ")),
                '…' | '‥' => a(".."),
                '→' | '⇒' | '➡' => a("->"),
                '←' | '⇐' => a("<-"),
                '↑' | '⇑' => a("^ "),
                '↓' | '⇓' => a("v "),
                '▲' | '△' => a("/\\"),
                '▼' | '▽' => a("\\/"),
                '▶' | '▷' | '►' => a("|>"),
                '◀' | '◁' | '◄' => a("<|"),
                '〜' | '∽' | '∼' => a("~ "),
                '「' | '『' | '【' | '〔' | '〈' | '《' => a("[ "),
                '」' | '』' | '】' | '〕' | '〉' | '》' => a(" ]"),
                '、' => a(", "),
                '。' => a(". "),
                '♂' => a("m "),
                '♀' => a("f "),
                '♪' | '♫' => a("~ "),
                '♥' | '♡' => a("<3"),
                '×' | '✕' => a("x "),
                '÷' => a("/ "),
                '±' => self.alt(b'g').map(|x| vec![x]).unwrap_or_else(|| a("+-")),
                '°' | '˚' => self.alt(b'f').map(|x| vec![x]).unwrap_or_else(|| a("o")),
                '≦' | '≤' => self.alt(b'y').map(|x| vec![x]).unwrap_or_else(|| a("<=")),
                '≧' | '≥' => self.alt(b'z').map(|x| vec![x]).unwrap_or_else(|| a(">=")),
                '≠' => self.alt(b'|').map(|x| vec![x]).unwrap_or_else(|| a("!=")),
                '£' => self.alt(b'}').map(|x| vec![x]).unwrap_or_else(|| a("L")),
                '∫' | '∬' => a("S "),
                '∴' | '∵' => a(".:"),
                '╱' => a("/"),
                '╲' => a("\\"),
                '╳' => a("X"),
                '¥' | '￥' => a("Y"),
                '“' | '”' | '″' => a("\""),
                '‘' | '’' | '′' => a("'"),
                '—' | '–' => a("-"),
                '\t' => a("    "),
                _ => match latin_base(c) {
                    Some(b) => a(&b.to_string()),
                    None => a(&"?".repeat(w)),
                },
            }
        };
        v.truncate(w);
        while v.len() < w {
            v.push(BLANK);
        }
        v
    }
}

/// The letter under an accented Latin letter (é → e).
fn latin_base(c: char) -> Option<char> {
    const FROM: &str = "ÀÁÂÃÄÅàáâãäåÇçÈÉÊËèéêëÌÍÎÏìíîïÑñÒÓÔÕÖØòóôõöøÙÚÛÜùúûüÝýÿ";
    const TO: &str = "AAAAAAaaaaaaCcEEEEeeeeIIIIiiiiNnOOOOOOooooooUUUUuuuuYyy";
    FROM.chars().position(|f| f == c).and_then(|i| TO.chars().nth(i))
}

/// Keys we act on.
#[derive(Debug, PartialEq)]
enum Key {
    Char(char),
    Enter,
    Backspace,
    Esc,
    CtrlC,
    CtrlL,
    CtrlU,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Tab: continue and skip messages up to the next choice
    Skip,
    Other,
}

pub struct Retro {
    port: Port,
    t: TermInfo,
    cs: Charset,
    cols: usize,
    rows: usize,
    /// --size / terminfo size (else the driver's, re-read each frame)
    fixed_size: Option<(usize, usize)>,
    /// no cursor addressing: print lines as they come
    line_mode: bool,
    colors: bool,
    attrs: bool,
    /// what the terminal shows now (screen mode); None: unknown, redraw all
    shown: Option<Vec<Vec<RCell>>>,
    /// line mode: console lines already printed
    printed: usize,
    dropped_seen: usize,
    buf: String,
    scroll: usize,
    pending: Vec<u8>,
    hangup: bool,
    active: bool,
    /// key sequences from terminfo (and common defaults)
    keys: Vec<(Vec<u8>, Key)>,
}

impl Retro {
    /// `term`: terminfo name; `size`: forced (cols, rows); `force_line`:
    /// line mode even if the terminal can address the cursor.
    pub fn new(port: Port, term: &str, size: Option<(usize, usize)>, force_line: bool) -> Result<Retro, String> {
        let t = TermInfo::load(term)?;
        let cs = Charset::new(&t);
        let cup = t.str(ti::S_CUP).is_some();
        let line_mode = force_line || !cup || t.flag(ti::B_HC) || t.flag(ti::B_GN);
        let colors = t.num(ti::N_COLORS).unwrap_or(0) >= 8 && (t.str(ti::S_SETAF).is_some() || t.str(ti::S_SETF).is_some());
        // a magic-cookie terminal (attributes take a cell) gets none
        let attrs = t.num(ti::N_XMC).unwrap_or(0) == 0 && t.str(ti::S_SGR0).is_some();
        let fixed_size = size.or_else(|| {
            let (c, r) = port.winsize();
            if c > 0 && r > 0 {
                None
            } else {
                Some((t.num(ti::N_COLS).unwrap_or(80).max(20) as usize, t.num(ti::N_LINES).unwrap_or(24).max(4) as usize))
            }
        });
        let mut keys: Vec<(Vec<u8>, Key)> = Vec::new();
        for (cap, k) in [
            (ti::S_KCUU1, Key::Up),
            (ti::S_KCUD1, Key::Down),
            (ti::S_KPP, Key::PageUp),
            (ti::S_KNP, Key::PageDown),
            (ti::S_KHOME, Key::Home),
            (ti::S_KEND, Key::End),
            (ti::S_KBS, Key::Backspace),
            (ti::S_KENT, Key::Enter),
        ] {
            if let Some(s) = t.str(cap) {
                // a one-byte key (ADM-3A's ^K up, ^H backspace) only when
                // it isn't a key we need for itself
                if s.len() > 1 || matches!(k, Key::Backspace) {
                    keys.push((s.to_vec(), k));
                }
            }
        }
        // the ANSI / VT52 cursor keys in both modes, whatever the entry says
        for (s, k) in [
            (&b"\x1b[A"[..], Key::Up),
            (b"\x1b[B", Key::Down),
            (b"\x1bOA", Key::Up),
            (b"\x1bOB", Key::Down),
            (b"\x1bA", Key::Up),
            (b"\x1bB", Key::Down),
            (b"\x1b[5~", Key::PageUp),
            (b"\x1b[6~", Key::PageDown),
            (b"\x1b[H", Key::Home),
            (b"\x1b[F", Key::End),
            (b"\x1b[C", Key::Other),
            (b"\x1b[D", Key::Other),
            (b"\x1bOC", Key::Other),
            (b"\x1bOD", Key::Other),
            (b"\x1bC", Key::Other),
            (b"\x1bD", Key::Other),
        ] {
            if !keys.iter().any(|(q, _)| q == s) {
                keys.push((s.to_vec(), k));
            }
        }
        let (cols, rows) = fixed_size.unwrap_or_else(|| port.winsize());
        let r = Retro {
            port,
            t,
            cs,
            cols: cols.max(20),
            rows: rows.max(4),
            fixed_size,
            line_mode,
            colors,
            attrs,
            shown: None,
            printed: 0,
            dropped_seen: 0,
            buf: String::new(),
            scroll: 0,
            pending: Vec::new(),
            hangup: false,
            active: true,
            keys,
        };
        let mut init = Vec::new();
        if !r.line_mode {
            r.cap(&mut init, ti::S_SMCUP);
            r.cap(&mut init, ti::S_ENACS);
            r.cap(&mut init, ti::S_SMKX);
        }
        r.port.write(&init);
        Ok(r)
    }

    pub fn term_name(&self) -> &str {
        self.t.name()
    }

    pub fn mode_name(&self) -> &'static str {
        if self.line_mode {
            "line"
        } else {
            "screen"
        }
    }

    /// Append capability `i` (with delays padded) if the terminal has it.
    fn cap(&self, out: &mut Vec<u8>, i: usize) -> bool {
        match self.t.str(i) {
            Some(s) => {
                out.extend(ti::pad(s, self.port.baud, &self.t, 1));
                true
            }
            None => false,
        }
    }

    fn update_size(&mut self) {
        let (c, r) = self.fixed_size.unwrap_or_else(|| self.port.winsize());
        let (c, r) = (c.max(20), r.max(4));
        if (c, r) != (self.cols, self.rows) {
            self.cols = c;
            self.rows = r;
            self.shown = None;
        }
    }

    /// Lay a console line out as rows of cells, `width` wide.
    fn layout(&self, line: &Line, con: &Console, width: usize) -> Vec<Vec<RCell>> {
        let width = if line.cols > 0 { line.cols.min(width) } else { width };
        let mut rows: Vec<Vec<RCell>> = vec![Vec::new()];
        let mut divs: Vec<(usize, usize, i32, Vec<Vec<RCell>>)> = Vec::new();
        let cell_px = (con.font_size / 2.0).max(1.0);
        let push = |rows: &mut Vec<Vec<RCell>>, cells: Vec<RCell>| {
            if rows.last().unwrap().len() + cells.len() > width && !rows.last().unwrap().is_empty() {
                rows.push(Vec::new());
            }
            let r = rows.last_mut().unwrap();
            for c in cells {
                if r.len() < width {
                    r.push(c);
                }
            }
        };
        for (si, seg) in line.segs.iter().enumerate() {
            match seg {
                Seg::Text { text, style, button, .. } => {
                    // a button that doesn't fit the rest of the row starts
                    // the next one whole (the game's button wrapping setting)
                    let starts = button.is_some() && !matches!(si.checked_sub(1).and_then(|p| line.segs.get(p)), Some(Seg::Text { button: b, .. }) if b == button);
                    let used = rows.last().unwrap().len();
                    if con.button_wrap && starts && used > 0 {
                        let w: usize = line.segs[si..]
                            .iter()
                            .map_while(|s2| match s2 {
                                Seg::Text { text: t2, button: b2, .. } if b2 == button => Some(t2.chars().map(|c| if c == '\t' { 4 } else { emu_width(c) }).sum::<usize>()),
                                _ => None,
                            })
                            .sum();
                        if used + w > width && w <= width {
                            rows.push(Vec::new());
                        }
                    }
                    let mut attr = 0;
                    if self.attrs {
                        if style.bold {
                            attr |= A_BOLD;
                        }
                        if style.underline {
                            attr |= A_UL;
                        }
                    }
                    let color = if self.colors && style.fg != con.default_fg {
                        match crate::tty::rgb16(style.fg) {
                            (c @ 1..=6, _) => c,
                            _ => 0,
                        }
                    } else {
                        0
                    };
                    for ch in text.chars() {
                        if ch == '\r' || ch == '\n' || (ch as u32) < 0x20 && ch != '\t' {
                            continue;
                        }
                        let w = if ch == '\t' { 4 } else { emu_width(ch) };
                        let mut cells = self.cs.glyph(ch, w);
                        for c in cells.iter_mut() {
                            c.attr = attr;
                            c.color = color;
                        }
                        push(&mut rows, cells);
                    }
                }
                Seg::Image { w, .. } => {
                    let n = ((w / cell_px).round().max(1.0) as usize).min(width);
                    push(&mut rows, vec![BLANK; n]);
                }
                Seg::Space { w } => {
                    let n = ((w / cell_px).round().max(0.0) as usize).min(width.saturating_sub(rows.last().unwrap().len()));
                    push(&mut rows, vec![BLANK; n]);
                }
                Seg::Rect { w, .. } => {
                    let n = ((w / cell_px).round().max(1.0) as usize).min(width);
                    let cell = self.cs.alt(b'0').unwrap_or(RCell { ch: b'#', ..BLANK });
                    push(&mut rows, vec![cell; n]);
                }
                Seg::Div { dx, dy, cols, rows: max_rows, lines, .. } => {
                    let mut inner = Vec::new();
                    for l in lines {
                        inner.extend(self.layout(l, con, *cols));
                    }
                    if let Some(m) = max_rows {
                        inner.truncate(*m);
                    }
                    let at = (rows.last().unwrap().len() as i64 + *dx as i64).max(0) as usize;
                    divs.push((rows.len() - 1, at, *dy, inner));
                }
            }
        }
        // trailing spaces that only spilled onto another row: drop it
        while rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(|c| *c == BLANK)) && divs.iter().all(|d| d.0 < rows.len() - 1) {
            rows.pop();
        }
        if line.align != Align::Left {
            for r in rows.iter_mut() {
                let pad = match line.align {
                    Align::Center => self.cols.saturating_sub(r.len()) / 2,
                    _ => self.cols.saturating_sub(r.len() + 1),
                };
                let mut v = vec![BLANK; pad];
                v.append(r);
                *r = v;
            }
        }
        // divs inside this line's own rows (ones reaching other lines are
        // drawn where they can be: here)
        for (ri, col, dy, inner) in divs {
            for (k, ir) in inner.into_iter().enumerate() {
                let t = ri as i64 + dy as i64 + k as i64;
                if t < 0 {
                    continue;
                }
                let t = t as usize;
                while rows.len() <= t {
                    rows.push(Vec::new());
                }
                let r = &mut rows[t];
                for (j, c) in ir.into_iter().enumerate() {
                    let x = col + j;
                    if x >= self.cols {
                        break;
                    }
                    while r.len() <= x {
                        r.push(BLANK);
                    }
                    r[x] = c;
                }
            }
        }
        rows
    }

    /// The whole screen for this frame: console rows and the input line.
    fn frame(&mut self, con: &mut Console, req: Option<&InputReq>) -> Vec<Vec<RCell>> {
        con.width = self.cols.saturating_sub(1).max(20);
        let view = self.rows - 1;
        let need = view + self.scroll;
        let mut chunks = Vec::new();
        let mut count = 0;
        if !con.cur.is_empty() {
            let l = Line { segs: con.cur.clone(), align: con.cur_align.unwrap_or(con.align), gen: con.gen, temporary: false, cols: self.cols, kojo: false };
            let r = self.layout(&l, con, self.cols);
            count += r.len();
            chunks.push(r);
        }
        for l in con.lines.iter().rev() {
            if count >= need {
                break;
            }
            let r = self.layout(l, con, self.cols);
            count += r.len();
            chunks.push(r);
        }
        let mut all: Vec<Vec<RCell>> = chunks.into_iter().rev().flatten().collect();
        self.scroll = self.scroll.min(all.len().saturating_sub(view));
        let end = all.len() - self.scroll;
        let start = end.saturating_sub(view);
        let mut screen: Vec<Vec<RCell>> = all.drain(start..end).collect();
        while screen.len() < view {
            screen.push(Vec::new());
        }
        // the input line
        let text = match req {
            Some(r) if r.kind == InputKind::Wait => "-- more (Return) --".to_string(),
            Some(_) => {
                let room = self.cols.saturating_sub(4);
                let b: Vec<char> = self.buf.chars().collect();
                let tail: String = b[b.len().saturating_sub(room)..].iter().collect();
                format!("> {}", tail)
            }
            None => String::new(),
        };
        let mut last: Vec<RCell> = Vec::new();
        for ch in text.chars() {
            last.extend(self.cs.glyph(ch, if ch.is_ascii() { 1 } else { emu_width(ch) }));
        }
        if self.scroll > 0 {
            let tag = format!(" [back {}] ", self.scroll);
            let at = self.cols.saturating_sub(tag.len() + 1);
            while last.len() < at {
                last.push(BLANK);
            }
            last.truncate(at);
            last.extend(tag.bytes().map(|b| RCell { ch: b, attr: if self.attrs { A_REV } else { 0 }, ..BLANK }));
        }
        screen.push(last);
        for r in screen.iter_mut() {
            r.truncate(self.cols);
            while r.last() == Some(&BLANK) {
                r.pop();
            }
        }
        // writing the bottom-right corner scrolls an auto-margin terminal
        if let Some(r) = screen.last_mut() {
            r.truncate(self.cols - 1);
        }
        screen
    }

    /// Send what differs between `self.shown` and `new`.
    fn paint(&mut self, new: Vec<Vec<RCell>>, cursor: Option<usize>) {
        let mut out: Vec<u8> = Vec::with_capacity(512);
        let full = self.shown.is_none();
        if full {
            self.cap(&mut out, ti::S_SGR0);
            if !self.cap(&mut out, ti::S_CLEAR) {
                // no clear: home and wipe every row
                self.cap(&mut out, ti::S_HOME);
            }
            self.shown = Some(vec![Vec::new(); self.rows]);
            if self.t.str(ti::S_CLEAR).is_none() {
                self.shown = Some(vec![vec![RCell { ch: 0, ..BLANK }; self.cols - 1]; self.rows]);
            }
        }
        let mut old = self.shown.take().unwrap();
        old.resize(self.rows, Vec::new());
        let mut pos: Option<(usize, usize)> = if full && self.t.str(ti::S_CLEAR).is_some() { Some((0, 0)) } else { None };
        let (mut attr, mut color, mut acs) = (0u8, 0u8, false);
        let has_el = self.t.str(ti::S_EL).is_some();
        for (r, row) in new.iter().enumerate() {
            let prev = &old[r];
            let len = row.len().max(prev.len());
            let at = |v: &Vec<RCell>, i: usize| v.get(i).copied().unwrap_or(BLANK);
            let mut c = 0;
            while c < len {
                if at(row, c) == at(prev, c) {
                    c += 1;
                    continue;
                }
                // the rest of the row is blank now: clear it in one go
                if c >= row.len() && has_el {
                    self.goto(&mut out, &mut pos, r, c);
                    if attr != 0 || color != 0 || acs {
                        self.set_attr(&mut out, &mut attr, &mut color, &mut acs, BLANK);
                    }
                    self.cap(&mut out, ti::S_EL);
                    break;
                }
                self.goto(&mut out, &mut pos, r, c);
                // write up to the next run of 4 unchanged cells
                let mut e = c;
                let mut same = 0;
                while e < len && same < 4 {
                    if at(row, e) == at(prev, e) {
                        same += 1;
                    } else {
                        same = 0;
                    }
                    e += 1;
                }
                let e = e - same;
                for k in c..e {
                    let cell = at(row, k);
                    self.set_attr(&mut out, &mut attr, &mut color, &mut acs, cell);
                    out.push(cell.ch);
                }
                pos = if e >= self.cols { None } else { Some((r, e)) };
                c = e;
            }
        }
        self.set_attr(&mut out, &mut attr, &mut color, &mut acs, BLANK);
        if let Some(col) = cursor {
            let r = self.rows - 1;
            self.goto(&mut out, &mut pos, r, col.min(self.cols - 2));
        }
        self.port.write(&out);
        self.shown = Some(new);
    }

    fn goto(&self, out: &mut Vec<u8>, pos: &mut Option<(usize, usize)>, r: usize, c: usize) {
        if *pos == Some((r, c)) {
            return;
        }
        // a carriage return is shorter than addressing the start of the row
        if c == 0 && pos.map(|p| p.0) == Some(r) {
            if !self.cap(out, ti::S_CR) {
                out.push(b'\r');
            }
        } else if let Some(cup) = self.t.str(ti::S_CUP) {
            out.extend(ti::pad(&ti::tparm(cup, &[r as i32, c as i32]), self.port.baud, &self.t, 1));
        }
        *pos = Some((r, c));
    }

    fn set_attr(&self, out: &mut Vec<u8>, attr: &mut u8, color: &mut u8, acs: &mut bool, cell: RCell) {
        if cell.attr != *attr || cell.color != *color {
            if *attr != 0 || *color != 0 {
                self.cap(out, ti::S_SGR0);
                // sgr0 may leave the line-drawing set too
                *acs = false;
                if let Some(rm) = self.t.str(ti::S_RMACS) {
                    if self.t.str(ti::S_SGR0) != Some(rm) {
                        out.extend(ti::pad(rm, self.port.baud, &self.t, 1));
                    }
                }
            }
            if cell.attr & A_BOLD != 0 {
                let _ = self.cap(out, ti::S_BOLD) || self.cap(out, ti::S_SMSO);
            }
            if cell.attr & A_UL != 0 {
                self.cap(out, ti::S_SMUL);
            }
            if cell.attr & A_REV != 0 {
                let _ = self.cap(out, ti::S_REV) || self.cap(out, ti::S_SMSO);
            }
            if cell.color != 0 {
                if let Some(s) = self.t.str(ti::S_SETAF) {
                    out.extend(ti::tparm(s, &[cell.color as i32]));
                } else if let Some(s) = self.t.str(ti::S_SETF) {
                    // the old setf order is BGR
                    let c = cell.color;
                    let bgr = ((c & 1) << 2) | (c & 2) | ((c & 4) >> 2);
                    out.extend(ti::tparm(s, &[bgr as i32]));
                }
            }
            *attr = cell.attr;
            *color = cell.color;
        }
        if cell.acs != *acs {
            self.cap(out, if cell.acs { ti::S_SMACS } else { ti::S_RMACS });
            *acs = cell.acs;
        }
    }

    fn render(&mut self, con: &mut Console, req: Option<&InputReq>) {
        self.update_size();
        let screen = self.frame(con, req);
        let cursor = match req {
            Some(r) if r.kind != InputKind::Wait => Some(2 + self.buf.chars().count().min(self.cols.saturating_sub(4))),
            _ => None,
        };
        self.paint(screen, cursor);
        con.dirty = false;
    }

    // ---- line mode ----

    /// Print the console lines not printed yet (reprinting after
    /// CLEARLINE / REUSELASTLINE, like the headless frontend).
    fn print_new(&mut self, con: &mut Console) {
        con.width = self.cols.saturating_sub(1).max(20);
        self.printed = self.printed.saturating_sub(con.dropped_front - self.dropped_seen);
        self.dropped_seen = con.dropped_front;
        self.printed = self.printed.min(con.low_water).min(con.lines.len());
        con.low_water = usize::MAX;
        let mut out = Vec::new();
        for i in self.printed..con.lines.len() {
            let rows = self.layout(&con.lines[i], con, self.cols);
            for r in rows {
                out.extend(Self::plain(&r));
                out.extend_from_slice(b"\r\n");
            }
        }
        self.printed = con.lines.len();
        self.port.write(&out);
        con.dirty = false;
    }

    /// A row as plain bytes (line mode prints no attributes or graphics).
    fn plain(r: &[RCell]) -> Vec<u8> {
        let mut v: Vec<u8> = r
            .iter()
            .map(|c| {
                if !c.acs {
                    c.ch
                } else {
                    match c.ch {
                        b'q' => b'-',
                        b'x' => b'|',
                        b'0' => b'#',
                        b'a' => b':',
                        _ => b'+',
                    }
                }
            })
            .collect();
        while v.last() == Some(&b' ') {
            v.pop();
        }
        v
    }

    // ---- keys ----

    fn next_key(&mut self, timeout_ms: i32) -> Option<Key> {
        if self.pending.is_empty() {
            match self.port.read(timeout_ms) {
                Some(b) => self.pending.extend(b),
                None => {
                    self.hangup = true;
                    return None;
                }
            }
        }
        if self.pending.is_empty() {
            return None;
        }
        // a key sequence still arriving: give the line time for the rest
        // (ten character times, at least 30 ms)
        if self.pending[0] == 0x1b {
            let wait = ((100_000 / self.port.baud.max(1)) as i32).max(30);
            let deadline = Instant::now() + Duration::from_millis(wait as u64);
            while self.keys.iter().any(|(s, _)| s.len() > self.pending.len() && s.starts_with(&self.pending)) && Instant::now() < deadline {
                match self.port.read(5) {
                    Some(b) => self.pending.extend(b),
                    None => break,
                }
            }
        }
        if let Some((n, k)) = self
            .keys
            .iter()
            .filter(|(s, _)| self.pending.starts_with(s))
            .max_by_key(|(s, _)| s.len())
            .map(|(s, k)| (s.len(), k))
        {
            let k = match k {
                Key::Up => Key::Up,
                Key::Down => Key::Down,
                Key::PageUp => Key::PageUp,
                Key::PageDown => Key::PageDown,
                Key::Home => Key::Home,
                Key::End => Key::End,
                Key::Backspace => Key::Backspace,
                Key::Enter => Key::Enter,
                _ => Key::Other,
            };
            self.pending.drain(..n);
            return Some(k);
        }
        let b = self.pending.remove(0);
        Some(match b {
            b'\r' => {
                // CR LF from a terminal set to send both: one Enter
                if self.pending.first() == Some(&b'\n') {
                    self.pending.remove(0);
                }
                Key::Enter
            }
            b'\n' => Key::Enter,
            0x08 | 0x7f => Key::Backspace,
            0x09 => Key::Skip,
            0x03 => Key::CtrlC,
            0x0c => Key::CtrlL,
            0x15 => Key::CtrlU,
            0x02 => Key::PageUp,   // ^B
            0x06 => Key::PageDown, // ^F
            0x10 => Key::Up,       // ^P
            0x0e => Key::Down,     // ^N
            0x1b => Key::Esc,
            b if (0x20..0x7f).contains(&b) => Key::Char(b as char),
            b if b >= 0xc0 => {
                // UTF-8 from a modern terminal: take the whole character
                let need = if b >= 0xf0 { 3 } else if b >= 0xe0 { 2 } else { 1 };
                let mut bytes = vec![b];
                while bytes.len() <= need && !self.pending.is_empty() {
                    bytes.push(self.pending.remove(0));
                }
                match std::str::from_utf8(&bytes).ok().and_then(|s| s.chars().next()) {
                    Some(c) => Key::Char(c),
                    None => Key::Other,
                }
            }
            _ => Key::Other,
        })
    }

    /// Line mode input: echo typed characters ourselves.
    fn line_input(&mut self, con: &mut Console, req: &InputReq) -> InputResult {
        self.print_new(con);
        let mut prompt = Vec::new();
        if !con.cur.is_empty() {
            let l = Line { segs: con.cur.clone(), align: con.align, gen: con.gen, temporary: false, cols: self.cols, kojo: false };
            let rows = self.layout(&l, con, self.cols);
            for (k, r) in rows.iter().enumerate() {
                prompt.extend(Self::plain(r));
                if k + 1 < rows.len() {
                    prompt.extend_from_slice(b"\r\n");
                }
            }
        }
        if req.kind == InputKind::Wait {
            prompt.extend_from_slice(b" [Return]");
        } else if prompt.is_empty() {
            prompt.extend_from_slice(b"> ");
        }
        self.port.write(&prompt);
        let deadline = req.timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        self.buf.clear();
        let done = |s: &mut Retro, r: InputResult| {
            s.port.write(b"\r\n");
            r
        };
        loop {
            if self.hangup {
                return InputResult::Quit;
            }
            let wait = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return done(self, InputResult::Timeout);
                    }
                    ((d - now).as_millis() as i32).min(200)
                }
                None => 200,
            };
            let Some(k) = self.next_key(wait) else { continue };
            match k {
                Key::CtrlC => return done(self, InputResult::Quit),
                Key::Skip if req.kind == InputKind::Wait => return done(self, InputResult::Skip(String::new())),
                _ if req.kind == InputKind::Wait => return done(self, InputResult::Text(String::new())),
                Key::Enter => {
                    let s = std::mem::take(&mut self.buf);
                    return done(self, InputResult::Text(s));
                }
                Key::Backspace => {
                    if self.buf.pop().is_some() {
                        self.port.write(if self.t.flag(ti::B_HC) { b"\\" } else { b"\x08 \x08" });
                    }
                }
                Key::CtrlU => {
                    let n = self.buf.chars().count();
                    self.buf.clear();
                    if self.t.flag(ti::B_HC) {
                        self.port.write(b" ^U\r\n> ");
                    } else {
                        self.port.write(&b"\x08 \x08".repeat(n));
                    }
                }
                Key::Char(c) => {
                    if req.one {
                        return done(self, InputResult::Text(c.to_string()));
                    }
                    self.buf.push(c);
                    let g = self.cs.glyph(c, if c.is_ascii() { 1 } else { emu_width(c) });
                    self.port.write(&Self::plain(&g));
                }
                _ => {}
            }
        }
    }
}

impl Frontend for Retro {
    fn flush(&mut self, con: &mut Console, _res: &mut Resources) {
        if self.line_mode {
            self.print_new(con);
        } else if con.redraw {
            self.render(con, None);
        }
    }

    fn refresh(&mut self, con: &mut Console, _res: &mut Resources) {
        if self.line_mode {
            self.print_new(con);
        } else {
            self.render(con, None);
        }
    }

    fn input(&mut self, con: &mut Console, _res: &mut Resources, req: &InputReq) -> InputResult {
        if self.line_mode {
            return self.line_input(con, req);
        }
        self.buf.clear();
        self.scroll = 0;
        self.render(con, Some(req));
        let deadline = req.timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        loop {
            if self.hangup {
                return InputResult::Quit;
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
            let Some(k) = self.next_key(wait) else { continue };
            let page = (self.rows / 2).max(1);
            let mut redraw = true;
            match k {
                Key::CtrlC => return InputResult::Quit,
                Key::CtrlL => self.shown = None,
                Key::PageUp => self.scroll += page,
                Key::PageDown => self.scroll = self.scroll.saturating_sub(page),
                Key::Up => self.scroll += 1,
                Key::Down => self.scroll = self.scroll.saturating_sub(1),
                Key::Home => self.scroll = usize::MAX / 2,
                Key::End => self.scroll = 0,
                Key::Skip if req.kind == InputKind::Wait => return InputResult::Skip(String::new()),
                _ if req.kind == InputKind::Wait => {
                    if k != Key::Other {
                        return InputResult::Text(String::new());
                    }
                    redraw = false;
                }
                Key::Enter => return InputResult::Text(std::mem::take(&mut self.buf)),
                Key::Backspace => {
                    self.buf.pop();
                }
                Key::Esc | Key::CtrlU => self.buf.clear(),
                Key::Char(c) => {
                    if req.one {
                        return InputResult::Text(c.to_string());
                    }
                    self.buf.push(c);
                }
                Key::Other | Key::Skip => redraw = false,
            }
            // a burst of typing is drawn once it has all been read
            if redraw && self.pending.is_empty() {
                self.render(con, Some(req));
            } else if redraw {
                let more = self.port.read(0);
                if let Some(b) = more {
                    self.pending.extend(b);
                }
                if self.pending.is_empty() {
                    self.render(con, Some(req));
                }
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
        // a serial terminal doesn't share our stderr
        !self.port.owned && !self.line_mode
    }

    fn poll_interrupt(&mut self) -> bool {
        match self.port.read(0) {
            Some(b) => self.pending.extend(b),
            None => self.hangup = true,
        }
        if let Some(i) = self.pending.iter().position(|&b| b == 0x03) {
            self.pending.drain(..=i);
            return true;
        }
        self.hangup
    }

    fn shutdown(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut out = Vec::new();
        if !self.line_mode {
            self.cap(&mut out, ti::S_SGR0);
            self.cap(&mut out, ti::S_RMACS);
            self.cap(&mut out, ti::S_RMKX);
            if let Some(cup) = self.t.str(ti::S_CUP) {
                out.extend(ti::tparm(cup, &[self.rows as i32 - 1, 0]));
            }
            out.extend_from_slice(b"\r\n");
            self.cap(&mut out, ti::S_RMCUP);
        }
        self.port.write(&out);
        self.port.restore();
    }
}

impl Drop for Retro {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Should a terminal be driven by this frontend rather than the ANSI one?
/// Yes for those whose cursor addressing isn't ANSI (VT52, ADM-3A, Wyse…)
/// and for those with none at all (`dumb`, hard copy).
pub fn wanted_for(term: &str) -> bool {
    if term == "dumb" {
        return true;
    }
    match TermInfo::load(term) {
        Ok(t) => match t.str(ti::S_CUP) {
            Some(cup) => !cup.starts_with(b"\x1b["),
            None => true,
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(acs: bool) -> Charset {
        let mut t = TermInfo::default();
        if acs {
            t = TermInfo::load("vt100").unwrap_or_default();
        }
        Charset::new(&t)
    }

    fn text(v: &[RCell]) -> String {
        v.iter().map(|c| if c.acs { format!("<{}>", c.ch as char) } else { (c.ch as char).to_string() }).collect()
    }

    #[test]
    fn glyphs_keep_emuera_widths() {
        let a = cs(false);
        assert_eq!(text(&a.glyph('A', 1)), "A");
        assert_eq!(text(&a.glyph('か', 2)), "ka");
        assert_eq!(text(&a.glyph('ン', 2)), "n ");
        assert_eq!(text(&a.glyph('Ａ', 2)), "A ");
        assert_eq!(text(&a.glyph('━', 2)), "--");
        assert_eq!(text(&a.glyph('┃', 2)), "| ");
        assert_eq!(text(&a.glyph('┏', 2)), "+-");
        assert_eq!(text(&a.glyph('■', 2)), "##");
        assert_eq!(text(&a.glyph('漢', 2)), "??");
        assert_eq!(text(&a.glyph('é', 1)), "e");
        assert_eq!(text(&a.glyph('ﾄ', 1)), "t");
        assert_eq!(KANA.len(), 0x3096 - 0x3041 + 1);
    }

    #[test]
    fn line_drawing_set_when_there_is_one() {
        let v = cs(true);
        if !v.has_acs {
            return; // no terminfo database here
        }
        assert_eq!(text(&v.glyph('┏', 2)), "<l><q>");
        assert_eq!(text(&v.glyph('│', 2)), "<x> ");
        // the VT100 set has a checkerboard but no solid block
        assert_eq!(text(&v.glyph('▒', 2)), "<a><a>");
        assert_eq!(text(&v.glyph('■', 2)), "##");
    }

    #[test]
    fn non_ansi_terminals_are_picked() {
        if TermInfo::load("vt52").is_err() {
            return;
        }
        assert!(wanted_for("vt52"));
        assert!(wanted_for("adm3a"));
        assert!(wanted_for("dumb"));
        assert!(!wanted_for("vt100"));
        assert!(!wanted_for("xterm-256color"));
    }
}
