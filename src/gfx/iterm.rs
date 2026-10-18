//! iTerm2 inline images (`OSC 1337 ; File=…`), for iTerm2 on macOS (and
//! the other terminals that took the protocol up: WezTerm, mintty, VS
//! Code's terminal).
//!
//! The picture is a PNG sized in cells (`width=N;height=M`), so it fits
//! the box the layout reserved whatever the display's pixel density; it
//! is padded with transparent pixels to whole cells first so it isn't
//! stretched. Like sixels the image is part of the cells it covers, and
//! redrawing those cells replaces it. Encoded images are cached.

use super::{crop_rows, Backend, Placement};
use base64::Engine;
use image::RgbaImage;
use std::collections::HashMap;
use std::io::Write;

pub struct Iterm {
    cache: HashMap<(String, u32, u32, i32), Vec<u8>>,
}

impl Iterm {
    pub fn new() -> Iterm {
        Iterm { cache: HashMap::new() }
    }
}

impl Backend for Iterm {
    fn name(&self) -> &'static str {
        "iterm"
    }
    fn draw(&mut self, out: &mut Vec<u8>, ps: &[Placement], cell: (u32, u32)) {
        let (cw, ch) = (cell.0.max(1), cell.1.max(1));
        for p in ps {
            if p.crop_h == 0 || p.row < 0 {
                continue;
            }
            let key = (p.key.clone(), p.crop_top, p.crop_h, p.y_off);
            if !self.cache.contains_key(&key) {
                if self.cache.len() > 128 {
                    self.cache.clear();
                }
                let img = if p.crop_top == 0 && p.crop_h >= p.img.height() { (*p.img).clone() } else { crop_rows(&p.img, p.crop_top, p.crop_h) };
                // whole cells, the offset inside the first cell as padding
                let pad = p.y_off.max(0) as u32;
                let cols = img.width().div_ceil(cw).max(1);
                let rows = (img.height() + pad).div_ceil(ch).max(1);
                let mut t = RgbaImage::new(cols * cw, rows * ch);
                image::imageops::replace(&mut t, &img, 0, pad as i64);
                let mut png = Vec::new();
                if image::DynamicImage::ImageRgba8(t).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
                    continue;
                }
                let mut seq = Vec::new();
                let _ = write!(seq, "\x1b]1337;File=inline=1;size={};width={};height={};preserveAspectRatio=0;doNotMoveCursor=1:", png.len(), cols, rows);
                seq.extend_from_slice(base64::engine::general_purpose::STANDARD.encode(&png).as_bytes());
                seq.push(0x07);
                self.cache.insert(key.clone(), seq);
            }
            let _ = write!(out, "\x1b[{};{}H", p.row + 1, p.col + 1);
            out.extend_from_slice(&self.cache[&key]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn whole_cells_png() {
        let img = std::rc::Rc::new(RgbaImage::from_pixel(13, 20, image::Rgba([255, 0, 0, 255])));
        let p = Placement { key: "a".into(), img, col: 2, row: 3, y_off: 4, crop_top: 0, crop_h: 20, below: false };
        let mut out = Vec::new();
        Iterm::new().draw(&mut out, &[p], (8, 16));
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("\x1b[4;3H\x1b]1337;File=inline=1;"));
        assert!(s.contains(";width=2;height=2;"), "{}", &s[..80]);
        let b64 = &s[s.find(':').unwrap() + 1..s.len() - 1];
        let png = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (16, 32));
        // padded down by y_off, transparent above
        assert_eq!(back.get_pixel(0, 3)[3], 0);
        assert_eq!(back.get_pixel(0, 4)[0], 255);
    }
}
