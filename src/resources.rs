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
    /// columns 7-8: where the sprite is drawn relative to the point it is
    /// drawn at (Emuera's DestBasePosition; portrait parts use it to sit
    /// on the body)
    pub pos: (i32, i32),
    /// columns 10-11: the size it is drawn at (default: the rectangle's)
    pub dest: Option<(u32, u32)>,
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
    /// the bytes buffers may take unpacked (see [`Resources::trim_graphics`])
    pub gbuf_budget: usize,
    /// sprites made by SPRITECREATE: (graphics id, x, y, w, h) — they show
    /// the buffer as it is now, like Emuera's SpriteG
    g_sprites: HashMap<String, (i64, i32, i32, i32, i32)>,
    /// bumped whenever a buffer or SPRITECREATE sprite changes, so cached
    /// renderings of them (scaled copies, images sent to the terminal) renew
    pub gen: u64,
    /// client background layers (CBGSETG / CBGSETSPRITE /
    /// CBGSETBUTTONSPRITE), back to front
    pub cbg: Vec<CbgLayer>,
    /// CBGSETBMAPG: the buffer whose pixel colours are button values
    pub cbg_bmap: Option<i64>,
    /// image sizes read from file headers (before decoding)
    dims: HashMap<PathBuf, (u32, u32)>,
    /// SPRITESETPOS / SPRITEMOVE base positions (over the CSV's)
    pos_override: HashMap<String, (i64, i64)>,
    /// when each buffer last changed (`gen` values)
    g_gen: HashMap<i64, u64>,
    /// prepared (cut, scaled) parts of draws, by source, its version, the
    /// rectangle and size: a picture redrawn every frame is scaled once
    parts: HashMap<(String, u64, (i32, i32, i32, i32), i32, i32), Rc<RgbaImage>>,
    /// last fingerprint and copy of each buffer shown as a CBG layer
    cbg_cache: HashMap<i64, (u64, Rc<RgbaImage>)>,
    /// animated sprites (`name,ANIME,w,h` in a resources CSV, or
    /// SPRITEANIMECREATE): their frames are sprites of their own
    pub anime: HashMap<String, Anime>,
    /// SETANIMETIMER: how often the screen redraws for animations (ms, 0: off)
    pub anim_timer: i64,
    /// an animated sprite was drawn since the last [`Resources::take_anim_used`]
    anim_used: bool,
}

/// An animated sprite: its size and frames (frame sprite name, ms shown).
#[derive(Default, Clone)]
pub struct Anime {
    pub size: (u32, u32),
    pub frames: Vec<(String, u32)>,
}

/// Milliseconds since the first animation frame was asked for.
fn anim_clock() -> u128 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis()
}

/// A fast hash of an image's pixels and size.
fn fingerprint(img: &RgbaImage) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325 ^ ((img.width() as u64) << 32 | img.height() as u64);
    for c in img.as_raw().chunks(8) {
        let mut b = [0u8; 8];
        b[..c.len()].copy_from_slice(c);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    h
}

/// What a client background layer shows.
#[derive(Clone, Debug, PartialEq)]
pub enum CbgSrc {
    /// a graphics buffer, live: later drawing into it shows
    G(i64),
    Sprite(String),
}

/// A CBG layer: anchored by its bottom-left corner at (`x`, `y`) from the
/// window's bottom-left (Emuera pixels, `y` down), `z` > 0 behind the
/// text, < 0 in front. A button layer shows `hover` while the button map
/// colour under the mouse is `button`.
#[derive(Clone, Debug)]
pub struct CbgLayer {
    pub src: CbgSrc,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub button: Option<(i64, Option<CbgSrc>)>,
}

/// Text drawn into a buffer (GDRAWTEXT), kept so terminals without
/// images can still show a menu drawn as a picture.
#[derive(Clone, Debug)]
pub struct GText {
    pub x: i32,
    pub y: i32,
    pub size: i32,
    pub color: [u8; 4],
    pub text: String,
}

/// A graphics buffer's pixels: as they are, or packed while unused when
/// buffers hold more than [`Resources::gbuf_budget`] (games like pops-tw
/// keep a few portraits per character in buffers: hundreds of MB).
#[derive(Clone)]
enum Pixels {
    Plain(RgbaImage),
    /// run-length coded (see `rle_pack`)
    Packed(Vec<u32>),
}

/// Bumped on every use of a buffer's pixels: least recently used first
/// when packing.
static GBUF_CLOCK: crate::atomic64::AtomicU64 = crate::atomic64::AtomicU64::new(0);

/// Run-length code pixels: a run of one value as `0x8000_0000 | n, value`,
/// other pixels as `n, pixels…`. Exact; drawn portraits are mostly runs of
/// transparent pixels.
fn rle_pack(raw: &[u8]) -> Vec<u32> {
    let px: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut out = Vec::new();
    let (mut i, mut lit) = (0, 0);
    let flush = |out: &mut Vec<u32>, a: usize, b: usize| {
        if b > a {
            out.push((b - a) as u32);
            out.extend_from_slice(&px[a..b]);
        }
    };
    while i < px.len() {
        let v = px[i];
        let mut j = i + 1;
        while j < px.len() && px[j] == v && j - i < 0x7FFF_FFFF {
            j += 1;
        }
        if j - i >= 4 {
            flush(&mut out, lit, i);
            out.push(0x8000_0000 | (j - i) as u32);
            out.push(v);
            lit = j;
        }
        i = j;
    }
    flush(&mut out, lit, px.len());
    out.shrink_to_fit();
    out
}

fn rle_unpack(packed: &[u32], w: u32, h: u32) -> RgbaImage {
    let n = w as usize * h as usize * 4;
    let mut out: Vec<u8> = Vec::with_capacity(n);
    let mut k = 0;
    while k < packed.len() {
        let t = packed[k];
        if t & 0x8000_0000 != 0 {
            let (run, v) = ((t & 0x7FFF_FFFF) as usize, packed[k + 1]);
            if v == 0 {
                out.resize(out.len() + run * 4, 0);
            } else {
                for _ in 0..run {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            k += 2;
        } else {
            for v in &packed[k + 1..k + 1 + t as usize] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            k += 1 + t as usize;
        }
    }
    out.resize(n, 0);
    RgbaImage::from_raw(w, h, out).expect("buffer size")
}

/// How many bytes graphics buffers may take unpacked: `EMUERA_GBUF_MB`, else
/// an eighth of the machine's memory (no limit where that's unknown).
fn gbuf_budget() -> usize {
    crate::sys::memory_budget("EMUERA_GBUF_MB", 8)
}

/// The source of [`Resources::draw_cached`]: a picture or a buffer.
pub enum PartSrc<'a> {
    Img(&'a RgbaImage),
    G(&'a Graphic),
}

/// A graphics buffer and its drawing state.
#[derive(Clone)]
pub struct Graphic {
    /// the pixels: [`Graphic::img`] / [`Graphic::img_mut`]
    px: std::cell::RefCell<Pixels>,
    w: u32,
    h: u32,
    /// `GBUF_CLOCK` when the pixels were last used
    used: std::cell::Cell<u64>,
    /// GSETBRUSH (fills, text); None: the console's text colour for text
    pub brush: [u8; 4],
    pub brush_set: bool,
    /// GSETPEN (lines, text outline): colour, width
    pub pen: ([u8; 4], i64),
    pub pen_set: bool,
    /// GSETFONT: name, size in px, style bits (1 bold 2 italic 4 strike 8 underline)
    pub font: (String, i64, i64),
    /// GDASHSTYLE: (style, cap)
    pub dash: (i64, i64),
    /// GDRAWTEXT calls since the last clear
    pub texts: Vec<GText>,
}

impl Graphic {
    pub fn new(img: RgbaImage) -> Graphic {
        let (w, h) = img.dimensions();
        Graphic {
            px: std::cell::RefCell::new(Pixels::Plain(img)),
            w,
            h,
            used: std::cell::Cell::new(GBUF_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            brush: [0, 0, 0, 255],
            brush_set: false,
            pen: ([0, 0, 0, 255], 1),
            pen_set: false,
            font: (String::new(), 100, 0),
            dash: (0, 0),
            texts: Vec::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    fn touch(&self) {
        self.used.set(GBUF_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    }

    /// The pixels (unpacked first if they were packed).
    pub fn img(&self) -> std::cell::Ref<'_, RgbaImage> {
        self.touch();
        if matches!(*self.px.borrow(), Pixels::Packed(_)) {
            let mut p = self.px.borrow_mut();
            if let Pixels::Packed(v) = &*p {
                *p = Pixels::Plain(rle_unpack(v, self.w, self.h));
            }
        }
        std::cell::Ref::map(self.px.borrow(), |p| match p {
            Pixels::Plain(i) => i,
            Pixels::Packed(_) => unreachable!("unpacked above"),
        })
    }

    pub fn img_mut(&mut self) -> &mut RgbaImage {
        self.touch();
        let (w, h) = (self.w, self.h);
        let p = self.px.get_mut();
        if let Pixels::Packed(v) = p {
            *p = Pixels::Plain(rle_unpack(v, w, h));
        }
        match p {
            Pixels::Plain(i) => i,
            Pixels::Packed(_) => unreachable!("unpacked above"),
        }
    }

    /// Replace all the pixels (a clear).
    pub fn set_img(&mut self, img: RgbaImage) {
        self.touch();
        *self.px.get_mut() = Pixels::Plain(img);
    }

    /// Bytes the pixels take as they are now, and whether they're plain.
    fn bytes(&self) -> (usize, bool) {
        match &*self.px.borrow() {
            Pixels::Plain(i) => (i.as_raw().capacity(), true),
            Pixels::Packed(v) => (v.capacity() * 4, false),
        }
    }

    /// Pack the pixels; the bytes that frees.
    fn pack(&mut self) -> usize {
        let p = self.px.get_mut();
        let Pixels::Plain(i) = p else { return 0 };
        let before = i.as_raw().capacity();
        let v = rle_pack(i.as_raw());
        let after = v.capacity() * 4;
        // (noise doesn't pack: keep it as it is)
        if after >= before {
            return 0;
        }
        *p = Pixels::Packed(v);
        before - after
    }
}

/// A line from (x0, y0) to (x1, y1), `w` pixels wide, blended over `img`.
pub fn draw_line(img: &mut RgbaImage, (x0, y0): (i32, i32), (x1, y1): (i32, i32), w: i64, c: [u8; 4], dash: i64) {
    let (dx, dy) = ((x1 - x0) as f32, (y1 - y0) as f32);
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = len.ceil() as i32;
    let r = (w.max(1) as f32) / 2.0;
    // GDI+ dash styles 1 dash, 2 dot, 3 dash-dot, 4 dash-dot-dot (in pen widths)
    let pattern: &[f32] = match dash {
        1 => &[3.0, 1.0],
        2 => &[1.0, 1.0],
        3 => &[3.0, 1.0, 1.0, 1.0],
        4 => &[3.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        _ => &[],
    };
    let period: f32 = pattern.iter().sum::<f32>() * w.max(1) as f32;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        if period > 0.0 {
            // skip the gaps of the dash pattern
            let mut pos = (t * len) % period;
            let mut on = true;
            for seg in pattern.iter().map(|s| s * w.max(1) as f32) {
                if pos < seg {
                    break;
                }
                pos -= seg;
                on = !on;
            }
            if !on {
                continue;
            }
        }
        let (cx, cy) = (x0 as f32 + dx * t, y0 as f32 + dy * t);
        let (lo_x, hi_x) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (lo_y, hi_y) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in lo_y..hi_y.max(lo_y + 1) {
            for x in lo_x..hi_x.max(lo_x + 1) {
                if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                    over(&mut img.get_pixel_mut(x as u32, y as u32).0, c);
                }
            }
        }
    }
}

/// `src` rotated by `deg` degrees about its point (px, py), drawn so that
/// point lands on the middle of `dst` (Emuera's GDRAWGWITHROTATE).
pub fn draw_rotated(dst: &mut RgbaImage, src: &RgbaImage, deg: f32, (px, py): (f32, f32)) {
    let (s, c) = deg.to_radians().sin_cos();
    let (cx, cy) = (dst.width() as f32 / 2.0, dst.height() as f32 / 2.0);
    for y in 0..dst.height() {
        for x in 0..dst.width() {
            // inverse rotation: where in `src` this pixel comes from
            let (rx, ry) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let sx = rx * c + ry * s + px;
            let sy = -rx * s + ry * c + py;
            if sx >= 0.0 && sy >= 0.0 && (sx as u32) < src.width() && (sy as u32) < src.height() {
                let p = src.get_pixel(sx as u32, sy as u32).0;
                over(&mut dst.get_pixel_mut(x, y).0, p);
            }
        }
    }
}

/// System fonts by name (looked up through fontconfig once each).
fn font_for(name: &str) -> Option<std::sync::Arc<ab_glyph::FontVec>> {
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<ab_glyph::FontVec>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(f) = cache.lock().ok()?.get(name) {
        return f.clone();
    }
    // prefer a face that covers Japanese, as Emuera games need; for the
    // fixed-pitch Japanese fonts games are laid out in (MS Gothic, BIZ
    // UDGothic) one whose half-width letters are half an em too, or Latin
    // text comes out narrower than the game measured it
    let query = match (name.is_empty(), fixed_pitch(name)) {
        (true, _) => "monospace:lang=ja".to_string(),
        (false, true) => format!("{}:spacing=mono:lang=ja", name),
        (false, false) => format!("{}:lang=ja", name),
    };
    let found = std::process::Command::new("fc-match")
        .args(["-f", "%{file}|%{index}", &query])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let (f, i) = s.split_once('|')?;
            let data = std::fs::read(f).ok()?;
            ab_glyph::FontVec::try_from_vec_and_index(data, i.trim().parse().unwrap_or(0)).ok()
        })
        .or_else(|| system_font_file(name))
        .map(Arc::new);
    cache.lock().ok()?.insert(name.to_string(), found.clone());
    found
}

/// Without fontconfig (Windows, macOS): the usual Japanese system fonts,
/// the requested one first.
fn system_font_file(name: &str) -> Option<ab_glyph::FontVec> {
    let n = name.to_lowercase().replace([' ', '　'], "");
    let mut c: Vec<(std::path::PathBuf, u32)> = Vec::new();
    if cfg!(windows) {
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        let proportional = n.contains("pgothic") || n.contains("ｐゴシック") || n.contains("pゴシック");
        if n.contains("meiryo") || n.contains("メイリオ") {
            c.push((dir.join("meiryo.ttc"), 0));
        }
        if n.contains("yugoth") || n.contains("游ゴシック") {
            c.push((dir.join("YuGothM.ttc"), 0));
        }
        if n.contains("biz") {
            c.push((dir.join("BIZ-UDGothicR.ttc"), 0));
        }
        if n.contains("mincho") || n.contains("明朝") {
            c.push((dir.join("msmincho.ttc"), if proportional { 1 } else { 0 }));
        }
        // MS Gothic is the first face of msgothic.ttc, MS PGothic the third
        c.push((dir.join("msgothic.ttc"), if proportional { 2 } else { 0 }));
        c.push((dir.join("meiryo.ttc"), 0));
        c.push((dir.join("YuGothM.ttc"), 0));
    } else if cfg!(target_os = "macos") {
        for f in ["/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/Supplemental/Osaka.ttf", "/Library/Fonts/Osaka.ttf", "/Library/Fonts/Arial Unicode.ttf", "/System/Library/Fonts/Supplemental/Arial Unicode.ttf"] {
            c.push((f.into(), 0));
        }
    }
    c.into_iter().find_map(|(f, i)| ab_glyph::FontVec::try_from_vec_and_index(std::fs::read(f).ok()?, i).ok())
}

/// Is `name` a fixed-pitch Japanese font (or the default, MS Gothic)?
/// There every half-width character is half the font size wide, which is
/// what games assume when they centre text (`STRLENS(text) * size / 2`).
pub fn fixed_pitch(name: &str) -> bool {
    let n = name.to_lowercase().replace(' ', "");
    if n.is_empty() {
        return true;
    }
    let proportional = n.contains("pgothic") || n.contains("pmincho") || n.contains("ｐゴシック") || n.contains("ｐ明朝") || n.contains("pゴシック") || n.contains("p明朝");
    let japanese_fixed = n.contains("gothic") || n.contains("ゴシック") || n.contains("mincho") || n.contains("明朝") || n.contains("mono");
    japanese_fixed && !proportional
}

/// A character's advance in a fixed-pitch Japanese font: half the size per
/// Shift_JIS byte (half-width 1, full-width 2).
fn fixed_advance(c: char, size: f32) -> f32 {
    let mut b = [0u8; 4];
    crate::loader::sjis_len(c.encode_utf8(&mut b)).clamp(1, 2) as f32 * size / 2.0
}

/// Width and height in pixels of `text` in font `name` at `size` px.
pub fn text_size(text: &str, name: &str, size: f32) -> (i64, i64) {
    use ab_glyph::{Font, ScaleFont};
    let Some(font) = font_for(name) else { return ((crate::loader::sjis_len(text) as f32 * size / 2.0) as i64, size as i64) };
    let sf = font.as_scaled(em_scale(&*font, size));
    let w: f32 = if fixed_pitch(name) { text.chars().map(|c| fixed_advance(c, size)).sum() } else { text.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum() };
    (w.ceil() as i64, sf.height().ceil() as i64)
}

/// The scale that makes the em square `size` pixels (GDI+'s font size).
fn em_scale(font: &impl ab_glyph::Font, size: f32) -> ab_glyph::PxScale {
    let upm = font.units_per_em().unwrap_or(1000.0);
    ab_glyph::PxScale::from(size * font.height_unscaled() / upm)
}

/// GDRAWTEXT: `text` with its top-left at (x, y), filled with `c`.
/// A rasterised glyph: offset from its pen position, size, coverage.
type GlyphMask = std::rc::Rc<(i32, i32, u32, u32, Vec<u8>)>;

thread_local! {
    /// Glyphs already rasterised, by font, size, glyph and sub-pixel
    /// position (quarters): scripts redraw the same text every frame.
    static GLYPHS: std::cell::RefCell<HashMap<(String, u32, u16, u8, u8), GlyphMask>> = std::cell::RefCell::new(HashMap::new());
}

pub fn draw_text(img: &mut RgbaImage, text: &str, (x, y): (i32, i32), name: &str, size: f32, c: [u8; 4]) {
    use ab_glyph::{Font, ScaleFont};
    let Some(font) = font_for(name) else { return };
    let scale = em_scale(&*font, size);
    let sf = font.as_scaled(scale);
    let mut pen_x = x as f32;
    let base = y as f32 + sf.ascent();
    let (by, bq) = (base.floor(), ((base - base.floor()) * 4.0).round().min(3.0));
    let fixed = fixed_pitch(name);
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        // fixed pitch: the game's grid, each glyph centred in its cell
        let (glyph_x, adv) = if fixed {
            let adv = fixed_advance(ch, size);
            (pen_x + (adv - sf.h_advance(id)) / 2.0, adv)
        } else {
            (pen_x, sf.h_advance(id))
        };
        let (px0, xq) = (glyph_x.floor(), ((glyph_x - glyph_x.floor()) * 4.0).round().min(3.0));
        pen_x += adv;
        let key = (name.to_string(), size.to_bits(), id.0, xq as u8, bq as u8);
        let mask = GLYPHS.with(|g| g.borrow().get(&key).cloned()).or_else(|| {
            let g = id.with_scale_and_position(scale, ab_glyph::point(xq / 4.0, bq / 4.0));
            let o = font.outline_glyph(g)?;
            let b = o.px_bounds();
            let (w, h) = (b.width().max(0.0) as u32, b.height().max(0.0) as u32);
            let mut cov = vec![0u8; (w * h) as usize];
            o.draw(|gx, gy, v| {
                if gx < w && gy < h {
                    cov[(gy * w + gx) as usize] = (v.min(1.0) * 255.0) as u8;
                }
            });
            let m: GlyphMask = std::rc::Rc::new((b.min.x as i32, b.min.y as i32, w, h, cov));
            GLYPHS.with(|g| {
                let mut g = g.borrow_mut();
                if g.len() > 8192 {
                    g.clear();
                }
                g.insert(key, m.clone());
            });
            Some(m)
        });
        let Some(m) = mask else { continue };
        let (ox, oy, w, h, cov) = (&m.0, &m.1, m.2, m.3, &m.4);
        for gy in 0..h {
            for gx in 0..w {
                let a = cov[(gy * w + gx) as usize];
                if a == 0 {
                    continue;
                }
                let (px, py) = (px0 as i32 + ox + gx as i32, by as i32 + oy + gy as i32);
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    let mut col = c;
                    col[3] = ((c[3] as u32 * a as u32) / 255) as u8;
                    over(&mut img.get_pixel_mut(px as u32, py as u32).0, col);
                }
            }
        }
    }
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
pub fn draw_image(dst: &mut RgbaImage, src: &RgbaImage, dest: (i32, i32, i32, i32), srect: (i32, i32, i32, i32), cm: Option<&[[f32; 5]; 5]>) {
    if let Some(part) = prepare_part(src, srect, dest.2, dest.3) {
        blit_part(dst, &part, dest, cm);
    }
}

/// The source rectangle of `src`, clipped, scaled to |dw| × |dh| and
/// mirrored for negative sizes (the slow part of a draw, worth caching).
pub fn prepare_part(src: &RgbaImage, (sx, sy, sw, sh): (i32, i32, i32, i32), dw: i32, dh: i32) -> Option<RgbaImage> {
    if dw == 0 || dh == 0 || sw <= 0 || sh <= 0 {
        return None;
    }
    let x0 = sx.clamp(0, src.width() as i32) as u32;
    let y0 = sy.clamp(0, src.height() as i32) as u32;
    let x1 = (sx + sw).clamp(0, src.width() as i32) as u32;
    let y1 = (sy + sh).clamp(0, src.height() as i32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (dw.unsigned_abs(), dh.unsigned_abs());
    let view = image::imageops::crop_imm(src, x0, y0, x1 - x0, y1 - y0);
    let mut scaled = if (w, h) == (x1 - x0, y1 - y0) { view.to_image() } else { image::imageops::resize(&*view, w, h, image::imageops::FilterType::Triangle) };
    if dw < 0 {
        image::imageops::flip_horizontal_in_place(&mut scaled);
    }
    if dh < 0 {
        image::imageops::flip_vertical_in_place(&mut scaled);
    }
    Some(scaled)
}

/// Blend a prepared part over `dst` at the destination rectangle.
pub fn blit_part(dst: &mut RgbaImage, part: &RgbaImage, (dx, dy, dw, dh): (i32, i32, i32, i32), cm: Option<&[[f32; 5]; 5]>) {
    let ox = if dw < 0 { dx + dw } else { dx };
    let oy = if dh < 0 { dy + dh } else { dy };
    let (dwid, dhei) = (dst.width() as i32, dst.height() as i32);
    for (x, y, p) in part.enumerate_pixels() {
        let (tx, ty) = (ox + x as i32, oy + y as i32);
        if tx < 0 || ty < 0 || tx >= dwid || ty >= dhei {
            continue;
        }
        let px = match cm {
            Some(m) => color_matrix(p.0, m),
            None => p.0,
        };
        if px[3] == 0 {
            continue;
        }
        over(&mut dst.get_pixel_mut(tx as u32, ty as u32).0, px);
    }
}

impl Resources {
    /// Read every sprite CSV under `dir` (images are not decoded yet).
    pub fn load(dir: Option<&Path>) -> Resources {
        let mut r = Resources { gbuf_budget: gbuf_budget(), ..Resources::default() };
        let Some(dir) = dir else { return r };
        for csv in walk(dir, &["csv"]) {
            let base = csv.parent().unwrap_or(dir).to_path_buf();
            let Ok(text) = read_text(&csv) else { continue };
            // the animated sprite whose frame lines follow
            let mut cur_anime: Option<String> = None;
            for line in text.lines() {
                let f = csv_fields(line);
                if f.len() < 2 {
                    continue;
                }
                let key = f[0].to_uppercase();
                if f[1].eq_ignore_ascii_case("ANIME") {
                    // name,ANIME,width,height; then name,file,x,y,w,h,offx,offy,ms per frame
                    let n = |i: usize| f.get(i).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
                    r.anime.insert(key.clone(), Anime { size: (n(2), n(3)), frames: Vec::new() });
                    cur_anime = Some(key);
                    continue;
                }
                let frame = cur_anime.as_ref().is_some_and(|a| *a == key);
                if !frame {
                    cur_anime = None;
                }
                let file = find_ci(&base, &f[1]);
                let rect = if f.len() >= 6 {
                    let n: Vec<u32> = f[2..6].iter().map(|s| s.parse::<i64>().unwrap_or(0).max(0) as u32).collect();
                    Some((n[0], n[1], n[2], n[3]))
                } else {
                    None
                };
                let num = |i: usize| f.get(i).and_then(|s| s.trim().parse::<i32>().ok());
                let pos = match (num(6), num(7)) {
                    (Some(x), Some(y)) => (x, y),
                    _ => (0, 0),
                };
                let dest = match (num(9), num(10)) {
                    (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as u32, h as u32)),
                    _ => None,
                };
                if frame {
                    let a = r.anime.get_mut(&key).unwrap();
                    let fname = format!("{}#{}", key, a.frames.len());
                    let ms = f.get(8).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(100).max(1);
                    a.frames.push((fname.clone(), ms));
                    r.sprites.insert(fname, Sprite { file, rect, pos, dest: None });
                    continue;
                }
                r.sprites.insert(key, Sprite { file, rect, pos, dest });
            }
        }
        r
    }

    /// Add a CBG layer (kept sorted back to front).
    pub fn cbg_add(&mut self, l: CbgLayer) {
        self.cbg.push(l);
        // stable: equal depths keep their order
        self.cbg.sort_by(|a, b| b.z.cmp(&a.z));
    }

    /// The pixels a CBG source shows now, and a fingerprint of them (so an
    /// unchanged picture isn't sent to the terminal again).
    pub fn cbg_pixels(&mut self, src: &CbgSrc) -> Option<(Rc<RgbaImage>, u64)> {
        match src {
            CbgSrc::G(gid) => {
                let g = self.graphics.get(gid)?;
                let img = g.img();
                let h = fingerprint(&img);
                if let Some((k, img)) = &self.cbg_cache.get(gid) {
                    if *k == h {
                        return Some((img.clone(), h));
                    }
                }
                let img = Rc::new(img.clone());
                self.cbg_cache.insert(*gid, (h, img.clone()));
                Some((img, h))
            }
            CbgSrc::Sprite(n) => {
                if !self.exists(n) {
                    return None;
                }
                let v = self.version(n);
                self.sprite(n).map(|i| (i, v ^ 0x5bd1e995))
            }
        }
    }

    /// The button map's value at a point from the window's bottom-left
    /// (Emuera pixels, `y` <= 0): the pixel's 0xRRGGBB if fully opaque.
    pub fn cbg_button_at(&self, x: i64, y: i64) -> Option<i64> {
        let g = self.graphics.get(&self.cbg_bmap?)?;
        let (w, h) = (g.width() as i64, g.height() as i64);
        let my = y + h;
        if x < 0 || my < 0 || x >= w || my >= h {
            return None;
        }
        let p = g.img().get_pixel(x as u32, my as u32).0;
        (p[3] == 255).then_some(((p[0] as i64) << 16) | ((p[1] as i64) << 8) | p[2] as i64)
    }

    /// A sprite cut from a graphics buffer exists only while the buffer
    /// does (Emuera's SpriteG.IsCreated); it comes back if the buffer is
    /// created again under the same id.
    /// The sprite to show for `name` now: the current frame of an animated
    /// sprite (by the clock, looping), else `name` itself.
    pub fn frame_name(&mut self, name: &str) -> String {
        let Some(a) = self.anime.get(&name.to_uppercase()) else { return name.to_string() };
        let total: u128 = a.frames.iter().map(|f| f.1 as u128).sum();
        if total == 0 {
            return name.to_string();
        }
        let mut t = anim_clock() % total;
        let mut pick = &a.frames[0].0;
        for (n, ms) in &a.frames {
            if t < *ms as u128 {
                pick = n;
                break;
            }
            t -= *ms as u128;
        }
        let pick = pick.clone();
        self.anim_used = true;
        pick
    }

    /// Was an animated sprite drawn since the last call?
    /// Keep the unpacked graphics buffers within [`Resources::gbuf_budget`]:
    /// pack the least recently used ones (they unpack when used again).
    pub fn trim_graphics(&mut self) {
        let plain: usize = self.graphics.values().map(|g| g.bytes()).filter(|b| b.1).map(|b| b.0).sum();
        if plain <= self.gbuf_budget {
            return;
        }
        let mut lru: Vec<(u64, i64)> = self.graphics.iter().filter(|(_, g)| g.bytes().1).map(|(id, g)| (g.used.get(), *id)).collect();
        lru.sort_unstable();
        let mut over = plain - self.gbuf_budget;
        for (_, id) in lru {
            if over == 0 {
                break;
            }
            if let Some(g) = self.graphics.get_mut(&id) {
                over = over.saturating_sub(g.pack());
            }
        }
    }

    /// Pictures held: (what, how many, bytes), for the memory report.
    pub fn mem_stats(&self) -> Vec<(&'static str, usize, usize)> {
        let px = |i: &RgbaImage| i.as_raw().capacity();
        let packed: Vec<usize> = self.graphics.values().map(|g| g.bytes()).filter(|b| !b.1).map(|b| b.0).collect();
        vec![
            ("graphics buffers", self.graphics.len(), self.graphics.values().map(|g| g.bytes().0).sum()),
            ("  of them packed", packed.len(), packed.iter().sum()),
            ("decoded image files", self.files.len(), self.files.values().flatten().map(|i| px(i)).sum()),
            ("cut sprites", self.cropped.len(), self.cropped.values().flatten().map(|i| px(i)).sum()),
            ("scaled sprites", self.scaled.len(), self.scaled.values().map(|i| px(i)).sum()),
            ("prepared draws", self.parts.len(), self.parts.values().map(|i| px(i)).sum()),
            ("background copies", self.cbg_cache.len(), self.cbg_cache.values().map(|(_, i)| px(i)).sum()),
        ]
    }

    pub fn take_anim_used(&mut self) -> bool {
        std::mem::take(&mut self.anim_used)
    }

    /// SPRITEANIMECREATE name, w, h.
    pub fn anime_create(&mut self, name: &str, w: u32, h: u32) -> bool {
        let k = name.to_uppercase();
        if self.exists(&k) {
            return false;
        }
        self.anime.insert(k, Anime { size: (w, h), frames: Vec::new() });
        true
    }

    /// SPRITEANIMEADDFRAME name, gID, x, y, w, h, offset x, y, ms.
    #[allow(clippy::too_many_arguments)]
    pub fn anime_add_frame(&mut self, name: &str, gid: i64, rect: (i32, i32, i32, i32), off: (i64, i64), ms: u32) -> bool {
        let k = name.to_uppercase();
        let Some(n) = self.anime.get(&k).map(|a| a.frames.len()) else { return false };
        let fname = format!("{}#{}", k, n);
        if !self.create_g_sprite(&fname, gid, Some(rect)) {
            return false;
        }
        self.set_base_pos(&fname, off);
        self.anime.get_mut(&k).unwrap().frames.push((fname, ms.max(1)));
        true
    }

    pub fn exists(&self, name: &str) -> bool {
        let k = name.to_uppercase();
        self.anime.contains_key(&k) || self.sprites.contains_key(&k) || self.g_sprites.get(&k).is_some_and(|s| self.graphics.contains_key(&s.0))
    }

    /// The SPRITECREATE sprites changed (made / dropped): drop every cached
    /// rendering of one.
    pub fn changed(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        let gs = &self.g_sprites;
        self.scaled.retain(|k, _| !gs.contains_key(&k.0));
        self.cropped.retain(|k, _| !gs.contains_key(k));
    }

    /// Buffer `gid`'s pixels changed: only the sprites cut from it need
    /// renewing (drawing into one buffer used to throw away every cached
    /// sprite, and big source images were copied again on every draw).
    pub fn changed_g(&mut self, gid: i64) {
        self.gen = self.gen.wrapping_add(1);
        self.g_gen.insert(gid, self.gen);
        let gs = &self.g_sprites;
        let from_g = |k: &String| gs.get(k).is_some_and(|s| s.0 == gid);
        self.scaled.retain(|k, _| !from_g(&k.0));
        self.cropped.retain(|k, _| !from_g(k));
    }

    /// Cache version of `name`'s pixels: 0 for image files (they never
    /// change), its buffer's last change for SPRITECREATE sprites.
    pub fn version(&self, name: &str) -> u64 {
        match self.g_sprites.get(&name.to_uppercase()) {
            Some(s) => self.g_gen.get(&s.0).copied().unwrap_or(0) ^ (s.0 as u64).rotate_left(32),
            None => 0,
        }
    }

    /// Draw `src` (an image, known by `key` + `version`) into buffer
    /// `dst`, reusing the scaled cut-out from an earlier identical draw.
    pub fn draw_cached(&mut self, dst: i64, key: &str, version: u64, src: PartSrc, dest: (i32, i32, i32, i32), srect: (i32, i32, i32, i32), cm: Option<&[[f32; 5]; 5]>) {
        let k = (key.to_string(), version, srect, dest.2, dest.3);
        let part = match self.parts.get(&k) {
            Some(p) => p.clone(),
            None => {
                // (a packed buffer unpacks only when its part isn't cached)
                let p = match src {
                    PartSrc::Img(i) => prepare_part(i, srect, dest.2, dest.3),
                    PartSrc::G(g) => prepare_part(&g.img(), srect, dest.2, dest.3),
                };
                let Some(p) = p else { return };
                if self.parts.len() > 64 {
                    self.parts.clear();
                }
                let p = Rc::new(p);
                self.parts.insert(k, p.clone());
                p
            }
        };
        if let Some(g) = self.graphics.get_mut(&dst) {
            blit_part(g.img_mut(), &part, dest, cm);
        }
    }

    /// The version of buffer `gid`'s pixels.
    pub fn g_version(&self, gid: i64) -> u64 {
        self.g_gen.get(&gid).copied().unwrap_or(0)
    }

    /// A sprite's base position (SPRITEPOSX / SPRITEPOSY).
    pub fn base_pos(&self, name: &str) -> (i64, i64) {
        let k = name.to_uppercase();
        if let Some(p) = self.pos_override.get(&k) {
            return *p;
        }
        self.sprites.get(&k).map(|s| (s.pos.0 as i64, s.pos.1 as i64)).unwrap_or((0, 0))
    }

    pub fn set_base_pos(&mut self, name: &str, p: (i64, i64)) {
        self.pos_override.insert(name.to_uppercase(), p);
        self.gen = self.gen.wrapping_add(1);
    }

    /// A SPRITECREATE sprite's source: (buffer id, x, y, w, h).
    pub fn g_sprite(&self, name: &str) -> Option<(i64, i32, i32, i32, i32)> {
        self.g_sprites.get(&name.to_uppercase()).copied().filter(|s| self.graphics.contains_key(&s.0))
    }

    /// SPRITECREATE name, buffer, rectangle. False if the name is taken
    /// or the buffer doesn't exist.
    pub fn create_g_sprite(&mut self, name: &str, gid: i64, rect: Option<(i32, i32, i32, i32)>) -> bool {
        if name.is_empty() || self.exists(name) {
            return false;
        }
        let Some(g) = self.graphics.get(&gid) else { return false };
        // (replacing one whose buffer was disposed)
        self.cropped.remove(&name.to_uppercase());
        let r = rect.unwrap_or((0, 0, g.width() as i32, g.height() as i32));
        self.g_sprites.insert(name.to_uppercase(), (gid, r.0, r.1, r.2, r.3));
        self.changed();
        true
    }

    /// SPRITEDISPOSE: only SPRITECREATE sprites go away (resource-file
    /// ones stay, as in Emuera where they'd be reloaded).
    /// SPRITEDISPOSEALL: drop every SPRITECREATE sprite; how many.
    pub fn dispose_all_sprites(&mut self) -> i64 {
        let n = self.g_sprites.len() as i64;
        self.g_sprites.clear();
        self.changed();
        n
    }

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
        if self.anime.contains_key(&name.to_uppercase()) {
            let f = self.frame_name(name);
            return self.sprite(&f);
        }
        let key = name.to_uppercase();
        if let Some(v) = self.cropped.get(&key) {
            return v.clone();
        }
        if let Some(&(gid, x, y, w, h)) = self.g_sprites.get(&key) {
            // the buffer's rectangle as it is now (cached until it changes)
            let g = self.graphics.get(&gid)?;
            let mut out = RgbaImage::new(w.unsigned_abs().max(1), h.unsigned_abs().max(1));
            draw_image(&mut out, &g.img(), (0, 0, w.abs().max(1), h.abs().max(1)), (x, y, w.abs(), h.abs()), None);
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
        if let Some(a) = self.anime.get(&key) {
            return Some(a.size);
        }
        if let Some(&(gid, _, _, w, h)) = self.g_sprites.get(&key) {
            return self.graphics.contains_key(&gid).then_some((w.unsigned_abs(), h.unsigned_abs()));
        }
        if let Some(Sprite { dest: Some(d), .. }) = self.sprites.get(&key) {
            return Some(*d);
        }
        if let Some(Sprite { rect: Some((_, _, w, h)), .. }) = self.sprites.get(&key) {
            return Some((*w, *h));
        }
        // a whole image file: its header has the size, so the (slow) decode
        // can wait for the screen, which decodes on all cores
        if let Some(sp) = self.sprites.get(&key) {
            if !self.files.contains_key(&sp.file) {
                if let Some(d) = self.dims.get(&sp.file) {
                    return Some(*d);
                }
                if let Ok(d) = image::ImageReader::open(&sp.file).and_then(|r| r.with_guessed_format()).map_err(image::ImageError::IoError).and_then(|r| r.into_dimensions()) {
                    self.dims.insert(sp.file.clone(), d);
                    return Some(d);
                }
            }
        }
        self.sprite(name).map(|i| (i.width(), i.height()))
    }

    /// Scale every listed sprite that isn't cached yet, in parallel
    /// (one thread per core, up to the number of pictures).
    pub fn prescale(&mut self, want: Vec<(&str, u32, u32)>) {
        // image files not decoded yet: decode them on all cores first
        let mut files: Vec<PathBuf> = Vec::new();
        for (name, w, h) in &want {
            let key = (name.to_uppercase(), (*w).max(1), (*h).max(1));
            if self.scaled.contains_key(&key) || self.cropped.contains_key(&key.0) {
                continue;
            }
            if let Some(sp) = self.sprites.get(&key.0) {
                if !self.files.contains_key(&sp.file) && !files.contains(&sp.file) {
                    files.push(sp.file.clone());
                }
            }
        }
        if files.len() >= 2 {
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get()).min(files.len());
            let mut decoded: Vec<(PathBuf, Result<RgbaImage, String>)> = Vec::new();
            std::thread::scope(|sc| {
                let hs: Vec<_> = (0..cores)
                    .map(|t| {
                        let files = &files;
                        sc.spawn(move || {
                            (t..files.len()).step_by(cores).map(|i| (files[i].clone(), image::open(&files[i]).map(|im| im.to_rgba8()).map_err(|e| e.to_string()))).collect::<Vec<_>>()
                        })
                    })
                    .collect();
                for h in hs {
                    decoded.extend(h.join().unwrap_or_default());
                }
            });
            for (p, r) in decoded {
                let img = match r {
                    Ok(i) => Some(Rc::new(i)),
                    Err(e) => {
                        self.warnings.push(format!("cannot load {}: {}", p.display(), e));
                        None
                    }
                };
                self.files.insert(p, img);
            }
        }
        let mut jobs: Vec<((String, u32, u32), Rc<RgbaImage>)> = Vec::new();
        for (name, w, h) in want {
            let key = (name.to_uppercase(), w.max(1), h.max(1));
            if self.scaled.contains_key(&key) || jobs.iter().any(|j| j.0 == key) {
                continue;
            }
            if let Some(src) = self.sprite(name) {
                if src.dimensions() != (key.1, key.2) {
                    jobs.push((key, src));
                }
            }
        }
        if jobs.len() < 2 {
            return; // one picture: `scaled` does it on this thread
        }
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get()).min(jobs.len());
        // the images are shared read-only with the workers (`Rc` itself
        // stays here); each worker scales every `cores`-th job
        let srcs: Vec<(&RgbaImage, u32, u32)> = jobs.iter().map(|(k, img)| (&**img, k.1, k.2)).collect();
        let mut results: Vec<Option<RgbaImage>> = (0..srcs.len()).map(|_| None).collect();
        std::thread::scope(|sc| {
            let chunks: Vec<_> = (0..cores)
                .map(|t| {
                    let srcs = &srcs;
                    sc.spawn(move || {
                        (t..srcs.len()).step_by(cores).map(|i| (i, image::imageops::resize(srcs[i].0, srcs[i].1, srcs[i].2, image::imageops::FilterType::Triangle))).collect::<Vec<_>>()
                    })
                })
                .collect();
            for c in chunks {
                for (i, img) in c.join().unwrap_or_default() {
                    results[i] = Some(img);
                }
            }
        });
        if self.scaled.len() + results.len() > 512 {
            self.scaled.clear();
        }
        for ((key, _), img) in jobs.into_iter().zip(results) {
            if let Some(img) = img {
                self.scaled.insert(key, Rc::new(img));
            }
        }
    }

    /// Sprite scaled to exactly w x h pixels (cached).
    pub fn scaled(&mut self, name: &str, w: u32, h: u32) -> Option<Rc<RgbaImage>> {
        let name = &self.frame_name(name);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anime_csv() {
        let d = std::env::temp_dir().join(format!("emuera-anime-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        RgbaImage::from_pixel(20, 10, image::Rgba([9, 9, 9, 255])).save(d.join("sheet.png")).unwrap();
        std::fs::write(d.join("a.csv"), "HALO,ANIME,10,10\nHALO,sheet.png,0,0,10,10,0,0,1000000\nHALO,sheet.png,10,0,10,10,2,3,50\nSTILL,sheet.png,0,0,5,5\n").unwrap();
        let mut r = Resources::load(Some(&d));
        assert!(r.exists("halo"));
        assert_eq!(r.anime["HALO"].frames.len(), 2);
        assert_eq!(r.sprite_size("HALO"), Some((10, 10)));
        // the first frame lasts long enough for the test
        assert_eq!(r.frame_name("halo"), "HALO#0");
        assert!(r.take_anim_used() && !r.take_anim_used());
        assert_eq!(r.sprites["HALO#1"].pos, (2, 3));
        assert_eq!(r.frame_name("STILL"), "STILL");
        assert!(r.sprite("halo").is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn packed_buffers_stay_exact() {
        // transparent runs, a coloured run, noise, an odd tail
        let mut img = RgbaImage::new(37, 23);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = match (x, y) {
                (_, 0..=4) => image::Rgba([0, 0, 0, 0]),
                (0..=19, 5..=9) => image::Rgba([200, 10, 10, 255]),
                (_, 5..=15) => image::Rgba([(x * 7 + y) as u8, (x ^ y) as u8, 3, 255]),
                _ => image::Rgba([1, 2, 3, (x % 3) as u8]),
            };
        }
        let mut r = Resources::default();
        r.graphics.insert(1, Graphic::new(img.clone()));
        r.graphics.insert(2, Graphic::new(RgbaImage::new(64, 64)));
        r.gbuf_budget = 0;
        r.trim_graphics();
        assert!(r.graphics.values().all(|g| !g.bytes().1), "both packed");
        assert_eq!(*r.graphics[&1].img(), img);
        assert!(r.graphics[&2].img().pixels().all(|p| p.0 == [0; 4]));
        // drawing into a packed buffer unpacks it first
        r.trim_graphics();
        r.graphics.get_mut(&1).unwrap().img_mut().put_pixel(0, 0, image::Rgba([5, 5, 5, 5]));
        img.put_pixel(0, 0, image::Rgba([5, 5, 5, 5]));
        assert_eq!(*r.graphics[&1].img(), img);
    }
}
