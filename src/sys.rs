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
        poll_fd(0, timeout_ms, extra)
    }

    /// [`poll`] for any file descriptor `fd` (a serial line) in place of
    /// stdin.
    pub fn poll_fd(fd: i32, timeout_ms: i32, extra: i32) -> Ready {
        // macOS's poll() doesn't handle terminal devices everywhere:
        // select() does (the end of input shows up as a 0-byte read)
        #[cfg(target_os = "macos")]
        unsafe {
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut set);
            libc::FD_SET(fd, &mut set);
            if extra >= 0 {
                libc::FD_SET(extra, &mut set);
            }
            let mut tv = libc::timeval { tv_sec: (timeout_ms.max(0) / 1000) as _, tv_usec: ((timeout_ms.max(0) % 1000) * 1000) as _ };
            let tvp = if timeout_ms < 0 { std::ptr::null_mut() } else { &mut tv as *mut _ };
            if libc::select(extra.max(fd) + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), tvp) <= 0 {
                return Ready::default();
            }
            return Ready { input: libc::FD_ISSET(fd, &set), extra: extra >= 0 && libc::FD_ISSET(extra, &set), hangup: false };
        }
        #[allow(unreachable_code)]
        let mut pfd = [libc::pollfd { fd, events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: extra, events: libc::POLLIN, revents: 0 }];
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
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::HANDLE;
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

    /// Console input as UTF-8 bytes: with VT input on, every key, mouse
    /// report and query reply arrives as key-down records carrying the
    /// characters of its escape sequence. No thread reads ahead, so a
    /// program we start in our place gets the console's input to itself.
    struct Input {
        buf: VecDeque<u8>,
        /// first half of a surrogate pair
        high: Option<u16>,
    }

    static INPUT: Mutex<Input> = Mutex::new(Input { buf: VecDeque::new(), high: None });

    /// Move the console's pending key characters into `i.buf`; false if the
    /// console is gone.
    fn drain_events(i: &mut Input) -> bool {
        let h = handle(STD_INPUT_HANDLE);
        loop {
            let mut n = 0u32;
            if unsafe { GetNumberOfConsoleInputEvents(h, &mut n) } == 0 {
                return false;
            }
            if n == 0 {
                return true;
            }
            let mut recs: [INPUT_RECORD; 64] = unsafe { std::mem::zeroed() };
            let mut got = 0u32;
            if unsafe { ReadConsoleInputW(h, recs.as_mut_ptr(), recs.len() as u32, &mut got) } == 0 {
                return false;
            }
            for r in &recs[..got as usize] {
                if r.EventType as u32 != KEY_EVENT as u32 {
                    continue;
                }
                let k = unsafe { r.Event.KeyEvent };
                let u = unsafe { k.uChar.UnicodeChar };
                if k.bKeyDown == 0 || u == 0 {
                    continue;
                }
                for _ in 0..k.wRepeatCount.max(1) {
                    let units: Vec<u16> = match i.high.take() {
                        Some(hi) => vec![hi, u],
                        None if (0xD800..0xDC00).contains(&u) => {
                            i.high = Some(u);
                            continue;
                        }
                        None => vec![u],
                    };
                    for c in char::decode_utf16(units) {
                        let mut b = [0u8; 4];
                        i.buf.extend(c.unwrap_or('\u{FFFD}').encode_utf8(&mut b).as_bytes());
                    }
                }
            }
        }
    }

    /// Wait up to `timeout_ms` (-1: forever) for input. There is no Linux
    /// console mouse here: `extra` is ignored.
    pub fn poll(timeout_ms: i32, _extra: i32) -> Ready {
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let start = std::time::Instant::now();
        let mut i = INPUT.lock().unwrap();
        loop {
            if !drain_events(&mut i) {
                return Ready { hangup: true, ..Ready::default() };
            }
            if !i.buf.is_empty() {
                return Ready { input: true, ..Ready::default() };
            }
            // other records (focus, resize, key releases) wake us too
            let left = if timeout_ms < 0 { u32::MAX } else { (timeout_ms as u128).saturating_sub(start.elapsed().as_millis()) as u32 };
            if timeout_ms >= 0 && left == 0 {
                return Ready::default();
            }
            if unsafe { WaitForSingleObject(handle(STD_INPUT_HANDLE), left) } != WAIT_OBJECT_0 {
                return Ready::default();
            }
        }
    }

    /// Bytes [`poll`] has collected (Ok(0): the console is gone).
    pub fn read_stdin(b: &mut [u8]) -> std::io::Result<usize> {
        let mut i = INPUT.lock().unwrap();
        if i.buf.is_empty() {
            if !drain_events(&mut i) {
                return Ok(0);
            }
            if i.buf.is_empty() {
                return Err(std::io::ErrorKind::WouldBlock.into());
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
