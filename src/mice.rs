//! Pointer input on the Linux text console (fbcon / fbterm), where the
//! terminal itself sends no mouse reports. Two sources, both turned into
//! the same events the terminal mouse protocols produce:
//!
//! - **gpm**: connect to its socket (`/dev/gpmctl`) as a client, like mc
//!   or links do, and get moves, clicks and the wheel in cells. Needs
//!   nothing but a running gpm.
//! - **`/dev/input/mice`**: relative motion (ImPS/2, so the wheel works),
//!   tracked into a pointer we draw ourselves. Needs read access to the
//!   device (root or the `input` group).

use std::ffi::CString;

/// An event in xterm/SGR terms: button code (0 left, 1 middle, 2 right,
/// +32 motion, 64/65 wheel up/down), 1-based cell, press or release.
pub type Event = (u16, u16, u16, bool);

// Gpm_Event types
const GPM_MOVE: u16 = 1;
const GPM_DRAG: u16 = 2;
const GPM_DOWN: u16 = 4;
const GPM_UP: u16 = 8;

pub struct Mice {
    fd: i32,
    /// a gpm client connection rather than the raw device
    gpm: bool,
    wheel: bool,
    buf: Vec<u8>,
    /// pointer position in pixels
    px: (i32, i32),
    buttons: u8,
    last_cell: (u16, u16),
}

impl Mice {
    /// Does gpm draw the pointer? (the raw device has none of its own)
    pub fn draws_pointer(&self) -> bool {
        self.gpm
    }

    /// Use the console mouse (gpm or the raw device)? On a VT;
    /// `EMUERA_MOUSE=mice` forces it, `=off` / `=term` disables it.
    pub fn wanted(is_vt: bool) -> bool {
        match std::env::var("EMUERA_MOUSE").as_deref() {
            Ok("off") | Ok("term") => false,
            Ok("mice") => true,
            _ => is_vt,
        }
    }

    pub fn open() -> Result<Mice, String> {
        let path = CString::new("/dev/input/mice").unwrap();
        let mut fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        let writable = fd >= 0;
        if fd < 0 {
            fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        }
        if fd < 0 {
            return Err(format!("/dev/input/mice: {}", std::io::Error::last_os_error()));
        }
        let mut m = Mice { fd, gpm: false, wheel: false, buf: Vec::new(), px: (0, 0), buttons: 0, last_cell: (1, 1) };
        if writable {
            m.wheel = m.enable_wheel();
        }
        Ok(m)
    }

    /// ImPS/2 "magic knock" (sample rate 200, 100, 80), then ask for the
    /// device id: 3 means 4-byte packets with a wheel byte.
    fn enable_wheel(&mut self) -> bool {
        let knock = [0xf3u8, 200, 0xf3, 100, 0xf3, 80, 0xf2];
        if unsafe { libc::write(self.fd, knock.as_ptr() as *const libc::c_void, knock.len()) } != knock.len() as isize {
            return false;
        }
        let mut reply = Vec::new();
        for _ in 0..10 {
            let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
            if unsafe { libc::poll(&mut pfd, 1, 20) } <= 0 {
                break;
            }
            let mut b = [0u8; 64];
            let n = unsafe { libc::read(self.fd, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
            if n <= 0 {
                break;
            }
            reply.extend_from_slice(&b[..n as usize]);
            if reply.len() > knock.len() {
                break;
            }
        }
        // replies are 0xFA acks; the id follows the last ack
        reply.iter().rposition(|&c| c == 0xfa).and_then(|i| reply.get(i + 1)).is_some_and(|&id| id == 3)
    }

    /// Connect to gpm (`EMUERA_GPMCTL` overrides the socket path, for
    /// tests) for the virtual console we run on: `Gpm_Connect` asks for
    /// every event (we draw the pointer, as for the raw device).
    pub fn open_gpm() -> Result<Mice, String> {
        // our VT number: stdin is /dev/ttyN on the console
        let vc: i32 = std::fs::read_link("/proc/self/fd/0")
            .ok()
            .and_then(|p| p.to_string_lossy().strip_prefix("/dev/tty").and_then(|n| n.parse().ok()))
            .or_else(|| std::env::var("EMUERA_GPM_VC").ok().and_then(|v| v.parse().ok()))
            .ok_or("not on a virtual console")?;
        let path = std::env::var("EMUERA_GPMCTL").unwrap_or_else(|_| "/dev/gpmctl".into());
        let sock = std::os::unix::net::UnixStream::connect(&path).map_err(|e| format!("{}: {}", path, e))?;
        // struct Gpm_Connect { u16 eventMask, defaultMask, minMod, maxMod; i32 pid, vc; }
        let mut c = Vec::with_capacity(16);
        c.extend_from_slice(&0xffffu16.to_ne_bytes()); // every event
        // moves also go to gpm's default handler, which draws its own
        // pointer (only root may: the kernel's selection pointer): GPM_HARD
        // makes gpm pass it events the client gets too. Clicks don't go,
        // so they make no selection
        const GPM_MOVE: u16 = 1;
        const GPM_HARD: u16 = 256;
        c.extend_from_slice(&(GPM_MOVE | GPM_HARD).to_ne_bytes());
        c.extend_from_slice(&0u16.to_ne_bytes());
        c.extend_from_slice(&0xffffu16.to_ne_bytes()); // any modifiers
        c.extend_from_slice(&(std::process::id() as i32).to_ne_bytes());
        c.extend_from_slice(&vc.to_ne_bytes());
        use std::io::Write;
        (&sock).write_all(&c).map_err(|e| format!("gpm: {}", e))?;
        sock.set_nonblocking(true).map_err(|e| format!("gpm: {}", e))?;
        use std::os::unix::io::IntoRawFd;
        let fd = sock.into_raw_fd();
        Ok(Mice { fd, gpm: true, wheel: true, buf: Vec::new(), px: (0, 0), buttons: 0, last_cell: (1, 1) })
    }

    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// gpm's `Gpm_Event` records (28 bytes) → events.
    fn read_gpm(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while self.buf.len() >= 28 {
            let b: Vec<u8> = self.buf.drain(..28).collect();
            let i16_at = |k: usize| i16::from_ne_bytes([b[k], b[k + 1]]);
            let buttons = b[0];
            let (x, y) = (i16_at(8).max(1) as u16, i16_at(10).max(1) as u16);
            let ty = i32::from_ne_bytes([b[12], b[13], b[14], b[15]]);
            let wdy = i16_at(26);
            // Gpm_Event buttons: 4 left, 2 middle, 1 right, 16 / 32 wheel
            let code = if buttons & 4 != 0 {
                0
            } else if buttons & 2 != 0 {
                1
            } else if buttons & 1 != 0 {
                2
            } else {
                3
            };
            if wdy > 0 || (ty & GPM_DOWN as i32 != 0 && buttons & 16 != 0) {
                out.push((64, x, y, true));
            } else if wdy < 0 || (ty & GPM_DOWN as i32 != 0 && buttons & 32 != 0) {
                out.push((65, x, y, true));
            } else if ty & GPM_DOWN as i32 != 0 {
                out.push((code, x, y, true));
            } else if ty & GPM_UP as i32 != 0 {
                out.push((code, x, y, false));
            } else if ty & (GPM_MOVE | GPM_DRAG) as i32 != 0 {
                out.push((32 | code, x, y, true));
            }
        }
        out
    }

    /// Read pending packets and convert them for a `cols`×`rows` screen of
    /// `cell`-pixel cells.
    pub fn read(&mut self, cell: (u32, u32), cols: usize, rows: usize) -> Vec<Event> {
        let mut b = [0u8; 512];
        loop {
            let n = unsafe { libc::read(self.fd, b.as_mut_ptr() as *mut libc::c_void, b.len()) };
            if n <= 0 {
                break;
            }
            self.buf.extend_from_slice(&b[..n as usize]);
        }
        if self.gpm {
            return self.read_gpm();
        }
        let (cw, ch) = (cell.0.max(1) as i32, cell.1.max(1) as i32);
        let (maxx, maxy) = (cols as i32 * cw - 1, rows as i32 * ch - 1);
        let plen = if self.wheel { 4 } else { 3 };
        let mut out = Vec::new();
        let mut i = 0;
        while i + plen <= self.buf.len() {
            let p = &self.buf[i..i + plen];
            // bit 3 of the first byte is always set: resync on garbage
            if p[0] & 0x08 == 0 {
                i += 1;
                continue;
            }
            let dx = p[1] as i32 - if p[0] & 0x10 != 0 { 256 } else { 0 };
            let dy = p[2] as i32 - if p[0] & 0x20 != 0 { 256 } else { 0 };
            self.px.0 = (self.px.0 + dx).clamp(0, maxx.max(0));
            self.px.1 = (self.px.1 - dy).clamp(0, maxy.max(0)); // PS/2 y grows upwards
            let c = ((self.px.0 / cw + 1) as u16, (self.px.1 / ch + 1) as u16);
            if c != self.last_cell {
                self.last_cell = c;
                out.push((35, c.0, c.1, true));
            }
            // left, right, middle -> xterm codes 0, 2, 1
            for (bit, code) in [(0x01u8, 0u16), (0x02, 2), (0x04, 1)] {
                let now = p[0] & bit != 0;
                if now != (self.buttons & bit != 0) {
                    out.push((code, c.0, c.1, now));
                }
            }
            self.buttons = p[0] & 0x07;
            if self.wheel {
                let dz = p[3] as i8;
                if dz != 0 {
                    out.push((if dz < 0 { 64 } else { 65 }, c.0, c.1, true));
                }
            }
            i += plen;
        }
        self.buf.drain(..i);
        out
    }
}

impl Drop for Mice {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Mice reading from a pipe we can write packets into.
    fn piped(wheel: bool) -> (Mice, i32) {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK) }, 0);
        (Mice { fd: fds[0], gpm: false, wheel, buf: Vec::new(), px: (0, 0), buttons: 0, last_cell: (1, 1) }, fds[1])
    }

    fn send(w: i32, b: &[u8]) {
        assert_eq!(unsafe { libc::write(w, b.as_ptr() as *const libc::c_void, b.len()) }, b.len() as isize);
    }

    #[test]
    fn motion_buttons_and_resync() {
        let (mut m, w) = piped(false);
        // garbage byte, then +20 x / -40 y (down), then left press, release
        send(w, &[0x00, 0x08 | 0x20, 20, (256 - 40) as u8, 0x09, 0, 0, 0x08, 0, 0]);
        let ev = m.read((8, 16), 80, 25);
        assert_eq!(ev, vec![(35, 3, 3, true), (0, 3, 3, true), (0, 3, 3, false)]);
        // moving left past the edge clamps to column 1
        send(w, &[0x08 | 0x10, (256 - 100) as u8, 0]);
        assert_eq!(m.read((8, 16), 80, 25), vec![(35, 1, 3, true)]);
        unsafe { libc::close(w) };
    }

    #[test]
    fn wheel_packets() {
        let (mut m, w) = piped(true);
        send(w, &[0x08, 0, 0, 0xff, 0x08, 0, 0, 0x01]);
        assert_eq!(m.read((8, 16), 80, 25), vec![(64, 1, 1, true), (65, 1, 1, true)]);
        unsafe { libc::close(w) };
    }
}
