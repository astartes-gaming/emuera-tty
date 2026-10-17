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
        self.sprites.contains_key(&name.to_uppercase())
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
