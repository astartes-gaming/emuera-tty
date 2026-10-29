//! KMSCON terminal backend: direct framebuffer blitting for KMSCON when
//! using the fbdev video backend (/dev/fb0).
//!
//! KMSCON is a userspace console for Linux using KMS/DRM. When built with
//! the fbdev video backend, it writes to /dev/fb0 like the Linux framebuffer
//! console. This backend reuses the fb logic for that case.
//!
//! ⚠️ WARNING: This backend is unlikely to work in practice. KMSCON controls
//! the framebuffer as a userspace process; applications running inside KMSCON
//! cannot directly write to /dev/fb0 without conflicting with KMSCON's own
//! rendering. For graphics to work properly in KMSCON, KMSCON would need to
//! implement a terminal graphics protocol (sixel, kitty graphics, etc.) in
//! libtsm and its renderers. This backend is kept for experimentation only.

use super::{Backend, Placement};
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

pub struct Kmscon {
    _file: std::fs::File,
    /// mmapped framebuffer memory
    mem: *mut u8,
    len: usize,
    pub xres: u32,
    pub yres: u32,
    /// visible area offset inside the (virtual) framebuffer
    xoff: u32,
    yoff: u32,
    bpp: u32,
    /// bytes per scanline
    line: usize,
    /// (bit offset, bit length) of each channel
    r: (u32, u32),
    g: (u32, u32),
    b: (u32, u32),
}

impl Kmscon {
    /// Open and map `$FRAMEBUFFER` (default /dev/fb0); needs write access
    /// (usually the `video` group).
    pub fn open() -> Result<Kmscon, String> {
        let path = std::env::var("FRAMEBUFFER").unwrap_or_else(|_| "/dev/fb0".into());
        let file = OpenOptions::new().read(true).write(true).open(&path).map_err(|e| format!("{}: {}", path, e))?;
        let fd = file.as_raw_fd();
        let mut var = [0u32; 40];
        let mut fix = [0u8; 128];
        unsafe {
            if libc::ioctl(fd, FBIOGET_VSCREENINFO as _, var.as_mut_ptr()) != 0 {
                return Err("FBIOGET_VSCREENINFO failed".into());
            }
            if libc::ioctl(fd, FBIOGET_FSCREENINFO as _, fix.as_mut_ptr()) != 0 {
                return Err("FBIOGET_FSCREENINFO failed".into());
            }
        }
        let base = 16 + std::mem::size_of::<usize>() + 16 + 6;
        let ll_off = (base + 3) & !3;
        let line = u32::from_ne_bytes([fix[ll_off], fix[ll_off + 1], fix[ll_off + 2], fix[ll_off + 3]]) as usize;
        let yres_v = var[3].max(var[1]);
        let len = line * yres_v as usize;
        let mem = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        if mem == libc::MAP_FAILED {
            return Err("mmap of framebuffer failed".into());
        }
        let bpp = var[6];
        if !matches!(bpp, 16 | 24 | 32) {
            return Err(format!("unsupported framebuffer depth {}bpp", bpp));
        }
        Ok(Kmscon {
            _file: file,
            mem: mem as *mut u8,
            len,
            xres: var[0],
            yres: var[1],
            xoff: var[4],
            yoff: var[5],
            bpp,
            line,
            r: (var[8], var[9]),
            g: (var[11], var[12]),
            b: (var[14], var[15]),
        })
    }

    #[inline]
    /// Native pixel → RGB.
    fn unpack(&self, px: u32) -> [u8; 3] {
        let ch = |(off, len): (u32, u32)| -> u8 {
            if len == 0 {
                return 0;
            }
            let v = (px >> off) & ((1 << len) - 1);
            ((v * 255) / ((1 << len) - 1)) as u8
        };
        [ch(self.r), ch(self.g), ch(self.b)]
    }

    #[inline]
    /// RGB → native pixel.
    fn pack(&self, c: [u8; 3]) -> u32 {
        let ch = |v: u8, (off, len): (u32, u32)| -> u32 {
            if len == 0 {
                return 0;
            }
            ((v as u32 * ((1 << len) - 1) + 127) / 255) << off
        };
        ch(c[0], self.r) | ch(c[1], self.g) | ch(c[2], self.b)
    }

    /// Alpha-blend rows `crop_top..+crop_h` of `img` at pixel (x0, y0),
    /// clipped to the screen.
    fn blit(&mut self, img: &image::RgbaImage, x0: i32, y0: i32, crop_top: u32, crop_h: u32) {
        let bytes = (self.bpp / 8) as usize;
        let w = img.width() as i32;
        for iy in crop_top..(crop_top + crop_h).min(img.height()) {
            let y = y0 + (iy - crop_top) as i32;
            if y < 0 || y >= self.yres as i32 {
                continue;
            }
            let row = (y as usize + self.yoff as usize) * self.line;
            for ix in 0..w {
                let x = x0 + ix;
                if x < 0 || x >= self.xres as i32 {
                    continue;
                }
                let p = img.get_pixel(ix as u32, iy).0;
                if p[3] == 0 {
                    continue;
                }
                let off = row + (x as usize + self.xoff as usize) * bytes;
                if off + bytes > self.len {
                    continue;
                }
                unsafe {
                    let dst = self.mem.add(off);
                    let cur = match bytes {
                        4 => (dst as *const u32).read_unaligned(),
                        3 => (*dst as u32) | (*dst.add(1) as u32) << 8 | (*dst.add(2) as u32) << 16,
                        _ => (dst as *const u16).read_unaligned() as u32,
                    };
                    let bg = self.unpack(cur);
                    let a = p[3] as u32;
                    let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a)) / 255) as u8;
                    let out = self.pack([mix(p[0], bg[0]), mix(p[1], bg[1]), mix(p[2], bg[2])]);
                    match bytes {
                        4 => (dst as *mut u32).write_unaligned(out),
                        3 => {
                            *dst = out as u8;
                            *dst.add(1) = (out >> 8) as u8;
                            *dst.add(2) = (out >> 16) as u8;
                        }
                        _ => (dst as *mut u16).write_unaligned(out as u16),
                    }
                }
            }
        }
    }
}

impl Drop for Kmscon {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mem as *mut libc::c_void, self.len);
        }
    }
}

impl Backend for Kmscon {
    fn name(&self) -> &'static str {
        "kmscon"
    }
    fn after_flush(&mut self, ps: &[Placement], cell: (u32, u32)) {
        // give the console a moment to finish painting text
        std::thread::sleep(std::time::Duration::from_millis(15));
        for p in ps {
            if p.crop_h == 0 {
                continue;
            }
            let x = p.col * cell.0 as i32;
            let y = p.row.max(0) * cell.1 as i32 + p.y_off;
            self.blit(&p.img, x, y, p.crop_top, p.crop_h);
        }
    }
    fn invert_cell(&mut self, col: usize, row: usize, cell: (u32, u32)) {
        let bytes = (self.bpp / 8) as usize;
        // flip the colour bits only (not padding or alpha)
        let bits = |(off, len): (u32, u32)| if len == 0 { 0 } else { ((1u32 << len) - 1) << off };
        let mask = bits(self.r) | bits(self.g) | bits(self.b);
        let (x0, y0) = (col * cell.0 as usize, row * cell.1 as usize);
        for y in y0..(y0 + cell.1 as usize).min(self.yres as usize) {
            let line = (y + self.yoff as usize) * self.line;
            for x in x0..(x0 + cell.0 as usize).min(self.xres as usize) {
                let off = line + (x + self.xoff as usize) * bytes;
                if off + bytes > self.len {
                    continue;
                }
                unsafe {
                    let dst = self.mem.add(off);
                    match bytes {
                        4 => (dst as *mut u32).write_unaligned((dst as *const u32).read_unaligned() ^ mask),
                        3 => {
                            for k in 0..3 {
                                *dst.add(k) ^= (mask >> (8 * k)) as u8;
                            }
                        }
                        _ => (dst as *mut u16).write_unaligned((dst as *const u16).read_unaligned() ^ mask as u16),
                    }
                }
            }
        }
    }
}
