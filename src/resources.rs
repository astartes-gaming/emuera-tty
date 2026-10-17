//! resources/*.csv sprite definitions and image loading/scaling cache.
//!
//! A game's `resources/` folder holds images plus CSVs naming sprites:
//! `NAME,file.png[,x,y,w,h]` (the rectangle cuts the sprite out of a sheet).
//! Images are decoded on first use and cached three ways: whole files,
//! cropped sprites, and sprites scaled to the size they are shown at.

use crate::loader::{csv_fields, read_text, walk};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// A named image: a file, optionally a rectangle of it.
#[derive(Clone, Debug)]
pub struct Sprite {
    pub file: PathBuf,
    pub rect: Option<(u32, u32, u32, u32)>,
}

#[derive(Default)]
pub struct Resources {
    pub sprites: HashMap<String, Sprite>,
    files: HashMap<PathBuf, Option<Rc<RgbaImage>>>,
    cropped: HashMap<String, Option<Rc<RgbaImage>>>,
    scaled: HashMap<(String, u32, u32), Rc<RgbaImage>>,
    pub warnings: Vec<String>,
    /// Emuera's graphics buffers (GCREATE…), by id
    pub graphics: HashMap<i64, Graphic>,
    /// sprites made by SPRITECREATE: (graphics id, x, y, w, h) — they show
    /// the buffer as it is now, like Emuera's SpriteG
    g_sprites: HashMap<String, (i64, i32, i32, i32, i32)>,
    /// bumped whenever a buffer or SPRITECREATE sprite changes, so cached
    /// renderings of them (scaled copies, images sent to the terminal) renew
    pub gen: u64,
}

/// A graphics buffer and its brush (GSETBRUSH).
pub struct Graphic {
    pub img: RgbaImage,
    pub brush: [u8; 4],
}

/// Emuera's ARGB integer → RGBA bytes.
pub fn argb(c: i64) -> [u8; 4] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8]
}

/// RGBA bytes → Emuera's ARGB integer.
pub fn to_argb(p: [u8; 4]) -> i64 {
    ((p[3] as i64) << 24) | ((p[0] as i64) << 16) | ((p[1] as i64) << 8) | p[2] as i64
}

/// `src` drawn over `dst` (normal alpha blending, straight alpha).
fn over(dst: &mut [u8; 4], src: [u8; 4]) {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    for k in 0..3 {
        let v = (src[k] as f32 * sa + dst[k] as f32 * da * (1.0 - sa)) / oa;
        dst[k] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (oa * 255.0).round() as u8;
}

/// A GDI+ colour matrix applied to one pixel: `[r g b a 1] × M`.
fn color_matrix(p: [u8; 4], m: &[[f32; 5]; 5]) -> [u8; 4] {
    let v = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0, 1.0];
    let mut out = [0u8; 4];
    for (j, o) in out.iter_mut().enumerate() {
        let x: f32 = (0..5).map(|i| v[i] * m[i][j]).sum();
        *o = (x * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Draw `src`'s rectangle `(sx, sy, sw, sh)` into `dst` at `(dx, dy)`
/// scaled to `dw × dh` (negative sizes mirror, as GDI+ does), blended over
/// what is there, optionally through a colour matrix.
#[allow(clippy::too_many_arguments)]
pub fn draw_image(dst: &mut RgbaImage, src: &RgbaImage, (dx, dy, dw, dh): (i32, i32, i32, i32), (sx, sy, sw, sh): (i32, i32, i32, i32), cm: Option<&[[f32; 5]; 5]>) {
    if dw == 0 || dh == 0 || sw <= 0 || sh <= 0 {
        return;
    }
    // the source rectangle, clipped to the image
    let x0 = sx.clamp(0, src.width() as i32) as u32;
    let y0 = sy.clamp(0, src.height() as i32) as u32;
    let x1 = (sx + sw).clamp(0, src.width() as i32) as u32;
    let y1 = (sy + sh).clamp(0, src.height() as i32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let part = image::imageops::crop_imm(src, x0, y0, x1 - x0, y1 - y0).to_image();
    let (w, h) = (dw.unsigned_abs(), dh.unsigned_abs());
    let mut scaled = if (w, h) == part.dimensions() { part } else { image::imageops::resize(&part, w, h, image::imageops::FilterType::Triangle) };
    if dw < 0 {
        image::imageops::flip_horizontal_in_place(&mut scaled);
    }
    if dh < 0 {
        image::imageops::flip_vertical_in_place(&mut scaled);
    }
    let ox = if dw < 0 { dx + dw } else { dx };
    let oy = if dh < 0 { dy + dh } else { dy };
    for (x, y, p) in scaled.enumerate_pixels() {
        let (tx, ty) = (ox + x as i32, oy + y as i32);
        if tx < 0 || ty < 0 || tx >= dst.width() as i32 || ty >= dst.height() as i32 {
            continue;
        }
        let px = match cm {
            Some(m) => color_matrix(p.0, m),
            None => p.0,
        };
        over(&mut dst.get_pixel_mut(tx as u32, ty as u32).0, px);
    }
}

impl Resources {
    /// Read every sprite CSV under `dir` (images are not decoded yet).
    pub fn load(dir: Option<&Path>) -> Resources {
        let mut r = Resources::default();
        let Some(dir) = dir else { return r };
        for csv in walk(dir, &["csv"]) {
            let base = csv.parent().unwrap_or(dir).to_path_buf();
            let Ok(text) = read_text(&csv) else { continue };
            for line in text.lines() {
                let f = csv_fields(line);
                if f.len() < 2 {
                    continue;
                }
                if f[1].eq_ignore_ascii_case("ANIME") {
                    continue; // animated sprites: not supported yet
                }
                let file = find_ci(&base, &f[1]);
                let rect = if f.len() >= 6 {
                    let n: Vec<u32> = f[2..6].iter().map(|s| s.parse::<i64>().unwrap_or(0).max(0) as u32).collect();
                    Some((n[0], n[1], n[2], n[3]))
                } else {
                    None
                };
                r.sprites.insert(f[0].to_uppercase(), Sprite { file, rect });
            }
        }
        r
    }

    pub fn exists(&self, name: &str) -> bool {
        let k = name.to_uppercase();
        self.sprites.contains_key(&k) || self.g_sprites.contains_key(&k)
    }

    /// A buffer or SPRITECREATE sprite changed: drop cached renderings.
    pub fn changed(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.scaled.clear();
        self.cropped.retain(|k, _| !self.g_sprites.contains_key(k));
    }

    /// Cache version of `name`'s pixels: 0 for image files (they never
    /// change), the buffer generation for SPRITECREATE sprites.
    pub fn version(&self, name: &str) -> u64 {
        if self.g_sprites.contains_key(&name.to_uppercase()) {
            self.gen
        } else {
            0
        }
    }

    /// SPRITECREATE name, buffer, rectangle. False if the name is taken
    /// or the buffer doesn't exist.
    pub fn create_g_sprite(&mut self, name: &str, gid: i64, rect: Option<(i32, i32, i32, i32)>) -> bool {
        if name.is_empty() || self.exists(name) {
            return false;
        }
        let Some(g) = self.graphics.get(&gid) else { return false };
        let r = rect.unwrap_or((0, 0, g.img.width() as i32, g.img.height() as i32));
        self.g_sprites.insert(name.to_uppercase(), (gid, r.0, r.1, r.2, r.3));
        self.changed();
        true
    }

    /// SPRITEDISPOSE: only SPRITECREATE sprites go away (resource-file
    /// ones stay, as in Emuera where they'd be reloaded).
    pub fn dispose_sprite(&mut self, name: &str) -> bool {
        let gone = self.g_sprites.remove(&name.to_uppercase()).is_some();
        if gone {
            self.changed();
        }
        gone
    }

    /// Decode an image file once; failures are remembered as None.
    fn file(&mut self, p: &Path) -> Option<Rc<RgbaImage>> {
        if let Some(v) = self.files.get(p) {
            return v.clone();
        }
        let img = match image::open(p) {
            Ok(i) => Some(Rc::new(i.to_rgba8())),
            Err(e) => {
                self.warnings.push(format!("cannot load {}: {}", p.display(), e));
                None
            }
        };
        self.files.insert(p.to_path_buf(), img.clone());
        img
    }

    /// The sprite's pixels (cropped from its sheet).
    pub fn sprite(&mut self, name: &str) -> Option<Rc<RgbaImage>> {
        let key = name.to_uppercase();
        if let Some(v) = self.cropped.get(&key) {
            return v.clone();
        }
        if let Some(&(gid, x, y, w, h)) = self.g_sprites.get(&key) {
            // the buffer's rectangle as it is now (cached until it changes)
            let g = self.graphics.get(&gid)?;
            let mut out = RgbaImage::new(w.unsigned_abs().max(1), h.unsigned_abs().max(1));
            draw_image(&mut out, &g.img, (0, 0, w.abs().max(1), h.abs().max(1)), (x, y, w.abs(), h.abs()), None);
            let out = Some(Rc::new(out));
            self.cropped.insert(key, out.clone());
            return out;
        }
        let sp = self.sprites.get(&key)?.clone();
        let full = self.file(&sp.file);
        let out = full.map(|img| match sp.rect {
            None => img,
            Some((x, y, w, h)) => {
                let x = x.min(img.width());
                let y = y.min(img.height());
                let w = w.min(img.width() - x).max(1);
                let h = h.min(img.height() - y).max(1);
                Rc::new(image::imageops::crop_imm(&*img, x, y, w, h).to_image())
            }
        });
        self.cropped.insert(key, out.clone());
        out
    }

    pub fn sprite_size(&mut self, name: &str) -> Option<(u32, u32)> {
        let key = name.to_uppercase();
        if let Some(&(_, _, _, w, h)) = self.g_sprites.get(&key) {
            return Some((w.unsigned_abs(), h.unsigned_abs()));
        }
        if let Some(Sprite { rect: Some((_, _, w, h)), .. }) = self.sprites.get(&key) {
            return Some((*w, *h));
        }
        self.sprite(name).map(|i| (i.width(), i.height()))
    }

    /// Sprite scaled to exactly w x h pixels (cached).
    pub fn scaled(&mut self, name: &str, w: u32, h: u32) -> Option<Rc<RgbaImage>> {
        let key = (name.to_uppercase(), w.max(1), h.max(1));
        if let Some(i) = self.scaled.get(&key) {
            return Some(i.clone());
        }
        let src = self.sprite(name)?;
        let out = Rc::new(image::imageops::resize(&*src, key.1, key.2, image::imageops::FilterType::Triangle));
        if self.scaled.len() > 512 {
            self.scaled.clear();
        }
        self.scaled.insert(key, out.clone());
        Some(out)
    }
}

/// `base/rel` matching each path component case-insensitively and
/// accepting `\` separators, since games are made on Windows.
fn find_ci(base: &Path, rel: &str) -> PathBuf {
    let direct = base.join(rel);
    if direct.exists() {
        return direct;
    }
    // case-insensitive walk of each component (games are made on Windows)
    let mut cur = base.to_path_buf();
    for comp in rel.split(['/', '\\']) {
        if comp.is_empty() {
            continue;
        }
        match crate::loader::find_file_ci(&cur, comp) {
            Some(p) => cur = p,
            None => return base.join(rel.replace('\\', "/")),
        }
    }
    cur
}
