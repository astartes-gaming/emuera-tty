//! DEC sixel backend with a median-cut quantizer.
//!
//! Sixel images are palette images drawn six pixel rows at a time, so each
//! RGBA image is reduced to at most 255 colours (median cut over a sample of
//! its opaque pixels), with index 255 left transparent. Encoded images are
//! cached per (sprite, size, crop) because encoding is the slow part.

use super::{crop_rows, Backend, Placement};
use image::RgbaImage;
use std::collections::HashMap;
use std::io::Write;

pub struct Sixel {
    cache: HashMap<(String, u32, u32), Vec<u8>>,
}

impl Sixel {
    pub fn new() -> Sixel {
        Sixel { cache: HashMap::new() }
    }
}

/// `img` laid on `bg` and padded with it to whole cells (`cell`), the
/// offset rows `pad` above it included.
///
/// Konsole (and other terminals) fill a sixel's transparent pixels, and
/// the rest of the cells it covers, with the terminal's own background:
/// where the game paints another one (eraMegatenP's black under a
/// terminal's gray), a portrait with a see-through background sat in a
/// gray box. Text under it doesn't show in those terminals either way.
fn on_background(img: &RgbaImage, pad: u32, bg: [u8; 3], cell: (u32, u32)) -> RgbaImage {
    let round_up = |v: u32, step: u32| v.div_ceil(step.max(1)) * step.max(1);
    let (w, h) = (round_up(img.width(), cell.0), round_up(img.height() + pad, cell.1));
    let mut out = RgbaImage::from_pixel(w, h, image::Rgba([bg[0], bg[1], bg[2], 255]));
    for (x, y, p) in img.enumerate_pixels() {
        let a = p[3] as u32;
        let mix = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a) + 127) / 255) as u8;
        out.put_pixel(x, y + pad, image::Rgba([mix(p[0], bg[0]), mix(p[1], bg[1]), mix(p[2], bg[2]), 255]));
    }
    out
}

impl Backend for Sixel {
    fn name(&self) -> &'static str {
        "sixel"
    }
    fn text_erases(&self) -> bool {
        true
    }
    fn draw(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
        for p in ps {
            if p.crop_h == 0 || p.row < 0 {
                continue;
            }
            // sixels start at a cell's top: an offset inside the cell
            // becomes transparent rows above the picture
            let pad = p.y_off.max(0) as u32;
            let key = (format!("{}+{}{:?}", p.key, pad, p.bg), p.crop_top, p.crop_h);
            if !self.cache.contains_key(&key) {
                if self.cache.len() > 128 {
                    self.cache.clear();
                }
                let img = if p.crop_top == 0 && p.crop_h >= p.img.height() { (*p.img).clone() } else { crop_rows(&p.img, p.crop_top, p.crop_h) };
                let img = if let Some(bg) = p.bg {
                    on_background(&img, pad, bg, cell)
                } else if pad == 0 {
                    img
                } else {
                    let mut t = RgbaImage::new(img.width(), img.height() + pad);
                    image::imageops::replace(&mut t, &img, 0, pad as i64);
                    t
                };
                self.cache.insert(key.clone(), encode(&img));
            }
            let _ = write!(out, "\x1b[{};{}H", p.row + 1, p.col + 1);
            out.extend_from_slice(&self.cache[&key]);
        }
    }
}

/// A box of colours in RGB space for median cut.
struct Box3 {
    px: Vec<[u8; 3]>,
}

impl Box3 {
    /// The widest channel (0 R, 1 G, 2 B) and its extent.
    fn range(&self) -> (usize, u8) {
        let mut best = (0, 0u8);
        for c in 0..3 {
            let (mut lo, mut hi) = (255u8, 0u8);
            for p in &self.px {
                lo = lo.min(p[c]);
                hi = hi.max(p[c]);
            }
            let r = hi.saturating_sub(lo);
            if r > best.1 {
                best = (c, r);
            }
        }
        best
    }
    /// Average colour of the box: one palette entry.
    fn mean(&self) -> [u8; 3] {
        let mut s = [0u64; 3];
        for p in &self.px {
            for c in 0..3 {
                s[c] += p[c] as u64;
            }
        }
        let n = self.px.len().max(1) as u64;
        [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
    }
}

/// Split the colour box with the widest channel at its median until there
/// are `max_colors` boxes; their means are the palette.
fn median_cut(samples: Vec<[u8; 3]>, max_colors: usize) -> Vec<[u8; 3]> {
    if samples.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes = vec![Box3 { px: samples }];
    while boxes.len() < max_colors {
        // split the box with the largest channel range
        let (bi, (ch, r)) = boxes.iter().enumerate().map(|(i, b)| (i, b.range())).max_by_key(|(_, (_, r))| *r).unwrap();
        if r == 0 {
            break;
        }
        let mut b = boxes.swap_remove(bi);
        b.px.sort_unstable_by_key(|p| p[ch]);
        let mid = b.px.len() / 2;
        let hi = b.px.split_off(mid);
        boxes.push(b);
        boxes.push(Box3 { px: hi });
    }
    boxes.iter().filter(|b| !b.px.is_empty()).map(|b| b.mean()).collect()
}

/// Encode an image as a sixel DCS string (pixels with alpha < 128 are
/// transparent; nearest-colour lookups are memoised on 15-bit colour).
pub fn encode(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let raw = img.as_raw();
    // sample opaque pixels
    let total = w * h;
    let step = (total / 40_000).max(1);
    let mut samples = Vec::new();
    for i in (0..total).step_by(step) {
        let p = &raw[i * 4..i * 4 + 4];
        if p[3] >= 128 {
            samples.push([p[0], p[1], p[2]]);
        }
    }
    let pal = median_cut(samples, 255);
    let mut lut: HashMap<u16, u8> = HashMap::new();
    let mut idx = vec![255u8; total]; // 255 = transparent
    for i in 0..total {
        let p = &raw[i * 4..i * 4 + 4];
        if p[3] < 128 {
            continue;
        }
        let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
        let c = *lut.entry(key).or_insert_with(|| {
            let mut best = (0usize, u32::MAX);
            for (k, q) in pal.iter().enumerate() {
                let d = (p[0] as i32 - q[0] as i32).pow(2) as u32 + (p[1] as i32 - q[1] as i32).pow(2) as u32 + (p[2] as i32 - q[2] as i32).pow(2) as u32;
                if d < best.1 {
                    best = (k, d);
                }
            }
            best.0 as u8
        });
        idx[i] = c;
    }
    let mut out = Vec::with_capacity(total / 2);
    let _ = write!(out, "\x1bP0;1;0q\"1;1;{};{}", w, h);
    for (k, c) in pal.iter().enumerate() {
        let _ = write!(out, "#{};2;{};{};{}", k, c[0] as u32 * 100 / 255, c[1] as u32 * 100 / 255, c[2] as u32 * 100 / 255);
    }
    let mut band_cols = vec![0u8; w];
    let mut y = 0;
    while y < h {
        let mut present = [false; 256];
        for yy in y..(y + 6).min(h) {
            for x in 0..w {
                present[idx[yy * w + x] as usize] = true;
            }
        }
        let mut first = true;
        for c in 0..255usize {
            if !present[c] {
                continue;
            }
            for x in 0..w {
                let mut bits = 0u8;
                for k in 0..6 {
                    let yy = y + k;
                    if yy < h && idx[yy * w + x] as usize == c {
                        bits |= 1 << k;
                    }
                }
                band_cols[x] = bits;
            }
            if !first {
                out.push(b'$');
            }
            first = false;
            let _ = write!(out, "#{}", c);
            // RLE
            let mut x = 0;
            while x < w {
                let b = band_cols[x];
                let mut run = 1;
                while x + run < w && band_cols[x + run] == b {
                    run += 1;
                }
                let ch = 63 + b;
                if run > 3 {
                    let _ = write!(out, "!{}{}", run, ch as char);
                } else {
                    for _ in 0..run {
                        out.push(ch);
                    }
                }
                x += run;
            }
        }
        out.push(b'-');
        y += 6;
    }
    out.extend_from_slice(b"\x1b\\");
    out
}

#[cfg(test)]
mod tests {
    use super::on_background;
    use image::{Rgba, RgbaImage};

    /// A see-through pixel takes the game's background, a half one is mixed
    /// with it, and the picture is padded with it to whole cells.
    #[test]
    fn pictures_sit_on_the_game_background() {
        let mut img = RgbaImage::from_pixel(3, 2, Rgba([200, 100, 0, 255]));
        img.put_pixel(0, 0, Rgba([255, 255, 255, 0]));
        img.put_pixel(1, 0, Rgba([255, 255, 255, 128]));
        let out = on_background(&img, 1, [0, 0, 40], (4, 4));
        assert_eq!(out.dimensions(), (4, 4));
        assert_eq!(out.get_pixel(0, 0), &Rgba([0, 0, 40, 255]), "offset row");
        assert_eq!(out.get_pixel(0, 1), &Rgba([0, 0, 40, 255]), "transparent pixel");
        assert_eq!(out.get_pixel(1, 1), &Rgba([128, 128, 148, 255]), "half transparent");
        assert_eq!(out.get_pixel(2, 2), &Rgba([200, 100, 0, 255]));
        assert_eq!(out.get_pixel(3, 3), &Rgba([0, 0, 40, 255]), "padding");
        assert!(out.pixels().all(|p| p[3] == 255));
    }
}
