//! Playing games straight from their download archive.
//!
//! An archive is extracted once into `$XDG_DATA_HOME/emuera-tty/games/<name>/`
//! (default `~/.local/share/…`) and the game folder inside it (the one with
//! `CSV/` and `ERB/`, possibly a few levels down) is played. A marker file
//! records the archive's size and modification time; when the archive
//! changes (a newer release under the same name) it is extracted again and
//! the player's `sav/` folder is carried over to the new copy.
//!
//! zip, tar and tar.gz are read in-process. Entries are written one by one
//! with their paths checked: absolute paths and `..` are skipped, as are
//! symlinks and other special files, so an archive can't write outside its
//! folder. Zip names that aren't UTF-8 are decoded as Shift_JIS (Windows
//! zips of Japanese games). Other formats (tar.xz/bz2/zst, 7z, rar) are
//! handed to `bsdtar`, `tar` or `7z` from the system, and any symlinks they
//! create are removed afterwards.

use crate::picker::is_game_dir;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// File name suffixes recognised as archives (lower-case).
const SUFFIXES: &[&str] = &[".zip", ".tar.gz", ".tgz", ".tar", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar.zst", ".7z", ".rar"];

/// The archive suffix of `path`, if it has one we know.
pub fn suffix(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    SUFFIXES.iter().copied().filter(|s| name.ends_with(s)).max_by_key(|s| s.len())
}

pub fn is_archive(path: &Path) -> bool {
    path.is_file() && suffix(path).is_some()
}

/// `games/<archive name without its suffix>`.
fn dest_for(archive: &Path) -> Option<PathBuf> {
    let name = archive.file_name()?.to_string_lossy().to_string();
    let stem = &name[..name.len() - suffix(archive)?.len()];
    let base = crate::sys::data_base()?;
    Some(base.join("emuera-tty").join("games").join(stem))
}

/// "size mtime" of the archive, to notice when it changes.
fn stamp(archive: &Path) -> String {
    let m = fs::metadata(archive).ok();
    let mtime = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    format!("{} {}", m.map_or(0, |m| m.len()), mtime)
}

const MARKER: &str = ".emuera-tty-source";

/// The game folder inside `root`: `root` itself or the first match within
/// three levels (archives often wrap everything in one top folder).
pub fn find_game(root: &Path) -> Option<PathBuf> {
    fn walk(d: &Path, depth: usize) -> Option<PathBuf> {
        if is_game_dir(d) {
            return Some(d.to_path_buf());
        }
        if depth == 0 {
            return None;
        }
        let mut subs: Vec<PathBuf> = fs::read_dir(d).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.is_symlink()).collect();
        subs.sort();
        subs.iter().find_map(|s| walk(s, depth - 1))
    }
    walk(root, 3)
}

/// The game folder from an earlier extraction, if the archive is unchanged.
pub fn already_extracted(archive: &Path) -> Option<PathBuf> {
    let dest = dest_for(archive)?;
    (fs::read_to_string(dest.join(MARKER)).ok()? == stamp(archive)).then(|| find_game(&dest)).flatten()
}

/// Extract `archive` (or reuse an earlier extraction) and return the game
/// folder to play. `progress` is told how many files have been written.
pub fn extract_game(archive: &Path, progress: &mut dyn FnMut(usize)) -> Result<PathBuf, String> {
    if let Some(g) = already_extracted(archive) {
        return Ok(g);
    }
    let dest = dest_for(archive).ok_or("can't work out where to extract (HOME not set?)")?;
    let stamp = stamp(archive);
    let parent = dest.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    let tmp = parent.join(format!(".{}.partial", dest.file_name().unwrap().to_string_lossy()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("{}: {}", tmp.display(), e))?;
    let result = unpack(archive, &tmp, progress);
    let game = match result.and_then(|()| find_game(&tmp).ok_or_else(|| "no Emuera game (a folder with CSV/ and ERB/) in this archive".to_string())) {
        Ok(g) => g,
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    // keep the player's saves from an earlier extraction
    if let Some(old_sav) = find_game(&dest).map(|g| g.join("sav")).filter(|s| s.is_dir()) {
        let new_sav = game.join("sav");
        if new_sav.exists() {
            let _ = fs::rename(&new_sav, game.join("sav.from-archive"));
        }
        fs::rename(&old_sav, &new_sav).map_err(|e| format!("keeping saves {}: {}", old_sav.display(), e))?;
    }
    let rel = game.strip_prefix(&tmp).unwrap_or(Path::new("")).to_path_buf();
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    }
    fs::rename(&tmp, &dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    let _ = fs::write(dest.join(MARKER), &stamp);
    Ok(dest.join(rel))
}

/// Decode an entry name: UTF-8, else Shift_JIS.
fn decode_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(raw).0.into_owned(),
    }
}

/// `root` + an archive entry path, or None if the entry would land outside
/// `root` (absolute path, `..`, Windows drive).
fn safe_join(root: &Path, name: &str) -> Option<PathBuf> {
    let rel = PathBuf::from(name.replace('\\', "/"));
    let mut out = root.to_path_buf();
    let mut any = false;
    for c in rel.components() {
        match c {
            Component::Normal(p) if !p.to_string_lossy().contains(':') => {
                out.push(p);
                any = true;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    any.then_some(out)
}

/// Write one regular file from an archive entry.
fn write_file(path: &Path, r: &mut dyn io::Read) -> Result<(), String> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {}", d.display(), e))?;
    }
    let mut f = fs::File::create(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    io::copy(r, &mut f).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok(())
}

fn unpack(archive: &Path, to: &Path, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
    let open = || fs::File::open(archive).map_err(|e| format!("{}: {}", archive.display(), e));
    match suffix(archive) {
        Some(".zip") => unzip(open()?, to, progress),
        Some(".tar") => untar(io::BufReader::new(open()?), to, progress),
        Some(".tar.gz") | Some(".tgz") => untar(flate2::read::MultiGzDecoder::new(io::BufReader::new(open()?)), to, progress),
        _ => external(archive, to),
    }
}

fn unzip(f: fs::File, to: &Path, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
    let mut z = zip::ZipArchive::new(f).map_err(|e| format!("not a readable zip: {}", e))?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| format!("zip entry {}: {}", i, e))?;
        let Some(path) = safe_join(to, &decode_name(e.name_raw())) else { continue };
        let is_link = e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        if e.is_dir() {
            fs::create_dir_all(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        } else if !is_link {
            write_file(&path, &mut e)?;
            n += 1;
            if n % 200 == 0 {
                progress(n);
            }
        }
    }
    progress(n);
    Ok(())
}

fn untar(r: impl io::Read, to: &Path, progress: &mut dyn FnMut(usize)) -> Result<(), String> {
    let mut a = tar::Archive::new(r);
    let mut n = 0;
    for e in a.entries().map_err(|e| format!("not a readable tar: {}", e))? {
        let mut e = e.map_err(|e| format!("tar: {}", e))?;
        let name = decode_name(&e.path_bytes());
        let Some(path) = safe_join(to, &name) else { continue };
        let kind = e.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        } else if kind.is_file() {
            write_file(&path, &mut e)?;
            n += 1;
            if n % 200 == 0 {
                progress(n);
            }
        }
    }
    progress(n);
    Ok(())
}

/// Formats we don't decode ourselves: the first available system tool.
fn external(archive: &Path, to: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let have = |t: &str| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file()));
    let is_tar = matches!(suffix(archive), Some(".tar.xz" | ".txz" | ".tar.bz2" | ".tbz2" | ".tar.zst"));
    let mut cmd = if have("bsdtar") {
        let mut c = Command::new("bsdtar");
        c.arg("-xf").arg(archive).arg("-C").arg(to);
        c
    } else if is_tar && have("tar") {
        let mut c = Command::new("tar");
        c.arg("-xf").arg(archive).arg("-C").arg(to).arg("--no-same-owner");
        c
    } else if let Some(t) = ["7z", "7za", "7zz"].into_iter().find(|t| have(t)) {
        let mut c = Command::new(t);
        c.arg("x").arg("-y").arg(format!("-o{}", to.display())).arg(archive);
        c
    } else {
        return Err("this archive type needs bsdtar, tar or 7z installed (e.g. `sudo apt install libarchive-tools p7zip-full`)".into());
    };
    let st = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(format!("extracting {} failed ({})", archive.display(), st));
    }
    remove_symlinks(to);
    Ok(())
}

/// Delete every symlink under `d` (external tools may create them).
fn remove_symlinks(d: &Path) {
    let Ok(rd) = fs::read_dir(d) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_symlink() {
            let _ = fs::remove_file(&p);
        } else if p.is_dir() {
            remove_symlinks(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_blocks_escapes() {
        let r = Path::new("/x");
        assert_eq!(safe_join(r, "a/b.txt"), Some(PathBuf::from("/x/a/b.txt")));
        assert_eq!(safe_join(r, "a\\b.txt"), Some(PathBuf::from("/x/a/b.txt")));
        assert_eq!(safe_join(r, "./a"), Some(PathBuf::from("/x/a")));
        assert_eq!(safe_join(r, "../evil"), None);
        assert_eq!(safe_join(r, "a/../../evil"), None);
        assert_eq!(safe_join(r, "/etc/passwd"), None);
        assert_eq!(safe_join(r, "C:/evil"), None);
        assert_eq!(safe_join(r, ""), None);
    }

    #[test]
    fn suffixes() {
        assert_eq!(suffix(Path::new("Game v1.2.tar.gz")), Some(".tar.gz"));
        assert_eq!(suffix(Path::new("x.TGZ")), Some(".tgz"));
        assert_eq!(suffix(Path::new("x.zip")), Some(".zip"));
        assert_eq!(suffix(Path::new("x.txt")), None);
    }

    #[test]
    fn sjis_names() {
        let (raw, _, _) = encoding_rs::SHIFT_JIS.encode("口上/霊夢.ERB");
        assert_eq!(decode_name(&raw), "口上/霊夢.ERB");
        assert_eq!(decode_name("ok.txt".as_bytes()), "ok.txt");
    }
}
