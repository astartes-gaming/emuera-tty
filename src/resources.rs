//! resources/*.csv sprite definitions and image loading/scaling cache.
//!
//! A game's `resources/` folder holds images plus CSVs naming sprites:
//! `NAME,file.png[,x,y,w,h]` (the rectangle cuts the sprite out of a sheet).
//! Images are decoded on first use and cached three ways: whole files,
//! cropped sprites, and sprites scaled to the size they are shown at.
//!
//! Whole files are kept within a budget (`EMUERA_PICTURES_MB`, 1/8 of the
//! memory), the least recently used dropped first: a portrait sheet can be
//! many MB while the parts cut from it (kept) are a fraction of that. And
//! the files a game decoded in its last sessions are decoded again in the
//! background when it starts (`start_warmup`), on the spare cores, so its
//! first screen full of portraits doesn't wait for them: decoding them one
//! by one as the script asked was most of a slow first screen (AnonTW's
//! whereabouts listing: 1.6 s cold, 12 ms once decoded).

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
    /// pictures that are Git LFS pointers, not images (a git copy of the
    /// game made without git-lfs), for the hint under a script error
    pub lfs_pointers: Vec<PathBuf>,
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
    /// bytes the cached parts take
    parts_bytes: usize,
    /// last fingerprint and copy of each buffer shown as a CBG layer
    cbg_cache: HashMap<i64, (u64, u64, Rc<RgbaImage>)>,
    /// animated sprites (`name,ANIME,w,h` in a resources CSV, or
    /// SPRITEANIMECREATE): their frames are sprites of their own
    pub anime: HashMap<String, Anime>,
    /// animations made by SPRITEANIMECREATE (the others come from CSV)
    runtime_anime: std::collections::HashSet<String>,
    /// SETANIMETIMER: how often the screen redraws for animations (ms, 0: off)
    pub anim_timer: i64,
    /// an animated sprite was drawn since the last [`Resources::take_anim_used`]
    anim_used: bool,
    /// the last files GCREATEFROMFILE decoded (path, size and time, pixels)
    loaded: Vec<(PathBuf, Option<(u64, std::time::SystemTime)>, Rc<RgbaImage>)>,
    /// when each whole file in `files` was last used (`file_tick`), for
    /// dropping the least recently used beyond `file_budget`
    file_used: HashMap<PathBuf, u64>,
    file_tick: u64,
    /// the bytes whole decoded files may take
    file_budget: usize,
    /// files being decoded in the background (see `start_warmup`)
    warm: Option<Warmup>,
    /// the files this session decoded, for the next warm-up
    record: Record,
}

/// The files a session decodes, oldest first, written to `path` (none: not
/// recorded) whenever the game waits for input after new ones (the game can
/// end without this being dropped).
#[derive(Default)]
struct Record {
    files: Vec<PathBuf>,
    path: Option<PathBuf>,
    dirty: bool,
}

impl Record {
    fn note(&mut self, p: &Path) {
        if self.path.is_none() {
            return;
        }
        if let Some(k) = self.files.iter().position(|q| q == p) {
            self.files.remove(k);
        }
        self.files.push(p.to_path_buf());
        self.dirty = true;
    }

    /// Write the list if it changed: this session's files, most recent
    /// first, then the earlier ones.
    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let Some(path) = self.path.clone() else { return };
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        // (absolute, but as the game names them: resolving symlinks would
        // give paths the game never asks for)
        let mut out: Vec<String> =
            self.files.iter().rev().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned()).collect();
        for l in old.lines() {
            if !l.is_empty() && !out.iter().any(|o| o == l) {
                out.push(l.to_string());
            }
        }
        out.truncate(RECORD_MAX);
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(&path, out.join("\n") + "\n");
        self.dirty = false;
    }
}

impl Drop for Record {
    fn drop(&mut self) {
        self.save();
    }
}

/// Files decoded in the background: results arrive on `rx`; `pending`
/// is what is still to come.
struct Warmup {
    rx: std::sync::mpsc::Receiver<(PathBuf, RgbaImage)>,
    pending: std::collections::HashSet<PathBuf>,
}

/// How many files the warm-up list keeps.
const RECORD_MAX: usize = 400;

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
    // four lanes, so the multiplies don't wait on each other (a whole
    // window's canvas is fingerprinted every frame a game redraws it)
    const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    let seed = 0xcbf29ce484222325 ^ ((img.width() as u64) << 32 | img.height() as u64);
    let mut h = [seed, seed ^ 1, seed ^ 2, seed ^ 3];
    let raw = img.as_raw();
    let mut blocks = raw.chunks_exact(32);
    for b in &mut blocks {
        for (k, l) in h.iter_mut().enumerate() {
            let v = u64::from_le_bytes(b[k * 8..k * 8 + 8].try_into().expect("8 bytes"));
            *l = (l.rotate_left(5) ^ v).wrapping_mul(K);
        }
    }
    let mut t = h[0] ^ h[1].rotate_left(17) ^ h[2].rotate_left(31) ^ h[3].rotate_left(47);
    for c in blocks.remainder().chunks(8) {
        let mut b = [0u8; 8];
        b[..c.len()].copy_from_slice(c);
        t = (t.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(K);
    }
    t
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

/// The key a decoded file is kept under: its absolute path as the game
/// names it (symlinks not resolved), so a relative path from a game
/// started in its own folder and the warm-up list's absolute one meet.
fn file_key(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
    }
}

/// Where the files a game decoded are listed for its next warm-up:
/// `<data>/emuera-tty/pictures/<game folder>-<hash of its path>.txt`.
fn record_file(game: &Path) -> Option<PathBuf> {
    let full = game.canonicalize().ok()?;
    let name = full.file_name()?.to_string_lossy().into_owned();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in full.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    Some(crate::sys::data_base()?.join("emuera-tty").join("pictures").join(format!("{}-{:08x}.txt", name, h as u32)))
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
    /// `GBUF_CLOCK` when the pixels were made or last written: unchanged,
    /// a buffer shown as a CBG needn't be fingerprinted again
    pub rev: u64,
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
            rev: GBUF_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
        self.rev = GBUF_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        self.rev = GBUF_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        for f in [
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Supplemental/Osaka.ttf",
            "/Library/Fonts/Osaka.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
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
    let proportional =
        n.contains("pgothic") || n.contains("pmincho") || n.contains("ｐゴシック") || n.contains("ｐ明朝") || n.contains("pゴシック") || n.contains("p明朝");
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
    let w: f32 =
        if fixed_pitch(name) { text.chars().map(|c| fixed_advance(c, size)).sum() } else { text.chars().map(|c| sf.h_advance(font.glyph_id(c))).sum() };
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

/// `src` scaled to `w × h` with the Triangle filter: the taps and weights
/// of image's `resize(…, FilterType::Triangle)` (its result within
/// rounding), in fixed point, rows shared out among the cores for big
/// pictures, many times faster: a game's whole-window canvas is scaled to
/// the screen on every frame, and every GDRAWSPRITE scales its sprite.
pub fn resize(src: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (sw, sh) = src.dimensions();
    if (w, h) == (sw, sh) || w == 0 || h == 0 || sw == 0 || sh == 0 {
        return if (w, h) == (sw, sh) { src.clone() } else { RgbaImage::new(w, h) };
    }
    let (sw, sh, w, h) = (sw as usize, sh as usize, w as usize, h as usize);
    let (vt, ht) = (triangle_taps(sh, h), triangle_taps(sw, w));
    let raw = src.as_raw();
    // vertical pass: sw × h, values × 256 (weights of 1 << 14, >> 6)
    let mut mid = vec![0u16; sw * 4 * h];
    par_rows(&mut mid, sw * 4, |y, row| {
        let (first, r) = &vt.0[y];
        for (k, wt) in vt.1[r.clone()].iter().enumerate() {
            let srow = &raw[(first + k) * sw * 4..][..sw * 4];
            for (m, &p) in row.iter_mut().zip(srow) {
                *m = (*m as u32 + ((p as u32 * *wt as u32) >> 6)) as u16;
            }
        }
    });
    // horizontal pass
    let mut out = vec![0u8; w * 4 * h];
    par_rows(&mut out, w * 4, |y, row| {
        let mrow = &mid[y * sw * 4..][..sw * 4];
        for (o, (first, r)) in row.chunks_exact_mut(4).zip(&ht.0) {
            let mut acc = [0u32; 4];
            for (k, wt) in ht.1[r.clone()].iter().enumerate() {
                let px = &mrow[(first + k) * 4..][..4];
                for c in 0..4 {
                    acc[c] += px[c] as u32 * *wt as u32;
                }
            }
            for c in 0..4 {
                o[c] = ((acc[c] + (1 << 21)) >> 22).min(255) as u8;
            }
        }
    });
    RgbaImage::from_raw(w as u32, h as u32, out).expect("size matches")
}

/// For each of `d` output pixels from `s` source ones: the first source
/// pixel and the range of its Triangle filter weights (summing to 1 << 14)
/// in the returned list, as image's `horizontal_sample` /
/// `vertical_sample` choose them.
fn triangle_taps(s: usize, d: usize) -> (Vec<(usize, std::ops::Range<usize>)>, Vec<u16>) {
    let ratio = s as f32 / d as f32;
    let sratio = ratio.max(1.0);
    let mut taps = Vec::with_capacity(d);
    let mut ws: Vec<u16> = Vec::with_capacity(d * (2 * sratio.ceil() as usize + 1));
    let mut fw: Vec<f32> = Vec::new();
    for o in 0..d {
        let centre = (o as f32 + 0.5) * ratio;
        let left = ((centre - sratio).floor() as i64).clamp(0, s as i64 - 1) as usize;
        let right = ((centre + sratio).ceil() as i64).clamp(left as i64 + 1, s as i64) as usize;
        fw.clear();
        fw.extend((left..right).map(|i| (1.0 - ((i as f32 - (centre - 0.5)) / sratio).abs()).max(0.0)));
        let sum: f32 = fw.iter().sum::<f32>().max(f32::MIN_POSITIVE);
        let start = ws.len();
        ws.extend(fw.iter().map(|x| (x / sum * 16384.0).round() as u16));
        // exact sum: the rounding left over goes to the heaviest tap
        let mine = &mut ws[start..];
        let total: i32 = mine.iter().map(|&x| x as i32).sum();
        if let Some(k) = (0..mine.len()).max_by_key(|&k| mine[k]) {
            mine[k] = (mine[k] as i32 + 16384 - total).max(0) as u16;
        }
        taps.push((left, start..ws.len()));
    }
    (taps, ws)
}

/// Run `f(row index, row)` over `buf`'s rows of `stride` elements, on up
/// to four cores when there are many.
fn par_rows<T: Send>(buf: &mut [T], stride: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = buf.len() / stride.max(1);
    let threads = if std::mem::size_of_val(buf) >= 1 << 20 { std::thread::available_parallelism().map_or(1, |n| n.get()).min(4) } else { 1 };
    if threads <= 1 || rows < 2 {
        for (y, row) in buf.chunks_exact_mut(stride).enumerate() {
            f(y, row);
        }
        return;
    }
    let per = rows.div_ceil(threads);
    std::thread::scope(|sc| {
        for (t, chunk) in buf.chunks_mut(per * stride).enumerate() {
            let f = &f;
            sc.spawn(move || {
                for (k, row) in chunk.chunks_exact_mut(stride).enumerate() {
                    f(t * per + k, row);
                }
            });
        }
    });
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
    let mut scaled = if (w, h) == (x1 - x0, y1 - y0) { view.to_image() } else { resize(&view.to_image(), w, h) };
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
    let (pw, ph) = (part.width() as i32, part.height() as i32);
    // the part's columns and rows that land inside `dst`
    let (x0, x1) = ((-ox).max(0), (dwid - ox).min(pw));
    let (y0, y1) = ((-oy).max(0), (dhei - oy).min(ph));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let (dstride, pstride) = (dwid as usize * 4, pw as usize * 4);
    let (praw, draw) = (part.as_raw(), &mut **dst);
    for y in y0..y1 {
        let prow = &praw[y as usize * pstride..][x0 as usize * 4..x1 as usize * 4];
        let drow = &mut draw[(oy + y) as usize * dstride..][(ox + x0) as usize * 4..(ox + x1) as usize * 4];
        for (d, p) in drow.chunks_exact_mut(4).zip(prow.chunks_exact(4)) {
            let px = match cm {
                Some(m) => color_matrix([p[0], p[1], p[2], p[3]], m),
                None => [p[0], p[1], p[2], p[3]],
            };
            match px[3] {
                0 => {}
                255 => d.copy_from_slice(&px),
                _ => {
                    let mut q = [d[0], d[1], d[2], d[3]];
                    over(&mut q, px);
                    d.copy_from_slice(&q);
                }
            }
        }
    }
}

impl Resources {
    /// Read every sprite CSV under `dir` (images are not decoded yet).
    pub fn load(dir: Option<&Path>) -> Resources {
        let mut r = Resources { gbuf_budget: gbuf_budget(), file_budget: crate::sys::memory_budget("EMUERA_PICTURES_MB", 8), ..Resources::default() };
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
        self.cbg.sort_by_key(|l| std::cmp::Reverse(l.z));
    }

    /// The pixels a CBG source shows now, and a fingerprint of them (so an
    /// unchanged picture isn't sent to the terminal again).
    pub fn cbg_pixels(&mut self, src: &CbgSrc) -> Option<(Rc<RgbaImage>, u64)> {
        match src {
            CbgSrc::G(gid) => {
                let g = self.graphics.get(gid)?;
                if let Some((rev, h, img)) = self.cbg_cache.get(gid) {
                    if *rev == g.rev {
                        return Some((img.clone(), *h));
                    }
                }
                let img = g.img();
                let h = fingerprint(&img);
                let rev = g.rev;
                if let Some((r, k, old)) = self.cbg_cache.get_mut(gid) {
                    if *k == h {
                        *r = rev;
                        return Some((old.clone(), h));
                    }
                }
                let img = Rc::new(img.clone());
                self.cbg_cache.insert(*gid, (rev, h, img.clone()));
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
    /// Drop every cached picture decoded from files (sprites, crops, scaled
    /// copies, prepared parts, CBG copies): they decode again when drawn.
    /// Graphics buffers made by the game (GCREATE…) are its state and stay.
    pub fn release_images(&mut self) {
        self.files = HashMap::default();
        self.file_used = HashMap::default();
        self.cropped = HashMap::default();
        self.scaled = HashMap::default();
        self.parts = HashMap::default();
        self.parts_bytes = 0;
        self.cbg_cache = HashMap::default();
        self.loaded = Vec::new();
        self.gen += 1;
        crate::sys::trim_memory();
    }

    /// Keep the unpacked graphics buffers within [`Resources::gbuf_budget`]:
    /// pack the least recently used ones (they unpack when used again).
    pub fn trim_graphics(&mut self) {
        self.take_warm(None);
        self.record.save();
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

    /// Pictures held, for the memory report, in two groups of (what, how
    /// many, bytes): what the game made (graphics buffers from GCREATE…, and
    /// the files GCREATEFROMFILE kept: its state, held whatever the Images
    /// setting says), and the sprite pictures decoded for the screen (not
    /// loaded while Images is none, and freed when it is set to none).
    #[allow(clippy::type_complexity)]
    pub fn mem_stats(&self) -> (Vec<(&'static str, usize, usize)>, Vec<(&'static str, usize, usize)>) {
        let px = |i: &RgbaImage| i.as_raw().capacity();
        let packed: Vec<usize> = self.graphics.values().map(|g| g.bytes()).filter(|b| !b.1).map(|b| b.0).collect();
        let game = vec![
            ("graphics buffers (GCREATE…)", self.graphics.len(), self.graphics.values().map(|g| g.bytes().0).sum()),
            ("  of them packed", packed.len(), packed.iter().sum()),
            ("files kept for buffers (GCREATEFROMFILE)", self.loaded.len(), self.loaded.iter().map(|e| px(&e.2)).sum()),
        ];
        let sprites = vec![
            ("decoded image files", self.files.len(), self.files.values().flatten().map(|i| px(i)).sum()),
            ("cut sprites", self.cropped.len(), self.cropped.values().flatten().map(|i| px(i)).sum()),
            ("scaled sprites", self.scaled.len(), self.scaled.values().map(|i| px(i)).sum()),
            ("prepared draws", self.parts.len(), self.parts.values().map(|i| px(i)).sum()),
            ("background copies", self.cbg_cache.len(), self.cbg_cache.values().map(|(_, _, i)| px(i)).sum()),
        ];
        (game, sprites)
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
        self.anime.insert(k.clone(), Anime { size: (w, h), frames: Vec::new() });
        self.runtime_anime.insert(k);
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
    pub fn draw_cached(
        &mut self,
        dst: i64,
        key: &str,
        version: u64,
        src: PartSrc,
        dest: (i32, i32, i32, i32),
        srect: (i32, i32, i32, i32),
        cm: Option<&[[f32; 5]; 5]>,
    ) {
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
                // bounded by size, not count: a bar chart redrawn every
                // frame draws one sprite at a hundred sizes
                let cap = (self.gbuf_budget / 4).clamp(4 << 20, 32 << 20);
                if self.parts_bytes + p.as_raw().len() > cap || self.parts.len() >= 4096 {
                    self.parts.clear();
                    self.parts_bytes = 0;
                }
                self.parts_bytes += p.as_raw().len();
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

    /// A sprite drawn at its base position inside its own size, when it
    /// has one: (x, y, width, height of that size).
    fn placement(&mut self, name: &str) -> Option<(i64, i64, i64, i64)> {
        let (px, py) = self.base_pos(name);
        if (px, py) == (0, 0) {
            return None;
        }
        let (dw, dh) = self.sprite_size(name)?;
        (dw > 0 && dh > 0).then_some((px, py, dw as i64, dh as i64))
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
    /// SPRITEDISPOSEALL: the sprites made while playing (SPRITECREATE,
    /// SPRITEANIMECREATE) go, and with `csv_too` those from the CSV files
    /// as well; how many went (an animation counts once, as in Emuera).
    pub fn dispose_all_sprites(&mut self, csv_too: bool) -> i64 {
        let frame = |k: &str| k.contains('#');
        let runtime = self.g_sprites.keys().filter(|k| !frame(k)).count() + self.runtime_anime.len();
        let csv = self.sprites.keys().filter(|k| !frame(k)).count() + self.anime.len() - self.runtime_anime.len();
        self.g_sprites.clear();
        for k in std::mem::take(&mut self.runtime_anime) {
            self.anime.remove(&k);
        }
        if csv_too {
            self.sprites.clear();
            self.anime.clear();
            self.pos_override.clear();
            self.cropped.clear();
            self.scaled.clear();
        }
        self.changed();
        (runtime + if csv_too { csv } else { 0 }) as i64
    }

    pub fn dispose_sprite(&mut self, name: &str) -> bool {
        let gone = self.g_sprites.remove(&name.to_uppercase()).is_some();
        if gone {
            self.changed();
        }
        gone
    }

    /// Note a picture that can't be decoded (and whether it is a Git LFS
    /// pointer: a few lines of text standing in for the picture).
    fn cannot_load(&mut self, p: &Path, e: &dyn std::fmt::Display) {
        if is_lfs_pointer(p) {
            if self.lfs_pointers.len() < 8 {
                self.lfs_pointers.push(p.to_path_buf());
            }
            self.warnings.push(format!("cannot load {}: a Git LFS pointer, not the picture (git-lfs missing?)", p.display()));
        } else {
            self.warnings.push(format!("cannot load {}: {}", p.display(), e));
        }
    }

    /// Decode an image file once; failures are remembered as None.
    fn file(&mut self, p: &Path) -> Option<Rc<RgbaImage>> {
        let p = &file_key(p);
        if !self.files.contains_key(p) {
            // (being decoded in the background: wait for it, not twice)
            self.take_warm(Some(p));
        }
        if let Some(v) = self.files.get(p).cloned() {
            self.touch(p);
            return v;
        }
        let img = match image::open(p) {
            Ok(i) => Some(Rc::new(i.to_rgba8())),
            Err(e) => {
                self.cannot_load(p, &e);
                None
            }
        };
        self.insert_file(p.to_path_buf(), img.clone());
        img
    }

    /// Mark a whole file as just used.
    fn touch(&mut self, p: &Path) {
        self.file_tick += 1;
        self.file_used.insert(p.to_path_buf(), self.file_tick);
    }

    /// Keep a decoded file, note it for the next warm-up, and drop the
    /// least recently used files beyond the budget (the parts cut from
    /// them stay cached; a file needed again is decoded again).
    fn insert_file(&mut self, p: PathBuf, img: Option<Rc<RgbaImage>>) {
        let p = file_key(&p);
        if img.is_some() {
            self.record.note(&p);
        }
        self.touch(&p);
        self.files.insert(p.clone(), img);
        let size = |i: &Option<Rc<RgbaImage>>| i.as_ref().map_or(0, |i| i.as_raw().len());
        let mut total: usize = self.files.values().map(size).sum();
        if total <= self.file_budget {
            return;
        }
        let mut lru: Vec<(u64, PathBuf)> = self.file_used.iter().filter(|(q, _)| **q != p).map(|(q, t)| (*t, q.clone())).collect();
        lru.sort_unstable();
        for (_, q) in lru {
            if total <= self.file_budget {
                break;
            }
            total -= self.files.remove(&q).as_ref().map_or(0, size);
            self.file_used.remove(&q);
        }
    }

    /// Take the files the background warm-up has decoded; with `want`,
    /// wait until that one has come if it is still on its way.
    fn take_warm(&mut self, want: Option<&Path>) {
        let Some(w) = self.warm.as_mut() else { return };
        let mut got = Vec::new();
        let wait = want.is_some_and(|p| w.pending.contains(p));
        loop {
            let r = if wait && want.is_some_and(|p| w.pending.contains(p)) { w.rx.recv().ok() } else { w.rx.try_recv().ok() };
            let Some((p, img)) = r else { break };
            w.pending.remove(&p);
            got.push((p, img));
        }
        // (a closed channel: the warm-up is over, whatever it didn't decode)
        if wait && want.is_some_and(|p| w.pending.contains(p)) {
            w.pending.clear();
        }
        if w.pending.is_empty() && got.is_empty() && matches!(w.rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)) {
            self.warm = None;
        }
        for (p, img) in got {
            if !self.files.contains_key(&p) {
                self.insert_file(p, Some(Rc::new(img)));
            }
        }
    }

    /// Start decoding, in the background, the files `game` decoded in its
    /// last sessions (most recent first), within three quarters of the
    /// file budget, on the cores but one (up to 8); and record this
    /// session's for the next start.
    pub fn start_warmup(&mut self, game: &Path) {
        let Some(path) = record_file(game) else { return };
        let list: Vec<PathBuf> = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_file() && !self.files.contains_key(p))
            .collect();
        self.record.path = Some(path);
        if list.is_empty() {
            return;
        }
        let budget = self.file_budget / 4 * 3;
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).clamp(1, 8);
        let (tx, rx) = std::sync::mpsc::channel();
        self.warm = Some(Warmup { rx, pending: list.iter().cloned().collect() });
        let spawned = std::thread::Builder::new().name("warmup".into()).spawn(move || {
            use std::sync::atomic::{AtomicUsize, Ordering};
            let (next, bytes) = (AtomicUsize::new(0), AtomicUsize::new(0));
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    let (tx, list, next, bytes) = (tx.clone(), &list, &next, &bytes);
                    sc.spawn(move || loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= list.len() || bytes.load(Ordering::Relaxed) >= budget {
                            return;
                        }
                        if let Ok(img) = image::open(&list[i]) {
                            let img = img.to_rgba8();
                            bytes.fetch_add(img.as_raw().len(), Ordering::Relaxed);
                            if tx.send((list[i].clone(), img)).is_err() {
                                return;
                            }
                        }
                    });
                }
            });
        });
        if spawned.is_err() {
            self.warm = None;
        }
    }

    /// GCREATEFROMFILE's pixels. The last few files decoded are kept (at
    /// most 16 MB, less on small machines): games load one picture into
    /// many buffers (pops-tw its placeholder portrait six times for each
    /// character), and decoding it again each time was most of the cost.
    pub fn load_for_buffer(&mut self, p: &Path) -> Option<RgbaImage> {
        let stamp = std::fs::metadata(p).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
        if let Some(k) = self.loaded.iter().position(|(q, s, _)| q == p && *s == stamp && stamp.is_some()) {
            let e = self.loaded.remove(k);
            let img = (*e.2).clone();
            self.loaded.push(e);
            return Some(img);
        }
        let img = match image::open(p) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                self.cannot_load(p, &e);
                return None;
            }
        };
        let cap = (16usize << 20).min(self.gbuf_budget / 8);
        if stamp.is_some() && img.as_raw().len() <= cap / 2 {
            self.loaded.push((p.to_path_buf(), stamp, Rc::new(img.clone())));
            while self.loaded.len() > 4 || self.loaded.iter().map(|e| e.2.as_raw().len()).sum::<usize>() > cap {
                self.loaded.remove(0);
            }
        }
        Some(img)
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
            if !self.files.contains_key(&file_key(&sp.file)) {
                if let Some(d) = self.dims.get(&sp.file) {
                    return Some(*d);
                }
                if let Ok(d) = image::ImageReader::open(&sp.file)
                    .and_then(|r| r.with_guessed_format())
                    .map_err(image::ImageError::IoError)
                    .and_then(|r| r.into_dimensions())
                {
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
                if !self.files.contains_key(&file_key(&sp.file)) && !files.contains(&sp.file) {
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
                            (t..files.len())
                                .step_by(cores)
                                .map(|i| (files[i].clone(), image::open(&files[i]).map(|im| im.to_rgba8()).map_err(|e| e.to_string())))
                                .collect::<Vec<_>>()
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
                        self.cannot_load(&p, &e);
                        None
                    }
                };
                self.insert_file(p, img);
            }
        }
        let mut jobs: Vec<((String, u32, u32), Rc<RgbaImage>)> = Vec::new();
        for (name, w, h) in want {
            let key = (name.to_uppercase(), w.max(1), h.max(1));
            // (placed pieces: `scaled` puts them on their canvas)
            if self.scaled.contains_key(&key) || jobs.iter().any(|j| j.0 == key) || self.placement(name).is_some() {
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
                    sc.spawn(move || (t..srcs.len()).step_by(cores).map(|i| (i, resize(srcs[i].0, srcs[i].1, srcs[i].2))).collect::<Vec<_>>())
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
        let mut key = (name.to_uppercase(), w.max(1), h.max(1));
        // (SPRITEMOVE moves a placed piece: a picture of its own)
        if let Some((px, py, _, _)) = self.placement(name) {
            key.0 = format!("{}@{},{}", key.0, px, py);
        }
        if let Some(i) = self.scaled.get(&key) {
            return Some(i.clone());
        }
        let src = self.sprite(name)?;
        let out = match self.placement(name) {
            // a piece with a base position sits at it inside its own size,
            // both scaled to w x h (Emuera's SpriteF.GraphicsDraw: EagloV's
            // speech bubbles are 67x56 pieces in a 300x300 portrait)
            Some((px, py, dw, dh)) => {
                let (w, h) = (key.1 as i64, key.2 as i64);
                let (pw, ph) = ((w * src.width() as i64 / dw).max(1) as u32, (h * src.height() as i64 / dh).max(1) as u32);
                let piece = resize(&src, pw, ph);
                let mut canvas = RgbaImage::new(key.1, key.2);
                image::imageops::overlay(&mut canvas, &piece, px * w / dw, py * h / dh);
                Rc::new(canvas)
            }
            None => Rc::new(resize(&src, key.1, key.2)),
        };
        if self.scaled.len() > 512 {
            self.scaled.clear();
        }
        self.scaled.insert(key, out.clone());
        Some(out)
    }
}

/// `base/rel` matching each path component case-insensitively and
/// accepting `\` separators, since games are made on Windows.
pub(crate) fn find_ci(base: &Path, rel: &str) -> PathBuf {
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

/// Is `p` a Git LFS pointer (the text git leaves in place of a large file
/// when git-lfs didn't fetch it)?
pub fn is_lfs_pointer(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 40];
    let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    head[..n].starts_with(b"version https://git-lfs.github.com/spec/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed-point Triangle filter gives what image's gives, within
    /// rounding, enlarging and shrinking; and blit_part clips.
    #[test]
    fn fast_resize_matches_triangle() {
        let src =
            RgbaImage::from_fn(37, 23, |x, y| image::Rgba([(x * 7) as u8, (y * 11) as u8, ((x * y) % 256) as u8, if (x + y) % 5 == 0 { 0 } else { 255 }]));
        for (w, h) in [(80, 50), (34, 21), (37, 40), (25, 16), (11, 90), (5, 3), (37, 23)] {
            let a = resize(&src, w, h);
            let b = image::imageops::resize(&src, w, h, image::imageops::FilterType::Triangle);
            let worst = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap();
            assert!(worst <= 2, "{}x{}: off by {}", w, h, worst);
        }
        let mut dst = RgbaImage::new(10, 10);
        blit_part(&mut dst, &RgbaImage::from_pixel(4, 4, image::Rgba([9, 8, 7, 255])), (-2, 8, 4, 4), None);
        assert_eq!(dst.get_pixel(0, 9).0, [9, 8, 7, 255]);
        assert_eq!(dst.get_pixel(2, 9).0, [0, 0, 0, 0]);
        assert_eq!(dst.get_pixel(0, 7).0, [0, 0, 0, 0]);
    }

    #[test]
    fn lfs_pointer() {
        let d = std::env::temp_dir().join(format!("emuera-lfs-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("t.webp"), "version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 9\n").unwrap();
        RgbaImage::from_pixel(2, 2, image::Rgba([9, 9, 9, 255])).save(d.join("ok.png")).unwrap();
        assert!(is_lfs_pointer(&d.join("t.webp")) && !is_lfs_pointer(&d.join("ok.png")));
        let mut r = Resources::load(Some(&d));
        assert!(r.load_for_buffer(&d.join("t.webp")).is_none());
        assert_eq!(r.lfs_pointers, vec![d.join("t.webp")]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn anime_csv() {
        let d = std::env::temp_dir().join(format!("emuera-anime-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        RgbaImage::from_pixel(20, 10, image::Rgba([9, 9, 9, 255])).save(d.join("sheet.png")).unwrap();
        std::fs::write(d.join("a.csv"), "HALO,ANIME,10,10\nHALO,sheet.png,0,0,10,10,0,0,1000000\nHALO,sheet.png,10,0,10,10,2,3,50\nSTILL,sheet.png,0,0,5,5\n")
            .unwrap();
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
    fn placed_piece_keeps_its_place() {
        // EagloV: a 10x5 bubble at (20, 2) of a 40x40 portrait
        let d = std::env::temp_dir().join(format!("emuera-placed-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        RgbaImage::from_pixel(10, 5, image::Rgba([9, 9, 9, 255])).save(d.join("sheet.png")).unwrap();
        std::fs::write(d.join("a.csv"), "BUBBLE,sheet.png,0,0,10,5,20,2,,40,40\nPLAIN,sheet.png,0,0,10,5\n").unwrap();
        let mut r = Resources::load(Some(&d));
        let img = r.scaled("BUBBLE", 80, 80).unwrap();
        assert_eq!(img.dimensions(), (80, 80));
        // doubled: the piece covers (40..60, 4..14), nothing else
        assert_eq!(img.get_pixel(45, 8).0[3], 255);
        assert_eq!(img.get_pixel(10, 8).0[3], 0);
        assert_eq!(img.get_pixel(45, 30).0[3], 0);
        // no base position: stretched to the size asked
        assert_eq!(r.scaled("PLAIN", 20, 20).unwrap().get_pixel(1, 18).0[3], 255);
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

    /// Whole decoded files are kept within their budget, the least
    /// recently used dropped first; using one keeps it.
    #[test]
    fn file_cache_budget() {
        let mut r = Resources::default();
        let img = || Some(Rc::new(RgbaImage::new(10, 10))); // 400 bytes each
        r.file_budget = 800;
        let (a, b, c) = (PathBuf::from("/x/a.png"), PathBuf::from("/x/b.png"), PathBuf::from("/x/c.png"));
        r.insert_file(a.clone(), img());
        r.insert_file(b.clone(), img());
        r.touch(&a); // a used after b
        r.insert_file(c.clone(), img());
        assert!(r.files.contains_key(&a) && r.files.contains_key(&c), "{:?}", r.files.keys().collect::<Vec<_>>());
        assert!(!r.files.contains_key(&b), "b was the least recently used");
        // a relative path and its absolute form are the same entry
        let rel = PathBuf::from("rel-test.png");
        r.insert_file(rel.clone(), img());
        assert!(r.files.contains_key(&file_key(&rel)) && file_key(&rel).is_absolute());
    }
}
