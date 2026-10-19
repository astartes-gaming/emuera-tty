//! Inline image backends: kitty graphics protocol, DEC sixel, and direct
//! Linux framebuffer blitting (for the fbcon VT / fbterm).
//!
//! The renderer works out *where* images go (cell, pixel offset, visible
//! rows) and hands each frame's [`Placement`]s to the chosen [`Backend`],
//! which only knows *how* to put pixels on that kind of terminal.

#[cfg(target_os = "linux")]
pub mod fb;
#[cfg(not(target_os = "linux"))]
#[path = "fb_none.rs"]
pub mod fb;
pub mod iterm;
pub mod kitty;
pub mod sixel;

use image::RgbaImage;
use std::rc::Rc;

/// An image to show on screen, already scaled to its pixel size.
#[derive(Clone)]
pub struct Placement {
    pub key: String,
    pub img: Rc<RgbaImage>,
    /// top-left cell (0-based); row may be negative if scrolled partly off
    pub col: i32,
    pub row: i32,
    /// vertical pixel offset inside the first cell (ypos)
    pub y_off: i32,
    /// visible pixel rows of the image: [crop_top, crop_top + crop_h)
    pub crop_top: u32,
    pub crop_h: u32,
    /// behind the text (a CBG layer with depth > 0), where the terminal can
    pub below: bool,
}

/// One way of showing images. All hooks default to doing nothing.
pub trait Backend {
    /// "kitty", "iterm", "sixel", "fb" or "none" (for the startup message).
    fn name(&self) -> &'static str;
    /// Called before text is written for a frame (e.g. delete old placements).
    fn begin(&mut self, _out: &mut Vec<u8>) {}
    /// Emit escape sequences for placements (written after text).
    fn draw(&mut self, _out: &mut Vec<u8>, _p: &[Placement], _cell: (u32, u32)) {}
    /// Called after the frame has been written and drained to the tty.
    fn after_flush(&mut self, _p: &[Placement], _cell: (u32, u32)) {}
    /// The screen was cleared or may be out of step (Ctrl-L): place
    /// everything again on the next frame.
    fn reset(&mut self, _out: &mut Vec<u8>) {}
    /// Leaving: clean up anything on screen.
    fn shutdown(&mut self, _out: &mut Vec<u8>) {}
}

/// No image support: images appear as `░` placeholders of the right size.
pub struct NoGfx;
impl Backend for NoGfx {
    fn name(&self) -> &'static str {
        "none"
    }
}

/// Overlapping pictures made into one, drawn back to front as Emuera does.
///
/// A portrait is often a pile of layers (body, face, blush, sweat, a frame,
/// a tint while time is stopped) printed on successive lines and moved
/// onto each other with `ypos`. Terminals stack separate pictures by their
/// own rules (kitty by image id, Konsole by which was placed last), and only
/// pictures that changed are placed again, so a layer could end up over or
/// under the wrong one. Sent as one picture, the order can't go wrong; it
/// also helps sixel and iTerm2, which can't stack transparent pictures.
#[derive(Default)]
pub struct Merger {
    /// made pictures by the layers they were made of
    cache: std::collections::HashMap<String, Rc<RgbaImage>>,
}

impl Merger {
    /// `ps` (back to front) with each group of overlapping pictures
    /// replaced by one picture, where the group's first one was (those
    /// under the text and those over it apart).
    /// `cell`: a cell's size in pixels.
    pub fn merge(&mut self, ps: Vec<Placement>, cell: (u32, u32)) -> Vec<Placement> {
        let (cw, ch) = (cell.0.max(1) as i64, cell.1.max(1) as i64);
        // pixel rectangle on screen of what shows of each picture
        let rect = |p: &Placement| {
            let (x, y) = (p.col as i64 * cw, p.row as i64 * ch + p.y_off as i64);
            (x, y, x + p.img.width() as i64, y + p.crop_h as i64)
        };
        let n = ps.len();
        // groups of overlapping pictures (union-find)
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for i in 0..n {
            if ps[i].crop_h == 0 {
                continue;
            }
            let a = rect(&ps[i]);
            for j in 0..i {
                // (pictures under the text merge only among themselves)
                if ps[j].crop_h == 0 || ps[j].below != ps[i].below {
                    continue;
                }
                let b = rect(&ps[j]);
                if a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                    let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                    if ri != rj {
                        parent[ri.max(rj)] = ri.min(rj);
                    }
                }
            }
        }
        let roots: Vec<usize> = (0..n).map(|i| root(&mut parent, i)).collect();
        if roots.iter().enumerate().all(|(i, &r)| r == i) {
            return ps;
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if roots[i] != i {
                continue;
            }
            let members: Vec<&Placement> = (i..n).filter(|&j| roots[j] == i).map(|j| &ps[j]).collect();
            if members.len() == 1 {
                out.push(ps[i].clone());
                continue;
            }
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for m in &members {
                let r = rect(m);
                (x0, y0, x1, y1) = (x0.min(r.0), y0.min(r.1), x1.max(r.2), y1.max(r.3));
            }
            let mut key = String::from("merge");
            for m in &members {
                let r = rect(m);
                key.push_str(&format!("|{}@{},{}+{}+{}", m.key, r.0 - x0, r.1 - y0, m.crop_top, m.crop_h));
            }
            let img = match self.cache.get(&key) {
                Some(img) => img.clone(),
                None => {
                    let mut c = RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
                    for m in &members {
                        let r = rect(m);
                        blend_rows(&mut c, &m.img, m.crop_top, m.crop_h, (r.0 - x0) as u32, (r.1 - y0) as u32);
                    }
                    let img = Rc::new(c);
                    // (a portrait or two per screen; old ones aren't shown again)
                    if self.cache.len() >= 48 {
                        self.cache.clear();
                    }
                    self.cache.insert(key.clone(), img.clone());
                    img
                }
            };
            out.push(Placement {
                key,
                img,
                col: (x0 / cw) as i32,
                row: (y0 / ch) as i32,
                y_off: (y0 % ch) as i32,
                crop_top: 0,
                crop_h: (y1 - y0) as u32,
                below: members[0].below,
            });
        }
        out
    }
}

impl Merger {
    /// `p` with the pixels under the cells `cells` (columns, rows: start
    /// inclusive, end exclusive, 0-based) made transparent. A tooltip is
    /// text, and pictures sit above text: cut out of the picture, it
    /// shows. Cells map to pixels through the box of cells the picture is
    /// shown in, as terminals stretch it to fill that box.
    pub fn cut(&mut self, p: Placement, cells: (i64, i64, i64, i64), cell: (u32, u32)) -> Placement {
        let (cw, ch) = (cell.0.max(1) as f64, cell.1.max(1) as f64);
        let (w, h) = (p.img.width() as f64, p.crop_h as f64);
        let (bc, br) = ((w / cw).ceil().max(1.0), (h / ch).ceil().max(1.0));
        let sx = |c: i64| (((c - p.col as i64) as f64 * w / bc).round() as i64).clamp(0, w as i64);
        let sy = |r: i64| (((r - p.row as i64) as f64 * h / br - p.y_off as f64 * h / (br * ch)).round() as i64).clamp(0, h as i64);
        let (x0, x1) = (sx(cells.0), sx(cells.2));
        let (y0, y1) = (sy(cells.1), sy(cells.3));
        if x0 >= x1 || y0 >= y1 {
            return p;
        }
        // (rows of the image: the visible part starts at crop_top)
        let (y0, y1) = (y0 + p.crop_top as i64, y1 + p.crop_top as i64);
        let key = format!("{}|cut{},{},{},{}", p.key, x0, y0, x1, y1);
        let img = match self.cache.get(&key) {
            Some(i) => i.clone(),
            None => {
                let mut c = (*p.img).clone();
                for yy in y0..y1.min(c.height() as i64) {
                    for xx in x0..x1 {
                        c.put_pixel(xx as u32, yy as u32, image::Rgba([0, 0, 0, 0]));
                    }
                }
                let i = Rc::new(c);
                if self.cache.len() >= 48 {
                    self.cache.clear();
                }
                self.cache.insert(key.clone(), i.clone());
                i
            }
        };
        Placement { key, img, ..p }
    }
}

/// Draw rows `top..top + h` of `src` over `dst` at (`dx`, `dy`), blending
/// by alpha ("over", straight alpha).
fn blend_rows(dst: &mut RgbaImage, src: &RgbaImage, top: u32, h: u32, dx: u32, dy: u32) {
    let (dw, dh) = (dst.width() as usize, dst.height() as usize);
    let sw = src.width() as usize;
    let (dx, dy) = (dx as usize, dy as usize);
    if dx >= dw {
        return;
    }
    let w = sw.min(dw - dx);
    let (s, d) = (src.as_raw(), &mut **dst);
    for sy in top as usize..(top + h).min(src.height()) as usize {
        let y = dy + (sy - top as usize);
        if y >= dh {
            break;
        }
        let srow = &s[sy * sw * 4..(sy * sw + w) * 4];
        let drow = &mut d[(y * dw + dx) * 4..(y * dw + dx + w) * 4];
        for (sp, dp) in srow.chunks_exact(4).zip(drow.chunks_exact_mut(4)) {
            let sa = sp[3] as u32;
            if sa == 0 {
                continue;
            }
            if sa == 255 {
                dp.copy_from_slice(sp);
                continue;
            }
            let da = dp[3] as u32 * (255 - sa) / 255;
            let a = sa + da;
            for k in 0..3 {
                dp[k] = ((sp[k] as u32 * sa + dp[k] as u32 * da) / a) as u8;
            }
            dp[3] = a as u8;
        }
    }
}

/// Crop helper shared by sixel and fb backends.
/// The pixel rows `top..top+h` of `img` (for images cut off at the screen edge).
pub fn crop_rows(img: &RgbaImage, top: u32, h: u32) -> RgbaImage {
    let top = top.min(img.height());
    let h = h.min(img.height() - top);
    image::imageops::crop_imm(img, 0, top, img.width(), h.max(1)).to_image()
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    fn place(key: &str, c: [u8; 4], w: u32, h: u32, col: i32, row: i32, below: bool) -> Placement {
        Placement { key: key.into(), img: Rc::new(RgbaImage::from_pixel(w, h, image::Rgba(c))), col, row, y_off: 0, crop_top: 0, crop_h: h, below }
    }

    #[test]
    fn overlapping_layers_become_one() {
        let mut m = Merger::default();
        let base = place("base", [200, 0, 0, 255], 20, 20, 0, 0, false);
        // half-transparent blue over the right half of the base
        let tint = place("tint", [0, 0, 200, 128], 10, 20, 1, 0, false);
        let apart = place("apart", [0, 200, 0, 255], 10, 10, 10, 0, false);
        let bg = place("bg", [9, 9, 9, 255], 40, 40, 0, 0, true);
        let out = m.merge(vec![bg, base, tint, apart], (10, 10));
        assert_eq!(out.len(), 3, "base and tint merged");
        assert_eq!(out[0].key, "bg");
        let merged = &out[1];
        assert!(merged.key.starts_with("merge|base@0,0"));
        assert_eq!((merged.img.width(), merged.img.height(), merged.col, merged.row), (20, 20, 0, 0));
        assert_eq!(merged.img.get_pixel(2, 2).0, [200, 0, 0, 255]);
        let p = merged.img.get_pixel(15, 5).0;
        assert!(p[3] == 255 && p[0] > 90 && p[0] < 110 && p[2] > 90 && p[2] < 110, "blended: {:?}", p);
        assert_eq!(out[2].key, "apart");
        // the same layers again: the picture made before
        let again = m.merge(vec![place("base", [200, 0, 0, 255], 20, 20, 0, 0, false), place("tint", [0, 0, 200, 128], 10, 20, 1, 0, false)], (10, 10));
        assert!(Rc::ptr_eq(&again[0].img, &merged.img));
    }
}
