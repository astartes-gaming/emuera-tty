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

/// `s`'s lines as Emuera reads them (C#'s ReadAllLines): a line ends at
/// "\r\n", "\n" or a lone "\r" (eraMaouEX has `\rDRAWLINE\rCALL …`).
pub fn emuera_lines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let b = rest.as_bytes();
        Some(match b.iter().position(|&c| c == b'\n' || c == b'\r') {
            Some(i) => {
                let line = &rest[..i];
                rest = &rest[i + if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') { 2 } else { 1 }..];
                line
            }
            None => std::mem::take(&mut rest),
        })
    })
}

pub fn decode(bytes: &[u8]) -> String {
    decode_detect(bytes).0
}

/// [`decode`], and whether the bytes were read as Shift_JIS (one pass over
/// them: a big game has 50 MB of scripts).
pub fn decode_detect(bytes: &[u8]) -> (String, bool) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(&bytes[3..]).into_owned(), false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_owned(), false),
        Err(_) => {
            let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
            (cow.into_owned(), true)
        }
    }
}

/// Whether [`decode`] reads these bytes as Shift_JIS.
pub fn is_sjis(bytes: &[u8]) -> bool {
    !bytes.starts_with(&[0xEF, 0xBB, 0xBF]) && std::str::from_utf8(bytes).is_err()
}

/// Part of a file (whole lines) in the encoding [`decode`] found for it.
pub fn decode_as(bytes: &[u8], sjis: bool) -> String {
    if sjis {
        return encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned();
    }
    String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)).into_owned()
}

/// Find a direct child directory by case-insensitive name.
pub fn find_dir_ci(base: &Path, name: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(base).ok()?;
    for e in rd.flatten() {
        // e.path().is_dir() follows symlinks (a linked resources/ folder)
        if e.file_name().to_string_lossy().eq_ignore_ascii_case(name) && e.path().is_dir() {
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

/// How Emuera lists the ERB folder: `サブディレクトリを検索する` (look in
/// subfolders) and `読み込み順をファイル名順にソートする` (sort by name),
/// both off unless the config turns them on.
#[derive(Clone, Copy, Debug)]
pub struct Listing {
    pub subdirs: bool,
    pub sort: bool,
}

impl Listing {
    pub fn of(game: &Path) -> Listing {
        let v = crate::emuconfig::Values::read(game);
        Listing { subdirs: v.flag("サブディレクトリを検索する"), sort: v.flag("読み込み順をファイル名順にソートする") }
    }
}

/// Emuera's order of names in a folder. Sorted (`Array.Sort`, .NET's
/// culture order, ICU): case aside, punctuation before digits before
/// letters, kana as one, and on to other scripts by code point. Unsorted:
/// the order Windows lists an NTFS folder in, by upper-cased code units.
pub fn name_order(a: &str, b: &str, sort: bool) -> std::cmp::Ordering {
    if !sort {
        let up = |s: &str| {
            s.encode_utf16()
                .map(|u| char::from_u32(u as u32).map_or(u as u32, |c| c.to_uppercase().next().map_or(u as u32, |c| c as u32)))
                .collect::<Vec<u32>>()
        };
        return up(a).cmp(&up(b)).then_with(|| a.cmp(b));
    }
    fn key(c: char) -> (u8, u32) {
        const PUNCT: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
        // full-width ASCII is the same letter
        let c = match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            0x3000 => ' ',
            _ => c,
        };
        match c {
            ' ' | '\t' => (0, 0),
            '0'..='9' => (1, c as u32),
            'a'..='z' | 'A'..='Z' => (2, c.to_ascii_lowercase() as u32),
            _ if c.is_ascii() => (0, PUNCT.find(c).map_or(200, |i| i as u32 + 1)),
            // CJK punctuation and general symbols sort with punctuation
            '\u{2000}'..='\u{2BFF}' | '\u{3001}'..='\u{303F}' | '\u{30FB}' => (0, 1000 + c as u32),
            // katakana as hiragana
            '\u{30A1}'..='\u{30F6}' => (3, c as u32 - 0x60),
            _ => (3, c as u32),
        }
    }
    let ka: Vec<(u8, u32)> = a.chars().map(key).collect();
    let kb: Vec<(u8, u32)> = b.chars().map(key).collect();
    // then lower case before upper, then by code point
    let case = |s: &str| s.chars().map(|c| c.is_uppercase() as u8).collect::<Vec<u8>>();
    ka.cmp(&kb).then_with(|| case(a).cmp(&case(b))).then_with(|| a.cmp(b))
}

/// Emuera's `Config.GetFiles`: the files of `dir` with one of the given
/// (lowercase) extensions, then (with `subdirs`) each subfolder's the
/// same way, each level in `name_order`.
pub fn emuera_files(dir: &Path, exts: &[&str], l: Listing) -> Vec<PathBuf> {
    let mut out = Vec::new();
    list_into(dir, exts, l, &mut out);
    out
}

fn list_into(dir: &Path, exts: &[&str], l: Listing, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            dirs.push((name, p));
        } else if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if exts.iter().any(|x| *x == ext) {
                files.push((name, p));
            }
        }
    }
    files.sort_by(|a, b| name_order(&a.0, &b.0, l.sort));
    out.extend(files.into_iter().map(|f| f.1));
    if l.subdirs {
        dirs.sort_by(|a, b| name_order(&a.0, &b.0, l.sort));
        for (_, d) in dirs {
            list_into(&d, exts, l, out);
        }
    }
}

/// The ERB files in Emuera's load order: first those under folders whose
/// name has a `#` (Emuera-EE's load-first folders), then the rest.
pub fn erb_files(erb: &Path, l: Listing) -> Vec<PathBuf> {
    fn hash_dirs(d: &Path, deep: bool, sort: bool, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        let mut dirs: Vec<(String, PathBuf)> =
            rd.flatten().filter(|e| e.path().is_dir()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
        dirs.sort_by(|a, b| name_order(&a.0, &b.0, sort));
        for (name, p) in dirs {
            if name.contains('#') {
                out.push(p.clone());
            }
            if deep {
                hash_dirs(&p, deep, sort, out);
            }
        }
    }
    let mut first = Vec::new();
    hash_dirs(erb, l.subdirs, l.sort, &mut first);
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for d in first {
        for f in emuera_files(&d, &["erb"], l) {
            if seen.insert(f.clone()) {
                out.push(f);
            }
        }
    }
    for f in emuera_files(erb, &["erb"], l) {
        if seen.insert(f.clone()) {
            out.push(f);
        }
    }
    out
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

/// Shift_JIS byte length, which is what Emuera's STRLENS / alignment use
/// (`LangManager.GetByteCountLang`): per UTF-16 unit, a character code page
/// 932 can't encode becomes `?`, one byte, so an unencodable character
/// counts 1 and an emoji (two UTF-16 units) 2.
pub fn sjis_len(s: &str) -> usize {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                return 1;
            }
            let mut buf = [0u8; 4];
            let enc = c.encode_utf8(&mut buf);
            let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(enc);
            if unmappable {
                c.len_utf16()
            } else {
                bytes.len()
            }
        })
        .sum()
}

#[cfg(test)]
mod order_tests {
    #[test]
    fn lines_as_emuera_reads_them() {
        let v: Vec<&str> = super::emuera_lines("a\r\nb\rc\n\rd\n").collect();
        assert_eq!(v, ["a", "b", "c", "", "d"]);
        assert_eq!(super::emuera_lines("x").collect::<Vec<_>>(), ["x"]);
        assert_eq!(super::emuera_lines("").count(), 0);
    }

    use super::name_order;

    #[test]
    fn emuera_name_order() {
        use std::cmp::Ordering::*;
        // sorted (.NET / ICU): case aside, punctuation < digits < letters
        assert_eq!(name_order("apple.ERB", "Banana.ERB", true), Less);
        assert_eq!(name_order("A_B.ERB", "A.ERB", true), Less);
        assert_eq!(name_order("10.ERB", "2.ERB", true), Less);
        assert_eq!(name_order("_X.ERB", "0.ERB", true), Less);
        assert_eq!(name_order("a.erb", "A.erb", true), Less);
        assert_eq!(name_order("カ.ERB", "き.ERB", true), Less);
        // unsorted (NTFS): upper-cased code units, so '_' after letters
        assert_eq!(name_order("A_B.ERB", "AB.ERB", false), Greater);
        assert_eq!(name_order("A.ERB", "A_B.ERB", false), Less);
    }
}

#[cfg(test)]
mod sjis_tests {
    use super::sjis_len;

    #[test]
    fn emuera_byte_counts() {
        assert_eq!(sjis_len("abc"), 3);
        assert_eq!(sjis_len("あい"), 4);
        assert_eq!(sjis_len("ｱｲ"), 2); // half-width kana
        assert_eq!(sjis_len("♂"), 2); // in JIS X 0208
        assert_eq!(sjis_len("한"), 1); // not in code page 932: "?"
        assert_eq!(sjis_len("😀"), 2); // two UTF-16 units, two "?"
    }
}
