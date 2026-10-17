//! File discovery and text decoding for an Emuera game directory.
//!
//! Emuera games ship ERB/ERH/CSV files either as UTF-8 (usually with BOM)
//! or as Shift_JIS. We decode both transparently.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Read a text file, auto-detecting UTF-8 (with/without BOM) vs Shift_JIS.
pub fn read_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(decode(&bytes))
}

pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => {
            let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
            cow.into_owned()
        }
    }
}

/// Find a direct child directory by case-insensitive name.
pub fn find_dir_ci(base: &Path, name: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(base).ok()?;
    for e in rd.flatten() {
        // e.path().is_dir() follows symlinks (a linked resources/ folder)
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_dir()
        {
            return Some(e.path());
        }
    }
    None
}

/// Find a file in `dir` by case-insensitive name.
pub fn find_file_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(e.path());
        }
    }
    None
}

/// Recursively collect files with one of the given (lowercase) extensions,
/// sorted case-insensitively by path (matches Emuera's load order closely
/// enough: Windows enumerates alphabetically).
pub fn walk(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_inner(dir, exts, &mut out);
    out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    out
}

/// Recursive part of [`walk`].
fn walk_inner(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_inner(&p, exts, out);
        } else if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if exts.iter().any(|x| *x == ext) {
                out.push(p);
            }
        }
    }
}

/// Split a CSV line Emuera-style: comma separated, `;` starts a comment,
/// fields are trimmed. Empty trailing fields are kept.
pub fn csv_fields(line: &str) -> Vec<String> {
    let line = match line.find(';') {
        Some(i) => &line[..i],
        None => line,
    };
    if line.trim().is_empty() {
        return Vec::new();
    }
    line.split(',').map(|s| s.trim().to_string()).collect()
}

/// Shift_JIS byte length, which is what Emuera's STRLENS/alignment use.
pub fn sjis_len(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let mut buf = [0u8; 4];
            let enc = c.encode_utf8(&mut buf);
            let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(enc);
            if unmappable { 2 } else { bytes.len() }
        })
        .sum()
}
