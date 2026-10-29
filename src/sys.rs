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
    /// the second extra one (the gamepads' pipe) is readable
    pub extra2: bool,
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
    #[cfg(all(any(target_os = "linux", target_os = "hurd"), target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

/// A memory budget in bytes: `env` (in MB) if set, else the machine's
/// memory / `div` (no limit where that is unknown).
pub fn memory_budget(env: &str, div: u64) -> usize {
    match std::env::var(env).ok().and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(mb) => mb << 20,
        None => total_memory().map_or(usize::MAX, |m| (m / div) as usize),
    }
}

/// The machine's memory in bytes, where it can be read.
pub fn total_memory() -> Option<u64> {
    // the Hurd's procfs translator has Linux's meminfo and statm
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "hurd"))]
    {
        let s = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kb: u64 = s.lines().find(|l| l.starts_with("MemTotal:"))?.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut m) != 0).then_some(m.ullTotalPhys)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "hurd", windows)))]
    None
}

/// The most memory [`memory_in_use`] has reported this session.
static MEMORY_PEAK: crate::atomic64::AtomicU64 = crate::atomic64::AtomicU64::new(0);

/// The highest memory use seen so far this session (as recorded by the
/// readings [`memory_in_use`] has made; 0 before the first).
pub fn memory_peak() -> u64 {
    MEMORY_PEAK.load(std::sync::atomic::Ordering::Relaxed)
}

/// The memory this process uses now (resident), where it can be read.
/// Every reading also updates [`memory_peak`].
pub fn memory_in_use() -> Option<u64> {
    let now = memory_in_use_raw()?;
    if now > memory_peak() {
        MEMORY_PEAK.store(now, std::sync::atomic::Ordering::Relaxed);
    }
    Some(now)
}

/// The colour band the live memory counter is shown in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemoryLevel {
    Green,
    Yellow,
    Orange,
    Red,
}

/// The band for a reading, by the whole MB that is shown: up to 450 MB
/// green, up to 650 yellow, under 900 orange, 900 and over red.
pub fn memory_level(now: u64) -> MemoryLevel {
    match now >> 20 {
        0..=450 => MemoryLevel::Green,
        451..=650 => MemoryLevel::Yellow,
        651..=899 => MemoryLevel::Orange,
        _ => MemoryLevel::Red,
    }
}

/// A reading as its two texts: the live counter ("123 MB") and the peak
/// ("" while it is the same MB, else " (peak 150 MB)"). Only the live
/// counter is coloured by [`memory_level`].
pub fn memory_parts(now: u64) -> (String, String) {
    let (n, p) = (now >> 20, memory_peak() >> 20);
    let peak = if p > n { format!(" (peak {} MB)", p) } else { String::new() };
    (format!("{} MB", n), peak)
}

/// "123 MB (peak 150 MB)" for a reading; the peak is left out while it is
/// the same MB.
pub fn memory_text(now: u64) -> String {
    let (live, peak) = memory_parts(now);
    live + &peak
}

fn memory_in_use_raw() -> Option<u64> {
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "hurd"))]
    {
        let s = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        Some(pages * if page > 0 { page as u64 } else { 4096 })
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        (GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0).then_some(c.WorkingSetSize as u64)
    }
    #[cfg(target_os = "haiku")]
    unsafe {
        // the RAM held by this team's areas (heap, stacks, mapped files)
        let (mut cookie, mut info, mut sum) = (0isize, std::mem::zeroed::<libc::area_info>(), 0u64);
        while libc::get_next_area_info(0, &mut cookie, &mut info) == libc::B_OK {
            sum += info.ram_size as u64;
        }
        (sum > 0).then_some(sum)
    }
    // the BSDs: the process's resident pages from sysctl
    #[cfg(any(target_os = "freebsd", target_os = "dragonfly", target_os = "netbsd", target_os = "openbsd"))]
    unsafe {
        let page = libc::sysconf(libc::_SC_PAGESIZE).max(4096) as u64;
        // filled by the kernel and read one field at a time: DragonFly's
        // kinfo_proc has fields that may not be zero, so mem::zeroed panics
        #[cfg(target_os = "netbsd")]
        let (mut kp, mib) = (std::mem::MaybeUninit::<libc::kinfo_proc2>::zeroed(), libc::KERN_PROC2);
        #[cfg(not(target_os = "netbsd"))]
        let (mut kp, mib) = (std::mem::MaybeUninit::<libc::kinfo_proc>::zeroed(), libc::KERN_PROC);
        let size = std::mem::size_of_val(&kp);
        let mut len = size;
        // NetBSD and OpenBSD take the record size and count in the name
        let mib = [libc::CTL_KERN, mib, libc::KERN_PROC_PID, libc::getpid(), size as libc::c_int, 1];
        let n = if cfg!(any(target_os = "netbsd", target_os = "openbsd")) { 6 } else { 4 };
        if libc::sysctl(mib.as_ptr(), n, kp.as_mut_ptr() as *mut libc::c_void, &mut len, std::ptr::null_mut(), 0) != 0 || len == 0 {
            return None;
        }
        let kp = kp.as_ptr();
        #[cfg(target_os = "freebsd")]
        let pages = (*kp).ki_rssize as u64;
        #[cfg(target_os = "dragonfly")]
        let pages = (*kp).kp_vm_rssize as u64;
        #[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
        let pages = (*kp).p_vm_rssize as u64;
        (pages > 0).then_some(pages * page)
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    unsafe {
        let mut t: libc::proc_taskinfo = std::mem::zeroed();
        let size = std::mem::size_of_val(&t) as libc::c_int;
        (libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, &mut t as *mut _ as *mut libc::c_void, size) == size).then_some(t.pti_resident_size)
    }
    // illumos / Solaris: psinfo_t's pr_rssize (KB), after ten 4-byte ids
    // and two pointer-sized fields
    #[cfg(any(target_os = "illumos", target_os = "solaris"))]
    {
        let b = std::fs::read("/proc/self/psinfo").ok()?;
        let at = 40 + 2 * std::mem::size_of::<usize>();
        let kb = usize::from_ne_bytes(b.get(at..at + std::mem::size_of::<usize>())?.try_into().ok()?);
        (kb > 0).then_some(kb as u64 * 1024)
    }
    // Redox: this process's private memory from the kernel's context list,
    // the PRIVATE column ps shows ("7.48 MB": number and unit); its
    // getrusage leaves ru_maxrss at 0
    #[cfg(target_os = "redox")]
    {
        let pid = std::process::id().to_string();
        let list = std::fs::read_to_string("/scheme/sys/context").ok()?;
        list.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() < 10 || f[0] != pid {
                    return None;
                }
                let unit = match f[9] {
                    "B" => 1.0,
                    "KB" => 1024.0,
                    "MB" => 1024.0 * 1024.0,
                    "GB" => 1024.0 * 1024.0 * 1024.0,
                    _ => return None,
                };
                Some((f[8].parse::<f64>().ok()? * unit) as u64)
            })
            .max()
            .filter(|&b| b > 0)
    }
    // other Unixes: the peak resident size (KB), the nearest a portable call
    // gives
    #[cfg(all(
        unix,
        not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "hurd",
            target_os = "haiku",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "macos",
            target_os = "ios",
            target_os = "illumos",
            target_os = "solaris",
            target_os = "redox"
        ))
    ))]
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        (libc::getrusage(libc::RUSAGE_SELF, &mut u) == 0 && u.ru_maxrss > 0).then(|| u.ru_maxrss as u64 * 1024)
    }
    #[cfg(not(any(unix, windows)))]
    None
}

/// The characters the Linux console's current font can show (its Unicode
/// map, `GIO_UNIMAP` on the terminal, as `showconsolefont` reads it); None
/// elsewhere or when it can't be read. Others show as a diamond there.
pub fn console_font_chars() -> Option<std::collections::HashSet<char>> {
    #[cfg(target_os = "linux")]
    unsafe {
        #[repr(C)]
        struct UniPair {
            unicode: u16,
            fontpos: u16,
        }
        #[repr(C)]
        struct UniMapDesc {
            entry_ct: u16,
            entries: *mut UniPair,
        }
        const GIO_UNIMAP: libc::c_ulong = 0x4B66;
        let mut buf: Vec<UniPair> = Vec::with_capacity(65535);
        let mut d = UniMapDesc { entry_ct: 65535, entries: buf.as_mut_ptr() };
        if libc::ioctl(1, GIO_UNIMAP as _, &mut d) != 0 {
            return None;
        }
        buf.set_len((d.entry_ct as usize).min(65535));
        let set: std::collections::HashSet<char> = buf.iter().filter_map(|p| char::from_u32(p.unicode as u32)).collect();
        return (!set.is_empty()).then_some(set);
    }
    #[allow(unreachable_code)]
    None
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

        /// Re-enter raw mode without changing the stored original termios.
        /// Used when temporarily suspended (e.g., for debugger input).
        pub fn re_enter(&self) -> Result<(), String> {
            let mut raw = self.orig;
            unsafe { libc::cfmakeraw(&mut raw) };
            raw.c_oflag |= libc::OPOST | libc::ONLCR;
            unsafe { libc::tcsetattr(0, libc::TCSANOW, &raw) };
            Ok(())
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

    /// [`poll`] watching two extra file descriptors (the console mouse
    /// and the gamepads), -1 for none.
    pub fn poll2(timeout_ms: i32, extra: i32, extra2: i32) -> Ready {
        if extra2 < 0 {
            return poll_fd(0, timeout_ms, extra);
        }
        #[cfg(target_os = "macos")]
        unsafe {
            let mut set: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut set);
            libc::FD_SET(0, &mut set);
            for f in [extra, extra2] {
                if f >= 0 {
                    libc::FD_SET(f, &mut set);
                }
            }
            let mut tv = libc::timeval { tv_sec: (timeout_ms.max(0) / 1000) as _, tv_usec: ((timeout_ms.max(0) % 1000) * 1000) as _ };
            let tvp = if timeout_ms < 0 { std::ptr::null_mut() } else { &mut tv as *mut _ };
            if libc::select(extra.max(extra2) + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), tvp) <= 0 {
                return Ready::default();
            }
            return Ready {
                input: libc::FD_ISSET(0, &set),
                extra: extra >= 0 && libc::FD_ISSET(extra, &set),
                extra2: libc::FD_ISSET(extra2, &set),
                hangup: false,
            };
        }
        #[allow(unreachable_code)]
        let mut pfd = [
            libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: extra, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: extra2, events: libc::POLLIN, revents: 0 },
        ];
        let r = unsafe { libc::poll(pfd.as_mut_ptr(), 3, timeout_ms) };
        if r <= 0 {
            return Ready::default();
        }
        Ready {
            input: pfd[0].revents & libc::POLLIN != 0,
            extra: pfd[1].revents & libc::POLLIN != 0,
            extra2: pfd[2].revents & libc::POLLIN != 0,
            hangup: pfd[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 && pfd[0].revents & libc::POLLIN == 0,
        }
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
            return Ready { input: libc::FD_ISSET(fd, &set), extra: extra >= 0 && libc::FD_ISSET(extra, &set), extra2: false, hangup: false };
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
            extra2: false,
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
            let t = tv.tv_sec;
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

        /// Re-enter raw mode without changing the stored original termios.
        /// Used when temporarily suspended (e.g., for debugger input).
        pub fn re_enter(&self) -> Result<(), String> {
            let (hi, ho) = (handle(STD_INPUT_HANDLE), handle(STD_OUTPUT_HANDLE));
            unsafe {
                if SetConsoleMode(ho, ENABLE_PROCESSED_OUTPUT | ENABLE_WRAP_AT_EOL_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING) == 0 {
                    return Err("this console has no VT mode (needs Windows 10 or Windows Terminal)".into());
                }
                SetConsoleMode(hi, ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT | ENABLE_EXTENDED_FLAGS);
                SetConsoleCP(65001);
                SetConsoleOutputCP(65001);
            }
            Ok(())
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
                if r.EventType as u32 != KEY_EVENT {
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

    /// [`poll`]: no console mouse or gamepads here either.
    pub fn poll2(timeout_ms: i32, _extra: i32, _extra2: i32) -> Ready {
        poll(timeout_ms, -1)
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
        DateTime {
            y: t.wYear as i64,
            mo: t.wMonth as i64,
            d: t.wDay as i64,
            h: t.wHour as i64,
            mi: t.wMinute as i64,
            s: t.wSecond as i64,
            ms: t.wMilliseconds as i64,
        }
    }
}

#[cfg(test)]
mod memory_level_tests {
    use super::*;

    #[test]
    fn bands_follow_the_mb_shown() {
        let mb = |n: u64| (n << 20) + 1000;
        assert_eq!(memory_level(mb(0)), MemoryLevel::Green);
        assert_eq!(memory_level(mb(450)), MemoryLevel::Green);
        assert_eq!(memory_level(mb(451)), MemoryLevel::Yellow);
        assert_eq!(memory_level(mb(650)), MemoryLevel::Yellow);
        assert_eq!(memory_level(mb(651)), MemoryLevel::Orange);
        assert_eq!(memory_level(mb(899)), MemoryLevel::Orange);
        assert_eq!(memory_level(mb(900)), MemoryLevel::Red);
        assert_eq!(memory_level(mb(4000)), MemoryLevel::Red);
    }
}
