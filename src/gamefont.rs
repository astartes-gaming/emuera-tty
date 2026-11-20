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

/// Is `ch` drawn from the game's font as a picture (where pictures show)?
/// Its private-use symbols, which no other font has, and the characters
/// games draw grids with: box drawing, block elements and geometric shapes
/// (U+2500-25FF). Their size and shape decide whether a map's tiles meet:
/// eraMegatenP's dungeon floor is `□`, EraMTFont's fills its two cells,
/// a terminal font's is a small box with a gap. Other text stays text.
pub fn draws_as_picture(ch: char) -> bool {
    is_private_use(ch) || matches!(ch as u32, 0x2500..=0x25FF)
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

/// `ch` drawn from `font` as Braille dots, one Braille character per
/// half-width cell (`cells` of them): for a terminal without pictures,
/// where the game's own symbols (eraMegatenP's map tiles) would otherwise
/// come out as whatever the terminal's font has at that private-use code,
/// one cell wide, and pull the rest of the row out of line. Each cell is
/// 2 dots across and 4 down. A dot is set where the glyph fills most of
/// its patch, which keeps a thick wall a line rather than a solid block;
/// a glyph drawn in thin strokes (a door's outline) fills no patch that
/// much, so it is drawn again with a dot wherever it covers a fifth.
pub fn braille(font: &FontVec, ch: char, cells: usize) -> Vec<char> {
    let cells = cells.max(1);
    let img = glyph_picture(font, ch, (255, 255, 255), cells);
    let (cell_w, h) = (img.width() / cells as u32, img.height());
    let (patch_w, patch_h) = ((cell_w / 2).max(1), (h / 4).max(1));
    // how much of each patch the glyph covers (0..=1), cell by cell
    let cover = |c: u32, dx: u32, dy: u32| -> f32 {
        let (x0, y0) = (c * cell_w + dx * patch_w, dy * patch_h);
        let mut sum = 0u64;
        for y in y0..(y0 + patch_h).min(h) {
            for x in x0..(x0 + patch_w).min(img.width()) {
                sum += img.get_pixel(x, y).0[3] as u64;
            }
        }
        sum as f32 / (255 * patch_w * patch_h) as f32
    };
    let patches: Vec<[[f32; 4]; 2]> = (0..cells as u32).map(|c| [0, 1].map(|dx| [0, 1, 2, 3].map(|dy| cover(c, dx, dy)))).collect();
    // the dot bits of U+2800, by (column, row)
    const BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
    let dots = |threshold: f32| -> Vec<u32> {
        patches
            .iter()
            .map(|p| {
                let mut bits = 0;
                for dx in 0..2 {
                    for dy in 0..4 {
                        if p[dx][dy] >= threshold {
                            bits |= BITS[dx][dy];
                        }
                    }
                }
                bits
            })
            .collect()
    };
    let mut bits = dots(0.7);
    if bits.iter().map(|b| b.count_ones()).sum::<u32>() < 3 {
        bits = dots(0.2);
    }
    bits.into_iter().map(|b| char::from_u32(0x2800 + b).unwrap_or(' ')).collect()
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
        // the dungeon floor: a square filling two cells
        assert!(draws_as_picture('□') && has_glyph(&font, '□') && cells(&font, '□') == 2);
        assert!(!draws_as_picture('あ') && !draws_as_picture('♡'));
        let pic = glyph_picture(&font, '\u{F003}', (200, 100, 0), cells(&font, '\u{F003}'));
        assert!(pic.pixels().any(|p| p.0[3] > 0), "the glyph drew nothing");
        // through its picture name, as the TUI asks for it
        let _ = FONTS.set(vec![GameFont { names: vec!["EraMTFont".into()], path: p.to_path_buf(), font: OnceLock::from(Some(Arc::new(font))) }]);
        let name = picture_name("EraMTFont", '\u{F003}', (200, 100, 0), 2);
        let again = picture_for(&name.to_uppercase()).expect("made from its name");
        assert_eq!(again.dimensions(), (64, 64));
    }
}

#[cfg(test)]
mod braille_tests {
    /// eraMegatenP's EraMTFont, when this machine has it: thick wall tiles
    /// come out as lines, a door's thin outline still shows.
    #[test]
    fn walls_are_lines_doors_still_show() {
        let path = &std::path::Path::new(&std::env::var("EMUERA_TEST_GAMES").unwrap_or_default())
            .join("eraMegatenP/Data/font/EraMTFontMedium.ttf")
            .display()
            .to_string();
        let Ok(data) = std::fs::read(path) else {
            eprintln!("skipped: {path} missing");
            return;
        };
        let font = ab_glyph::FontVec::try_from_vec(data).unwrap();
        assert_eq!(super::braille(&font, '\u{E193}', 2), ['⠶', '⠶'], "horizontal wall");
        assert_eq!(super::braille(&font, '\u{E19C}', 2), ['⢸', '⡇'], "vertical wall");
        assert!(super::braille(&font, '\u{E001}', 2).iter().all(|c| *c != '\u{2800}'), "door");
    }
}
