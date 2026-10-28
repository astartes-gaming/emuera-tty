//! Gamepads (Linux): the Steam Deck's controller (under Steam it is a
//! virtual Xbox pad), Xbox, PlayStation, 8BitDo and other pads, read from
//! their evdev devices.
//!
//! A thread finds them through sysfs (devices with a gamepad's south
//! button), looking again every two seconds so a pad plugged in later
//! works, and turns their buttons and sticks into a few [`Ev`]s: held
//! directions repeat, the left stick moves at a speed that grows with how
//! far it is pushed. The events go through a pipe that the frontends poll
//! next to stdin ([`fd`], [`read`]); what they do with them is theirs (a
//! pointer and clicks in a game, arrows and Enter in menus).
//!
//! Joystick devices are readable by the user at the seat (logind gives
//! them the `uaccess` tag), so nothing needs setting up.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What a pad did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    /// the d-pad: 0 up, 1 down, 2 left, 3 right (repeats while held)
    Dir(u8),
    /// the left stick: a move of the pointer, in cells
    Move(i8, i8),
    /// A (south): a click
    A,
    /// B (east): a right click
    B,
    /// X / Y (north / west, which drivers swap): Enter
    Enter,
    /// Start: the settings
    Settings,
    /// Select / Back: Esc
    Esc,
    /// LB / RB
    PageUp,
    PageDown,
    /// the right stick, up / down (repeats)
    ScrollUp,
    ScrollDown,
    /// LT / RT
    Home,
    End,
}

impl Ev {
    fn encode(self) -> [u8; 3] {
        match self {
            Ev::Dir(d) => [1, d, 0],
            Ev::Move(x, y) => [2, x as u8, y as u8],
            Ev::A => [3, 0, 0],
            Ev::B => [4, 0, 0],
            Ev::Enter => [5, 0, 0],
            Ev::Settings => [6, 0, 0],
            Ev::Esc => [7, 0, 0],
            Ev::PageUp => [8, 0, 0],
            Ev::PageDown => [9, 0, 0],
            Ev::ScrollUp => [10, 0, 0],
            Ev::ScrollDown => [11, 0, 0],
            Ev::Home => [12, 0, 0],
            Ev::End => [13, 0, 0],
        }
    }

    fn decode(b: &[u8]) -> Option<Ev> {
        Some(match b[0] {
            1 => Ev::Dir(b[1]),
            2 => Ev::Move(b[1] as i8, b[2] as i8),
            3 => Ev::A,
            4 => Ev::B,
            5 => Ev::Enter,
            6 => Ev::Settings,
            7 => Ev::Esc,
            8 => Ev::PageUp,
            9 => Ev::PageDown,
            10 => Ev::ScrollUp,
            11 => Ev::ScrollDown,
            12 => Ev::Home,
            13 => Ev::End,
            _ => return None,
        })
    }
}

/// The pipe's read end, once [`start`]ed (None: no pipe could be made).
static PIPE: OnceLock<Option<i32>> = OnceLock::new();
/// Bytes read from the pipe that don't make a whole event yet.
static PARTIAL: Mutex<Vec<u8>> = Mutex::new(Vec::new());
/// The pads in use, by name (and those that can't be opened, marked so).
static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Start looking for gamepads (once; later calls do nothing). Off with
/// `EMUERA_GAMEPAD=off`; `EMUERA_GAMEPAD=/dev/input/eventN` reads just that
/// device as a pad (also a FIFO of `input_event`s, for tests).
pub fn start() {
    if std::env::var("EMUERA_GAMEPAD").as_deref() == Ok("off") {
        return;
    }
    PIPE.get_or_init(|| {
        let mut p = [0i32; 2];
        if unsafe { libc::pipe(p.as_mut_ptr()) } != 0 {
            return None;
        }
        for f in p {
            unsafe {
                libc::fcntl(f, libc::F_SETFL, libc::fcntl(f, libc::F_GETFL) | libc::O_NONBLOCK);
                libc::fcntl(f, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }
        let w = p[1];
        std::thread::Builder::new().name("gamepad".into()).spawn(move || run(w)).ok()?;
        Some(p[0])
    });
}

/// The pipe to poll for gamepad events (-1 when not started).
pub fn fd() -> i32 {
    PIPE.get().copied().flatten().unwrap_or(-1)
}

/// The events waiting in the pipe.
pub fn read() -> Vec<Ev> {
    let f = fd();
    if f < 0 {
        return Vec::new();
    }
    let mut part = PARTIAL.lock().unwrap();
    let mut b = [0u8; 768];
    loop {
        let n = unsafe { libc::read(f, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
        if n <= 0 {
            break;
        }
        part.extend_from_slice(&b[..n as usize]);
    }
    let whole = part.len() / 3 * 3;
    let evs = part[..whole].chunks_exact(3).filter_map(Ev::decode).collect();
    part.drain(..whole);
    evs
}

/// The gamepads found, for the settings page ("none" when there are none).
pub fn describe() -> String {
    let n = NAMES.lock().unwrap();
    if n.is_empty() {
        "none".into()
    } else {
        n.join(", ")
    }
}

/// Bits per `long` in the kernel, as sysfs prints capability bitmaps in
/// its longs (a 32-bit program can run on a 64-bit kernel).
fn kernel_long_bits() -> u32 {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return usize::BITS;
    }
    let m: String = u.machine.iter().take_while(|&&c| c != 0).map(|&c| c as u8 as char).collect();
    if m.contains("64") || m == "s390x" || m == "alpha" {
        64
    } else {
        32
    }
}

/// A sysfs capability bitmap ("1f 0 7fff0000…", most significant long
/// first): is bit `bit` set?
fn has_bit(map: &str, bit: u32, long_bits: u32) -> bool {
    let words: Vec<&str> = map.split_whitespace().collect();
    let k = (bit / long_bits) as usize;
    k < words.len() && u64::from_str_radix(words[words.len() - 1 - k], 16).is_ok_and(|w| w >> (bit % long_bits) & 1 == 1)
}

const BTN_SOUTH: u16 = 0x130;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

/// `_IOR(ty, nr, size)` (powerpc, MIPS, SPARC and Alpha put the direction
/// bits a bit lower).
fn ior(ty: u8, nr: u8, size: usize) -> u64 {
    let shift = if cfg!(any(
        target_arch = "powerpc",
        target_arch = "powerpc64",
        target_arch = "mips",
        target_arch = "mips64",
        target_arch = "sparc",
        target_arch = "sparc64"
    )) {
        29
    } else {
        30
    };
    (2u64 << shift) | ((size as u64) << 16) | ((ty as u64) << 8) | nr as u64
}

/// An axis's (minimum, maximum), by EVIOCGABS.
fn abs_range(fd: i32, code: u16) -> Option<(i32, i32)> {
    // struct input_absinfo: value, minimum, maximum, fuzz, flat, resolution
    let mut info = [0i32; 6];
    let req = ior(b'E', 0x40 + code as u8, std::mem::size_of_val(&info));
    let r = unsafe { libc::ioctl(fd, req as _, info.as_mut_ptr()) };
    (r >= 0 && info[2] > info[1]).then_some((info[1], info[2]))
}

/// An open pad and what its controls are doing.
struct Pad {
    file: std::fs::File,
    path: PathBuf,
    /// its name (sysfs `device/name`)
    label: String,
    /// axis → (min, max)
    range: HashMap<u16, (i32, i32)>,
    /// ABS_Z / ABS_RZ are the triggers (pads with ABS_RX); on plain HID
    /// pads they are the right stick
    z_triggers: bool,
    dpad: (i8, i8),
    /// d-pad buttons held (pads that have BTN_DPAD_* rather than a hat)
    dbtn: [bool; 4],
    left: (f32, f32),
    right: (f32, f32),
    triggers: [bool; 2],
}

impl Pad {
    fn new(file: std::fs::File, path: PathBuf, label: String, range: HashMap<u16, (i32, i32)>, z_triggers: bool) -> Pad {
        Pad { file, path, label, range, z_triggers, dpad: (0, 0), dbtn: [false; 4], left: (0.0, 0.0), right: (0.0, 0.0), triggers: [false; 2] }
    }

    /// An axis's value as -1…1 (sticks) from its range.
    fn norm(&self, code: u16, v: i32) -> f32 {
        let (lo, hi) = self.range.get(&code).copied().unwrap_or((-32768, 32767));
        ((v - lo) as f32 / (hi - lo).max(1) as f32 * 2.0 - 1.0).clamp(-1.0, 1.0)
    }

    /// One evdev event; button presses come out as events at once.
    fn event(&mut self, ty: u16, code: u16, value: i32, out: &mut Vec<Ev>) {
        const EV_KEY: u16 = 1;
        const EV_ABS: u16 = 3;
        match ty {
            EV_KEY => {
                let ev = match code {
                    0x130 => Some(Ev::A),
                    0x131 => Some(Ev::B),
                    0x133 | 0x134 => Some(Ev::Enter),
                    0x136 => Some(Ev::PageUp),
                    0x137 => Some(Ev::PageDown),
                    0x138 => Some(Ev::Home),
                    0x139 => Some(Ev::End),
                    0x13a => Some(Ev::Esc),
                    0x13b => Some(Ev::Settings),
                    0x220..=0x223 => {
                        self.dbtn[(code - 0x220) as usize] = value != 0;
                        let b = self.dbtn;
                        self.dpad = (b[3] as i8 - b[2] as i8, b[1] as i8 - b[0] as i8);
                        None
                    }
                    _ => None,
                };
                // presses only (1), not releases (0) or the kernel's repeats (2)
                if value == 1 {
                    out.extend(ev);
                }
            }
            EV_ABS => match code {
                ABS_HAT0X => self.dpad.0 = value.signum() as i8,
                ABS_HAT0Y => self.dpad.1 = value.signum() as i8,
                ABS_X => self.left.0 = self.norm(code, value),
                ABS_Y => self.left.1 = self.norm(code, value),
                ABS_RX => self.right.0 = self.norm(code, value),
                ABS_RY => self.right.1 = self.norm(code, value),
                ABS_Z | ABS_RZ if self.z_triggers => {
                    // 0…1: a press past half way, released under a third
                    let t = (self.norm(code, value) + 1.0) / 2.0;
                    let k = (code == ABS_RZ) as usize;
                    if !self.triggers[k] && t > 0.5 {
                        self.triggers[k] = true;
                        out.push(if k == 0 { Ev::Home } else { Ev::End });
                    } else if self.triggers[k] && t < 0.33 {
                        self.triggers[k] = false;
                    }
                }
                ABS_Z => self.right.0 = self.norm(code, value),
                ABS_RZ => self.right.1 = self.norm(code, value),
                _ => {}
            },
            _ => {}
        }
    }
}

/// Open the gamepads not open yet (and forget devices that are gone).
/// `unreadable`: pads that can't be opened (no permission), with why.
fn scan(pads: &mut Vec<Pad>, checked: &mut HashSet<PathBuf>, unreadable: &mut HashMap<PathBuf, String>, long_bits: u32) {
    if let Some(dev) = std::env::var_os("EMUERA_GAMEPAD").map(PathBuf::from).filter(|p| p.is_absolute()) {
        if checked.insert(dev.clone()) {
            use std::os::unix::fs::OpenOptionsExt;
            match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(&dev) {
                Ok(file) => {
                    let label = dev.display().to_string();
                    pads.push(Pad::new(file, dev, label, HashMap::new(), true));
                }
                Err(err) => {
                    unreadable.insert(dev.clone(), format!("{} ({})", dev.display(), err.kind()));
                }
            }
        }
        return;
    }
    let Ok(dir) = std::fs::read_dir("/sys/class/input") else { return };
    let mut seen = HashSet::new();
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with("event") {
            continue;
        }
        let dev = PathBuf::from("/dev/input").join(&name);
        seen.insert(dev.clone());
        if !checked.insert(dev.clone()) {
            continue;
        }
        let sys = e.path().join("device");
        let keys = std::fs::read_to_string(sys.join("capabilities/key")).unwrap_or_default();
        if !has_bit(&keys, BTN_SOUTH as u32, long_bits) {
            continue;
        }
        let label = std::fs::read_to_string(sys.join("name")).map(|s| s.trim().to_string()).unwrap_or_else(|_| name.clone());
        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(&dev) {
            Ok(f) => f,
            Err(err) => {
                unreadable.insert(dev.clone(), format!("{} (can't read {}: {})", label, dev.display(), err.kind()));
                continue;
            }
        };
        let abs = std::fs::read_to_string(sys.join("capabilities/abs")).unwrap_or_default();
        let has_abs = |c: u16| has_bit(&abs, c as u32, long_bits);
        let mut range = HashMap::new();
        for c in [ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ] {
            if has_abs(c) {
                if let Some(r) = abs_range(file.as_raw_fd(), c) {
                    range.insert(c, r);
                }
            }
        }
        pads.push(Pad::new(file, dev, label, range, has_abs(ABS_RX)));
    }
    // a device number that comes back is another device: look at it again
    checked.retain(|p| seen.contains(p));
    unreadable.retain(|p, _| seen.contains(p));
}

/// How far past the dead zone a stick is pushed, 0…1 (sticks drift a
/// little, the Steam Deck's too).
fn past_dead(v: f32) -> f32 {
    const DEAD: f32 = 0.25;
    (v.abs() - DEAD).max(0.0) / (1.0 - DEAD) * v.signum()
}

/// The thread: read the pads, write events to the pipe's write end `w`.
fn run(w: i32) {
    let long_bits = kernel_long_bits();
    let mut pads: Vec<Pad> = Vec::new();
    let mut checked = HashSet::new();
    let mut unreadable = HashMap::new();
    let mut last_scan: Option<Instant> = None;
    // the d-pad direction held and when it repeats
    let mut held: Option<(u8, Instant)> = None;
    // the right stick's scrolling: direction and when it repeats
    let mut scroll: Option<(bool, Instant)> = None;
    // the left stick's moves: cells not given yet, and the last tick
    let mut acc = (0.0f32, 0.0f32);
    let mut tick = Instant::now();
    let ev_size = 2 * std::mem::size_of::<libc::c_ulong>() + 8;
    let mut buf = vec![0u8; ev_size * 64];
    loop {
        if last_scan.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
            scan(&mut pads, &mut checked, &mut unreadable, long_bits);
            last_scan = Some(Instant::now());
            *NAMES.lock().unwrap() = pads.iter().map(|p| p.label.clone()).chain(unreadable.values().cloned()).collect();
        }
        let busy = held.is_some() || scroll.is_some() || pads.iter().any(|p| past_dead(p.left.0) != 0.0 || past_dead(p.left.1) != 0.0);
        let mut pfd: Vec<libc::pollfd> = pads.iter().map(|p| libc::pollfd { fd: p.file.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
        let timeout = if busy { 16 } else { 2000 };
        unsafe { libc::poll(pfd.as_mut_ptr(), pfd.len() as _, timeout) };
        let mut out = Vec::new();
        let mut gone = Vec::new();
        for (i, p) in pads.iter_mut().enumerate() {
            if pfd[i].revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 && pfd[i].revents & libc::POLLIN == 0 {
                gone.push(i);
                continue;
            }
            if pfd[i].revents & libc::POLLIN == 0 {
                continue;
            }
            loop {
                match p.file.read(&mut buf) {
                    Ok(0) => {
                        gone.push(i);
                        break;
                    }
                    Ok(n) => {
                        // struct input_event: struct timeval (two longs),
                        // u16 type, u16 code, i32 value
                        for e in buf[..n].chunks_exact(ev_size) {
                            let t = ev_size - 8;
                            let ty = u16::from_ne_bytes([e[t], e[t + 1]]);
                            let code = u16::from_ne_bytes([e[t + 2], e[t + 3]]);
                            let value = i32::from_ne_bytes([e[t + 4], e[t + 5], e[t + 6], e[t + 7]]);
                            p.event(ty, code, value, &mut out);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        gone.push(i);
                        break;
                    }
                }
            }
        }
        for &i in gone.iter().rev() {
            let p = pads.remove(i);
            checked.remove(&p.path);
            last_scan = None;
        }
        let now = Instant::now();
        // the d-pad: a press moves at once, held it repeats
        let dir = pads.iter().map(|p| p.dpad).find(|d| *d != (0, 0)).map(|(x, y)| match (x, y) {
            (_, -1) => 0,
            (_, 1) => 1,
            (-1, _) => 2,
            _ => 3,
        });
        held = match (dir, held) {
            (None, _) => None,
            (Some(d), Some((h, at))) if d == h => {
                if now >= at {
                    out.push(Ev::Dir(d));
                    Some((d, now + Duration::from_millis(110)))
                } else {
                    Some((h, at))
                }
            }
            (Some(d), _) => {
                out.push(Ev::Dir(d));
                Some((d, now + Duration::from_millis(350)))
            }
        };
        // the right stick scrolls, faster the further it is pushed
        let ry = pads.iter().map(|p| past_dead(p.right.1)).find(|v| v.abs() > 0.3).unwrap_or(0.0);
        scroll = if ry == 0.0 {
            None
        } else {
            let up = ry < 0.0;
            let every = Duration::from_millis((220.0 - 180.0 * ry.abs()) as u64);
            match scroll {
                Some((u, at)) if u == up && now < at => Some((u, at)),
                _ => {
                    out.push(if up { Ev::ScrollUp } else { Ev::ScrollDown });
                    Some((up, now + every))
                }
            }
        };
        // the left stick moves the pointer: up to 60 columns / 30 rows a
        // second, slowly near the middle (squared)
        let dt = now.duration_since(tick).as_secs_f32().min(0.1);
        tick = now;
        let (lx, ly) = pads.iter().map(|p| (past_dead(p.left.0), past_dead(p.left.1))).find(|l| *l != (0.0, 0.0)).unwrap_or((0.0, 0.0));
        if (lx, ly) == (0.0, 0.0) {
            acc = (0.0, 0.0);
        } else {
            // (a push starts with a cell, so small moves are easy)
            let start = |a: f32, v: f32| if a == 0.0 && v != 0.0 { 0.99 * v.signum() } else { a };
            acc = (start(acc.0, lx) + lx * lx.abs() * 60.0 * dt, start(acc.1, ly) + ly * ly.abs() * 30.0 * dt);
            let (mx, my) = (acc.0.trunc(), acc.1.trunc());
            if mx != 0.0 || my != 0.0 {
                acc = (acc.0 - mx, acc.1 - my);
                out.push(Ev::Move(mx.clamp(-100.0, 100.0) as i8, my.clamp(-100.0, 100.0) as i8));
            }
        }
        if !out.is_empty() {
            let bytes: Vec<u8> = out.iter().flat_map(|e| e.encode()).collect();
            // (a full pipe, while nobody reads: the events are dropped)
            unsafe { libc::write(w, bytes.as_ptr() as *const libc::c_void, bytes.len()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmaps() {
        // a pad's keys on a 64-bit kernel: BTN_SOUTH is 0x130 = 304
        let m = "7fdb000000000000 0 0 0 0";
        assert!(has_bit(m, 0x130, 64) && has_bit(m, 0x13b, 64) && !has_bit(m, 0x132, 64) && !has_bit(m, 1, 64));
        // a keyboard's: no BTN_SOUTH
        assert!(!has_bit("120013 803078f800d001 feffffdfffefffff fffffffffffffffe", 0x130, 64));
        // 32-bit longs: bit 0x130 is in the 10th word from the end
        let mut w = vec!["0"; 10];
        w[0] = "10000";
        assert!(has_bit(&w.join(" "), 0x130, 32));
    }

    #[test]
    fn events_round_trip() {
        for e in [Ev::Dir(3), Ev::Move(-5, 7), Ev::A, Ev::B, Ev::Enter, Ev::Settings, Ev::Esc, Ev::PageUp, Ev::ScrollDown, Ev::End] {
            assert_eq!(Ev::decode(&e.encode()), Some(e));
        }
    }

    #[test]
    fn sticks_and_triggers() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let mut range = HashMap::new();
        range.insert(ABS_X, (0, 255));
        let mut p = Pad::new(file, PathBuf::new(), String::new(), range, true);
        let mut out = Vec::new();
        p.event(3, ABS_X, 255, &mut out);
        p.event(3, ABS_Y, -32768, &mut out);
        assert_eq!(p.left, (1.0, -1.0));
        // RT (ABS_RZ, 0…255 on xpad: no range known here, s16 assumed)
        p.event(3, ABS_RZ, 30000, &mut out);
        p.event(3, ABS_RZ, 31000, &mut out);
        assert_eq!(out, vec![Ev::End]);
        p.event(1, 0x221, 1, &mut out);
        assert_eq!(p.dpad, (0, 1));
        p.event(1, 0x130, 1, &mut out);
        p.event(1, 0x130, 0, &mut out);
        assert_eq!(out, vec![Ev::End, Ev::A]);
    }
}
