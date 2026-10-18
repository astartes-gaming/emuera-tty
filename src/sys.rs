//! The platform layer: terminal raw mode, size and input, the clock, and
//! a few file-system details. Unix (Linux, the BSDs, macOS) goes through
//! termios and poll; Windows through the console API in its VT mode
//! (Windows Terminal, conhost on Windows 10+), where the console speaks
//! the same escape sequences as a Unix terminal.

use std::path::{Path, PathBuf};

/// Local date and time.
pub struct DateTime {
    pub y: i64,
    pub mo: i64,
    pub d: i64,
    pub h: i64,
    pub mi: i64,
    pub s: i64,
    pub ms: i64,
}

/// What [`poll`] saw.
#[derive(Default)]
pub struct Ready {
    /// stdin has bytes to read
    pub input: bool,
    /// the extra file descriptor (the Linux console mouse) is readable
    pub extra: bool,
    /// stdin was closed
    pub hangup: bool,
}

/// The user's home folder.
pub fn home_dir() -> Option<PathBuf> {
    let h = std::env::var_os("HOME").filter(|h| !h.is_empty());
    #[cfg(windows)]
    let h = h.or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()));
    h.map(PathBuf::from)
}

/// Where settings go: `$XDG_CONFIG_HOME`, else `~/.config` (on Windows
/// `%APPDATA%`).
pub fn config_base() -> Option<PathBuf> {
    let x = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute());
    #[cfg(windows)]
    let x = x.or_else(|| std::env::var_os("APPDATA").map(PathBuf::from).filter(|p| p.is_absolute()));
    x.or_else(|| home_dir().map(|h| h.join(".config")))
}

/// Where data goes: `$XDG_DATA_HOME`, else `~/.local/share` (on Windows
/// `%LOCALAPPDATA%`).
pub fn data_base() -> Option<PathBuf> {
    let x = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute());
    #[cfg(windows)]
    let x = x.or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_absolute()));
    x.or_else(|| home_dir().map(|h| h.join(".local").join("share")))
}

/// The program's file name: `emuera-tty` (`emuera-tty.exe` on Windows).
pub const EXE_NAME: &str = if cfg!(windows) { "emuera-tty.exe" } else { "emuera-tty" };

/// Run this program again with `args` in place of this one. Returns only
/// on failure.
pub fn exec_self(args: &[std::ffi::OsString]) -> std::io::Error {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return e,
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(exe).args(args).exec()
    }
    #[cfg(not(unix))]
    {
        // no exec: run it as a child and leave with its exit code
        match std::process::Command::new(exe).args(args).status() {
            Ok(s) => std::process::exit(s.code().unwrap_or(0)),
            Err(e) => e,
        }
    }
}

/// Is `d` an existing folder we can write into?
pub fn dir_writable(d: &Path) -> bool {
    if !d.is_dir() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(d.as_os_str().as_bytes()) else { return false };
        unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
    }
    #[cfg(not(unix))]
    {
        let probe = d.join(".emuera-tty-probe");
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }
}

/// Mark an installed copy as executable (nothing to do on Windows).
pub fn set_executable(_p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(_p, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Give freed memory back to the system where the allocator keeps it.
pub fn trim_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

pub use imp::*;

#[cfg(unix)]
mod imp {
    use super::{DateTime, Ready};

    /// Are stdin and stdout both terminals?
    pub fn is_tty() -> bool {
        unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
    }

    /// The terminal's size: (cols, rows, pixel width, pixel height), pixels
    /// 0 when unknown.
    pub fn winsize() -> Option<(usize, usize, u32, u32)> {
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
                Some((ws.ws_col as usize, ws.ws_row as usize, ws.ws_xpixel as u32, ws.ws_ypixel as u32))
            } else {
                None
            }
        }
    }

    /// The terminal in raw mode; [`RawMode::restore`] (or dropping it) puts
    /// it back.
    pub struct RawMode {
        orig: libc::termios,
    }

    impl RawMode {
        /// Raw input; output still turns `\n` into CR LF.
        pub fn enter() -> Result<RawMode, String> {
            Self::enter_with(false)
        }

        /// Raw mode; `keep_flow`: keep XON/XOFF flow control as it was
        /// (slow serial terminals need it).
        pub fn enter_with(keep_flow: bool) -> Result<RawMode, String> {
            if !is_tty() {
                return Err("stdin/stdout is not a terminal".into());
            }
            let mut orig: libc::termios = unsafe { std::mem::zeroed() };
            if unsafe { libc::tcgetattr(0, &mut orig) } != 0 {
                return Err("tcgetattr failed".into());
            }
            let mut raw = orig;
            unsafe { libc::cfmakeraw(&mut raw) };
            raw.c_oflag |= libc::OPOST | libc::ONLCR;
            if keep_flow {
                raw.c_iflag |= orig.c_iflag & (libc::IXON | libc::IXOFF);
            }
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) };
            Ok(RawMode { orig })
        }

        pub fn restore(&self) {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &self.orig);
            }
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            self.restore();
        }
    }

    /// Wait up to `timeout_ms` (-1: forever) for stdin or `extra` (a file
    /// descriptor, -1 for none).
    pub fn poll(timeout_ms: i32, extra: i32) -> Ready {
        let mut pfd = [libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: extra, events: libc::POLLIN, revents: 0 }];
        let r = unsafe { libc::poll(pfd.as_mut_ptr(), 2, timeout_ms) };
        if r <= 0 {
            return Ready::default();
        }
        Ready {
            input: pfd[0].revents & libc::POLLIN != 0,
            extra: pfd[1].revents & libc::POLLIN != 0,
            hangup: pfd[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 && pfd[0].revents & libc::POLLIN == 0,
        }
    }

    /// Read what stdin has: Ok(0) at the end of input; `Interrupted` /
    /// `WouldBlock` errors mean "try again".
    pub fn read_stdin(b: &mut [u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::read(0, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
        if n < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }

    /// Wait until everything written to stdout has gone out (so the next
    /// frame doesn't queue behind it on a slow terminal).
    pub fn drain_stdout() {
        unsafe {
            libc::tcdrain(1);
        }
    }

    pub fn now() -> DateTime {
        unsafe {
            let mut tv: libc::timeval = std::mem::zeroed();
            libc::gettimeofday(&mut tv, std::ptr::null_mut());
            let t: libc::time_t = tv.tv_sec;
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&t, &mut tm);
            DateTime {
                y: tm.tm_year as i64 + 1900,
                mo: tm.tm_mon as i64 + 1,
                d: tm.tm_mday as i64,
                h: tm.tm_hour as i64,
                mi: tm.tm_min as i64,
                s: tm.tm_sec as i64,
                ms: (tv.tv_usec / 1000) as i64,
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{DateTime, Ready};
    use std::collections::VecDeque;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::Console::*;

    fn handle(which: STD_HANDLE) -> HANDLE {
        unsafe { GetStdHandle(which) }
    }

    fn console_mode(h: HANDLE) -> Option<CONSOLE_MODE> {
        let mut m: CONSOLE_MODE = 0;
        (unsafe { GetConsoleMode(h, &mut m) } != 0).then_some(m)
    }

    /// Are stdin and stdout both consoles?
    pub fn is_tty() -> bool {
        console_mode(handle(STD_INPUT_HANDLE)).is_some() && console_mode(handle(STD_OUTPUT_HANDLE)).is_some()
    }

    /// The console window's size in cells (pixels unknown: 0).
    pub fn winsize() -> Option<(usize, usize, u32, u32)> {
        let mut i: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        if unsafe { GetConsoleScreenBufferInfo(handle(STD_OUTPUT_HANDLE), &mut i) } == 0 {
            return None;
        }
        let w = (i.srWindow.Right - i.srWindow.Left + 1).max(0) as usize;
        let h = (i.srWindow.Bottom - i.srWindow.Top + 1).max(0) as usize;
        (w > 0 && h > 0).then_some((w, h, 0, 0))
    }

    /// The console in VT raw mode; restored by [`RawMode::restore`] or drop.
    pub struct RawMode {
        orig_in: CONSOLE_MODE,
        orig_out: CONSOLE_MODE,
        orig_cp: (u32, u32),
    }

    impl RawMode {
        pub fn enter() -> Result<RawMode, String> {
            Self::enter_with(false)
        }

        /// Keys arrive as VT sequences (ENABLE_VIRTUAL_TERMINAL_INPUT, no
        /// line editing, echo or Ctrl-C handling, no quick-edit so the
        /// mouse reaches us); output interprets escape sequences and is
        /// UTF-8.
        pub fn enter_with(_keep_flow: bool) -> Result<RawMode, String> {
            let (hi, ho) = (handle(STD_INPUT_HANDLE), handle(STD_OUTPUT_HANDLE));
            let orig_in = console_mode(hi).ok_or("stdin is not a console")?;
            let orig_out = console_mode(ho).ok_or("stdout is not a console")?;
            let orig_cp = unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
            unsafe {
                if SetConsoleMode(ho, ENABLE_PROCESSED_OUTPUT | ENABLE_WRAP_AT_EOL_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
                    return Err("this console has no VT mode (needs Windows 10 or Windows Terminal)".into());
                }
                SetConsoleMode(hi, ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS);
                SetConsoleCP(65001);
                SetConsoleOutputCP(65001);
            }
            Ok(RawMode { orig_in, orig_out, orig_cp })
        }

        pub fn restore(&self) {
            unsafe {
                SetConsoleMode(handle(STD_INPUT_HANDLE), self.orig_in);
                SetConsoleMode(handle(STD_OUTPUT_HANDLE), self.orig_out);
                SetConsoleCP(self.orig_cp.0);
                SetConsoleOutputCP(self.orig_cp.1);
            }
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            self.restore();
        }
    }

    /// Console input can't be polled for bytes, so a thread reads it
    /// (blocking) and hands over UTF-8; `None` marks the end of input.
    struct Input {
        rx: Receiver<Option<Vec<u8>>>,
        buf: VecDeque<u8>,
        eof: bool,
    }

    static INPUT: OnceLock<Mutex<Input>> = OnceLock::new();

    fn input() -> &'static Mutex<Input> {
        INPUT.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let h = handle(STD_INPUT_HANDLE) as usize;
                let console = console_mode(h as HANDLE).is_some();
                let mut high: Option<u16> = None;
                loop {
                    let mut out = Vec::new();
                    if console {
                        let mut w = [0u16; 512];
                        let mut n = 0u32;
                        if unsafe { ReadConsoleW(h as HANDLE, w.as_mut_ptr() as *mut _, w.len() as u32, &mut n, std::ptr::null()) } == 0 {
                            let _ = tx.send(None);
                            return;
                        }
                        // UTF-16 to UTF-8, a surrogate pair may span reads
                        let mut units: Vec<u16> = high.take().into_iter().collect();
                        units.extend_from_slice(&w[..n as usize]);
                        if units.last().is_some_and(|&u| (0xD800..0xDC00).contains(&u)) {
                            high = units.pop();
                        }
                        for c in char::decode_utf16(units) {
                            let mut b = [0u8; 4];
                            out.extend_from_slice(c.unwrap_or('\u{FFFD}').encode_utf8(&mut b).as_bytes());
                        }
                    } else {
                        let mut b = [0u8; 4096];
                        let mut n = 0u32;
                        if unsafe { ReadFile(h as HANDLE, b.as_mut_ptr(), b.len() as u32, &mut n, std::ptr::null_mut()) } == 0 || n == 0 {
                            let _ = tx.send(None);
                            return;
                        }
                        out.extend_from_slice(&b[..n as usize]);
                    }
                    if !out.is_empty() && tx.send(Some(out)).is_err() {
                        return;
                    }
                }
            });
            Mutex::new(Input { rx, buf: VecDeque::new(), eof: false })
        })
    }

    /// Wait up to `timeout_ms` (-1: forever) for input. There is no Linux
    /// console mouse here: `extra` is ignored.
    pub fn poll(timeout_ms: i32, _extra: i32) -> Ready {
        let mut i = input().lock().unwrap();
        if !i.buf.is_empty() {
            return Ready { input: true, ..Ready::default() };
        }
        if i.eof {
            return Ready { hangup: true, ..Ready::default() };
        }
        let r = if timeout_ms < 0 { i.rx.recv().map_err(|_| RecvTimeoutError::Disconnected) } else { i.rx.recv_timeout(Duration::from_millis(timeout_ms as u64)) };
        match r {
            Ok(Some(b)) => {
                i.buf.extend(b);
                Ready { input: true, ..Ready::default() }
            }
            Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                i.eof = true;
                Ready { hangup: true, ..Ready::default() }
            }
            Err(RecvTimeoutError::Timeout) => Ready::default(),
        }
    }

    /// Bytes [`poll`] has collected; Ok(0) at the end of input.
    pub fn read_stdin(b: &mut [u8]) -> std::io::Result<usize> {
        let mut i = input().lock().unwrap();
        if i.buf.is_empty() {
            if i.eof {
                return Ok(0);
            }
            // nothing yet: take whatever the reader has without waiting
            while let Ok(m) = i.rx.try_recv() {
                match m {
                    Some(v) => i.buf.extend(v),
                    None => i.eof = true,
                }
            }
            if i.buf.is_empty() {
                return if i.eof { Ok(0) } else { Err(std::io::ErrorKind::WouldBlock.into()) };
            }
        }
        let n = b.len().min(i.buf.len());
        for (d, s) in b.iter_mut().zip(i.buf.drain(..n)) {
            *d = s;
        }
        Ok(n)
    }

    /// Console output is written synchronously.
    pub fn drain_stdout() {}

    pub fn now() -> DateTime {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut t = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        DateTime { y: t.wYear as i64, mo: t.wMonth as i64, d: t.wDay as i64, h: t.wHour as i64, mi: t.wMinute as i64, s: t.wSecond as i64, ms: t.wMilliseconds as i64 }
    }
}
