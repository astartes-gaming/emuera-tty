//! Fonts a game ships in its `font/` folder (Emuera.NET's
//! FontFactory.LoadFontFolder): every `.ttf` / `.otf` there, known by its
//! family name and its file name. Text drawn into graphics buffers uses
//! them (`resources::font_for`), and in the TUI the characters only such a
//! font has (private-use code points: eraMegatenP's dungeon tiles in
//! EraMTFont) are drawn as pictures of their glyphs (`glyph_picture`),
//! since the terminal's own font doesn't have them.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use image::RgbaImage;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// One font of the folder: the names it answers to (family names, file
/// name), and the font itself, read when first asked for (a font can be
/// many MB: eraMegatenP's is 14).
struct GameFont {
    names: Vec<String>,
    path: std::path::PathBuf,
    font: OnceLock<Option<Arc<FontVec>>>,
}

static FONTS: OnceLock<Vec<GameFont>> = OnceLock::new();

/// Read the game's `font/` folder (its subfolders too, as Emuera.NET).
/// Once per run; a font that doesn't parse is skipped.
pub fn load(game_dir: &Path) {
    let _ = FONTS.set(read_folder(game_dir));
}

fn read_folder(game_dir: &Path) -> Vec<GameFont> {
    let Some(dir) = crate::loader::find_dir_ci(game_dir, "font") else { return Vec::new() };
    let mut out = Vec::new();
    for path in crate::loader::walk(&dir, &["ttf", "otf"]) {
        let Ok(data) = std::fs::read(&path) else { continue };
        let mut names = family_names(&data);
        if let Some(f) = path.file_name() {
            names.push(f.to_string_lossy().into_owned());
        }
        out.push(GameFont { names, path, font: OnceLock::new() });
    }
    out
}

/// The game's font named `name` (a family or file name, any letter case).
pub fn find(name: &str) -> Option<Arc<FontVec>> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let f = FONTS.get()?.iter().find(|f| f.names.iter().any(|n| n.eq_ignore_ascii_case(name)))?;
    f.font.get_or_init(|| std::fs::read(&f.path).ok().and_then(|d| FontVec::try_from_vec(d).ok()).map(Arc::new)).clone()
}

/// A private-use code point: a font's own symbols, which no other font
/// draws the same way.
pub fn is_private_use(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

/// Does `font` have a glyph of its own for `ch`?
pub fn has_glyph(font: &FontVec, ch: char) -> bool {
    font.glyph_id(ch).0 != 0
}

/// How many half-width cells `ch` takes in `font`: its advance against
/// half an em, 1 or 2 (as Emuera.NET measures text with the font itself).
pub fn cells(font: &FontVec, ch: char) -> usize {
    let scaled = font.as_scaled(PxScale::from(100.0));
    let advance = scaled.h_advance(font.glyph_id(ch));
    if advance > 75.0 {
        2
    } else {
        1
    }
}

/// `ch` drawn from `font` in colour `rgb`, `cells` half-width cells wide:
/// the em fills the picture's height, the glyph sits on the font's
/// baseline. The picture is scaled to the cells it covers when shown.
pub fn glyph_picture(font: &FontVec, ch: char, rgb: (u8, u8, u8), cells: usize) -> RgbaImage {
    const EM: f32 = 64.0;
    let (w, h) = ((cells.max(1) as f32 * EM / 2.0) as u32, EM as u32);
    let mut img = RgbaImage::new(w, h);
    let scaled = font.as_scaled(PxScale::from(EM));
    // the em box: ascent and descent split the height as the font does
    let (ascent, descent) = (scaled.ascent(), -scaled.descent());
    let baseline = EM * ascent / (ascent + descent).max(1.0);
    let glyph = font.glyph_id(ch).with_scale_and_position(PxScale::from(EM), ab_glyph::point(0.0, baseline));
    if let Some(outline) = font.outline_glyph(glyph) {
        let b = outline.px_bounds();
        outline.draw(|x, y, coverage| {
            let (px, py) = (b.min.x as i32 + x as i32, b.min.y as i32 + y as i32);
            if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                let a = (coverage.clamp(0.0, 1.0) * 255.0) as u8;
                let p = img.get_pixel_mut(px as u32, py as u32);
                if a > p.0[3] {
                    *p = image::Rgba([rgb.0, rgb.1, rgb.2, a]);
                }
            }
        });
    }
    img
}

/// The picture name a glyph of `font_name` goes by in the picture
/// pipeline (`Resources::sprite` makes it with [`picture_for`]).
pub fn picture_name(font_name: &str, ch: char, rgb: (u8, u8, u8), cells: usize) -> String {
    format!("{}{:X}|{:02X}{:02X}{:02X}|{}|{}", PICTURE_PREFIX, ch as u32, rgb.0, rgb.1, rgb.2, cells, font_name)
}

/// What every [`picture_name`] starts with (no resource name has it).
pub const PICTURE_PREFIX: &str = "\u{1}GLYPH|";

/// The glyph picture a [`picture_name`] stands for (the name upper-cased
/// or not), or None if it isn't one or its font is gone.
pub fn picture_for(name: &str) -> Option<RgbaImage> {
    let rest = name.get(PICTURE_PREFIX.len()..).filter(|_| name.to_uppercase().starts_with(PICTURE_PREFIX))?;
    let mut parts = rest.splitn(4, '|');
    let ch = char::from_u32(u32::from_str_radix(parts.next()?, 16).ok()?)?;
    let rgb = u32::from_str_radix(parts.next()?, 16).ok()?;
    let cells: usize = parts.next()?.parse().ok()?;
    let font = find(parts.next()?)?;
    Some(glyph_picture(&font, ch, ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), cells.clamp(1, 2)))
}

/// The family names in a font's `name` table (IDs 1 and 16), as the
/// font's own records give them (UTF-16 for Windows and Unicode
/// platforms, single bytes for Macintosh).
fn family_names(data: &[u8]) -> Vec<String> {
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as usize);
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let mut out = Vec::new();
    let Some(n_tables) = u16_at(4) else { return out };
    let table = (0..n_tables).map(|k| 12 + 16 * k).find(|&rec| data.get(rec..rec + 4) == Some(b"name".as_slice()));
    let Some(base) = table.and_then(|rec| u32_at(rec + 8)) else { return out };
    let (Some(count), Some(strings)) = (u16_at(base + 2), u16_at(base + 4)) else { return out };
    for k in 0..count {
        let rec = base + 6 + 12 * k;
        let (Some(platform), Some(name_id), Some(len), Some(off)) = (u16_at(rec), u16_at(rec + 6), u16_at(rec + 8), u16_at(rec + 10)) else { break };
        if name_id != 1 && name_id != 16 {
            continue;
        }
        let Some(bytes) = data.get(base + strings + off..base + strings + off + len) else { continue };
        let name = if platform == 1 {
            bytes.iter().map(|&b| b as char).collect()
        } else {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        };
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eraMegatenP's EraMTFont, when that game is on this machine: its
    /// family name, its tile glyphs and their width.
    #[test]
    fn eramegatenp_font() {
        let p = &std::path::Path::new(&std::env::var("EMUERA_TEST_GAMES").unwrap_or_default()).join("eraMegatenP/Data/font/EraMTFontMedium.ttf");
        let Ok(data) = std::fs::read(p) else {
            eprintln!("skipped: {} not here", p.display());
            return;
        };
        let names = family_names(&data);
        assert!(names.iter().any(|n| n == "EraMTFont"), "{names:?}");
        let font = FontVec::try_from_vec(data).unwrap();
        assert!(has_glyph(&font, '\u{F003}'));
        let pic = glyph_picture(&font, '\u{F003}', (200, 100, 0), cells(&font, '\u{F003}'));
        assert!(pic.pixels().any(|p| p.0[3] > 0), "the glyph drew nothing");
        // through its picture name, as the TUI asks for it
        let _ = FONTS.set(vec![GameFont { names: vec!["EraMTFont".into()], path: p.to_path_buf(), font: OnceLock::from(Some(Arc::new(font))) }]);
        let name = picture_name("EraMTFont", '\u{F003}', (200, 100, 0), 2);
        let again = picture_for(&name.to_uppercase()).expect("made from its name");
        assert_eq!(again.dimensions(), (64, 64));
    }
}
