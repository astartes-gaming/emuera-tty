//! Direct /dev/fb0 blitting for the Linux framebuffer console (fbcon) and
//! fbterm. Images are alpha-blended over whatever the console drew.
//!
//! The framebuffer is mmapped; its pixel format (bits per pixel and the
//! bit offset/length of R, G, B) comes from FBIOGET_VSCREENINFO, so 16, 24
//! and 32 bpp all work. Since the console keeps drawing text into the same
//! memory, images are painted *after* each frame's text has been flushed.

use super::{Backend, Placement};
use std::fs::OpenOptions;
use std::os::unix::io::AsRawFd;

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

pub struct Fb {
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
    /// the pointer arrow, made for one cell size
    arrow: Option<((u32, u32), image::RgbaImage)>,
}

/// The mouse pointer: X outline, . fill, space see-through.
const ARROW: [&str; 19] = [
    "X           ",
    "XX          ",
    "X.X         ",
    "X..X        ",
    "X...X       ",
    "X....X      ",
    "X.....X     ",
    "X......X    ",
    "X.......X   ",
    "X........X  ",
    "X.........X ",
    "X......XXXXX",
    "X...X..X    ",
    "X..XX..X    ",
    "X.X  X..X   ",
    "XX   X..X   ",
    "X     X..X  ",
    "      X..X  ",
    "       XX   ",
];

/// The arrow scaled to fill one cell.
fn arrow(cell: (u32, u32)) -> image::RgbaImage {
    let (w, h) = (cell.0.max(1), cell.1.max(1));
    image::RgbaImage::from_fn(w, h, |x, y| {
        let row = ARROW[(y * ARROW.len() as u32 / h) as usize].as_bytes();
        match row[(x * row.len() as u32 / w) as usize] {
            b'X' => image::Rgba([0, 0, 0, 255]),
            b'.' => image::Rgba([255, 255, 255, 255]),
            _ => image::Rgba([0, 0, 0, 0]),
        }
    })
}

impl Fb {
    /// Open and map `$FRAMEBUFFER` (default /dev/fb0); needs write access
    /// (usually the `video` group).
    pub fn open() -> Result<Fb, String> {
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
        Ok(Fb {
            _file: file,
            mem: mem as *mut u8,
            len,
            xres: var[0],
            yres: var[1],
            xoff: var[4],
            yoff: var[5],
            bpp,
            arrow: None,
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

impl Drop for Fb {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mem as *mut libc::c_void, self.len);
        }
    }
}

impl Backend for Fb {
    fn name(&self) -> &'static str {
        "fb"
    }
    fn draw_pointer(&mut self, cell: (u32, u32), at: (usize, usize)) {
        // after the console has painted the cell's text
        std::thread::sleep(std::time::Duration::from_millis(15));
        if self.arrow.as_ref().is_none_or(|a| a.0 != cell) {
            self.arrow = Some((cell, arrow(cell)));
        }
        let img = self.arrow.take().unwrap();
        self.blit(&img.1, at.0 as i32 * cell.0 as i32, at.1 as i32 * cell.1 as i32, 0, cell.1);
        self.arrow = Some(img);
    }

    fn draws_pointer(&self) -> bool {
        true
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
}
