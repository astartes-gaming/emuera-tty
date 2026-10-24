//! Writing save files safely, and keeping their history.
//!
//! Every save the game writes (saveNN.sav, global.sav, SAVECHARA's
//! dat/chara_*.dat) goes through [`write`]:
//!
//! 1. The file being replaced is copied into the game's backup folder (the
//!    save manager's, `<data>/emuera-tty/backups/<game>/`) as
//!    `<date>-auto-<name>/`, unless that version is already there. The
//!    first time a file emuera-tty didn't write is replaced (one Emuera.exe
//!    or an emuera-tty from before this history made), the copy is
//!    `<date>-original-<name>/` and is never thinned out.
//! 2. The new data goes to a temporary file beside the save, which is
//!    flushed to disk, read back, compared, and walked through as Emuera's
//!    binary format ([`crate::vm::erasav::check`]); only then is it renamed
//!    over the save. A crash, power cut or full disk leaves the old save,
//!    and a save that wouldn't load is reported at once, the old one kept.
//! 3. `save-log.tsv` in the backup folder gets a line: when, which file,
//!    its fingerprint and size, and the emuera-tty version and commit that
//!    wrote it, so the saves a faulty build wrote can be found later.
//! 4. The automatic copies of that file are thinned: the last 10 kept,
//!    then the newest of each day for 30 days, then of each week for 26
//!    weeks.
//!
//! Nothing here changes what is in a save: Emuera reads them as before.
//! `EMUERA_SAVE_HISTORY=0` turns 1, 3 and 4 off (the safe writing stays).

use std::io::Write;
use std::path::{Path, PathBuf};

/// The version and commit this build was made from (`build.rs`).
pub fn build_id() -> String {
    format!("{} {}", env!("CARGO_PKG_VERSION"), env!("EMUERA_COMMIT"))
}

/// A fingerprint of `data` for the log and for spotting a version already
/// kept (64-bit FNV-1a; copies are compared byte for byte too).
pub fn fingerprint(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

/// What [`write`] has to say beyond success: history problems that didn't
/// stop the save.
pub type Warnings = Vec<String>;

/// Write save `path` with `data` as described in the module doc; `root`
/// is the game's backup folder (None: no history).
pub fn write(path: &Path, data: &[u8], root: Option<&Path>) -> Result<Warnings, String> {
    let mut warn = Warnings::new();
    let dir = path.parent().unwrap_or(Path::new("."));
    let _ = std::fs::create_dir_all(dir);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let root = root.filter(|_| std::env::var("EMUERA_SAVE_HISTORY").as_deref() != Ok("0"));
    // 1. the version being replaced
    if let (Some(root), Ok(old)) = (root, std::fs::read(path)) {
        if old != data {
            if let Err(e) = keep_old(root, &name, &old) {
                warn.push(format!("save history: {}", e));
            }
        }
    }
    // 2. write, check, replace
    let tmp = dir.join(format!(".{}.tmp{}", name, std::process::id()));
    let res = write_checked(&tmp, data).and_then(|_| {
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {}", path.display(), e))?;
        sync_dir(dir);
        Ok(())
    });
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("save failed, the previous file is kept: {}", e));
    }
    // 3, 4. the log and the thinning
    if let Some(root) = root {
        if let Err(e) = log(root, "written", &name, data) {
            warn.push(format!("save log: {}", e));
        }
        if let Err(e) = thin(root, &name, crate::savemgr::stamp().as_str()) {
            warn.push(format!("save history: {}", e));
        }
    }
    Ok(warn)
}

/// Write `data` to `tmp`, flush it to disk, and make sure what is on disk
/// is `data` and reads as a save.
fn write_checked(tmp: &Path, data: &[u8]) -> Result<(), String> {
    let e = |e: std::io::Error| format!("{}: {}", tmp.display(), e);
    let mut f = std::fs::File::create(tmp).map_err(e)?;
    f.write_all(data).map_err(e)?;
    f.sync_all().map_err(e)?;
    drop(f);
    let back = std::fs::read(tmp).map_err(e)?;
    if back != data {
        return Err(format!("{}: what was written reads back differently", tmp.display()));
    }
    crate::vm::erasav::check(data).map_err(|m| format!("the new save wouldn't load ({})", m))
}

/// Make a rename in `dir` survive a power cut (Unix: sync the folder).
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// The stem used in history folder names: `save01` for save01.sav.
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// Copy the save about to be replaced into the history, unless the newest
/// copy there is the same; as an original if emuera-tty didn't write it.
fn keep_old(root: &Path, name: &str, old: &[u8]) -> Result<(), String> {
    let fp = fingerprint(old);
    let entries = read_log(root);
    let ours = entries.iter().any(|e| e.kind == "written" && e.fp == fp);
    let kind = if ours {
        "auto"
    } else {
        // (an original kept once, even when it comes back later)
        if entries.iter().any(|e| e.kind == "original" && e.fp == fp) {
            "auto"
        } else {
            "original"
        }
    };
    if kind == "auto" && newest_copy(root, name).is_some_and(|p| std::fs::read(p).ok().as_deref() == Some(old)) {
        return Ok(());
    }
    let dest = root.join(format!("{}-{}-{}", crate::savemgr::stamp(), kind, stem(name)));
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    std::fs::write(dest.join(name), old).map_err(|e| format!("{}: {}", dest.display(), e))?;
    if kind == "original" {
        log(root, "original", name, old)?;
    }
    Ok(())
}

/// History folders of file `name`: (folder, its date stamp, original?).
fn copies(root: &Path, name: &str) -> Vec<(PathBuf, String, bool)> {
    let s = stem(name);
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<(PathBuf, String, bool)> = rd
        .flatten()
        .filter_map(|e| {
            let f = e.file_name().to_string_lossy().to_string();
            // <yyyymmdd-hhmmss>-<kind>-<stem>
            let (stamp, rest) = (f.get(..15)?, f.get(16..)?);
            let orig = match rest.split_once('-')? {
                ("auto", st) if st == s => false,
                ("original", st) if st == s => true,
                _ => return None,
            };
            e.path().join(name).is_file().then(|| (e.path(), stamp.to_string(), orig))
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// The newest copy of `name` in the history, original or not.
fn newest_copy(root: &Path, name: &str) -> Option<PathBuf> {
    copies(root, name).into_iter().next().map(|(p, _, _)| p.join(name))
}

/// Days since 1970-01-01 of a `yyyymmdd-...` stamp.
fn stamp_days(s: &str) -> Option<i64> {
    let (y, m, d): (i64, i64, i64) = (s.get(0..4)?.parse().ok()?, s.get(4..6)?.parse().ok()?, s.get(6..8)?.parse().ok()?);
    // (days_from_civil)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Thin the automatic copies of `name` (`now` is today's stamp): the 10
/// newest stay, then the newest of each day up to 30 days old, then of
/// each week up to 26 weeks; originals always stay.
fn thin(root: &Path, name: &str, now: &str) -> Result<(), String> {
    let today = stamp_days(now).ok_or("bad date")?;
    // newest first: a copy stays if it is among the 10 newest or the first
    // of its day (up to 30 days) or week (up to 26 weeks) not yet covered
    let (mut days, mut weeks) = (std::collections::HashSet::new(), std::collections::HashSet::new());
    for (i, (dir, stamp, _)) in copies(root, name).into_iter().filter(|c| !c.2).enumerate() {
        let Some(day) = stamp_days(&stamp) else { continue };
        let (age, week) = (today - day, day.div_euclid(7));
        let keep = i < 10 || (age <= 30 && !days.contains(&day)) || (age > 30 && age <= 26 * 7 && !weeks.contains(&week));
        if keep {
            days.insert(day);
            weeks.insert(week);
        } else {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        }
    }
    Ok(())
}

/// One line of save-log.tsv.
struct LogEntry {
    kind: String,
    fp: String,
}

fn log_path(root: &Path) -> PathBuf {
    root.join("save-log.tsv")
}

fn read_log(root: &Path) -> Vec<LogEntry> {
    let text = std::fs::read_to_string(log_path(root)).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 4).then(|| LogEntry { kind: f[1].to_string(), fp: f[3].to_string() })
        })
        .collect()
}

/// Add a line: date, what happened (`written`, `original`), file,
/// fingerprint, size, build.
fn log(root: &Path, kind: &str, name: &str, data: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {}", root.display(), e))?;
    let p = log_path(root);
    let new = !p.exists();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| format!("{}: {}", p.display(), e))?;
    let mut line = String::new();
    if new {
        line.push_str("# date\twhat\tfile\tfingerprint\tbytes\temuera-tty\n");
    }
    line.push_str(&format!("{}\t{}\t{}\t{}\t{}\t{}\n", crate::savemgr::stamp(), kind, name, fingerprint(data), data.len(), build_id()));
    f.write_all(line.as_bytes()).map_err(|e| format!("{}: {}", p.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("emuera-tty-savesafe-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Data that wouldn't load isn't written: the old save stays, and no
    /// temporary file is left.
    #[test]
    fn a_bad_save_keeps_the_old_one() {
        let d = tmp("bad");
        let p = d.join("save01.sav");
        std::fs::write(&p, b"the old save").unwrap();
        let e = write(&p, b"not a save at all", None).unwrap_err();
        assert!(e.contains("previous file is kept"), "{}", e);
        assert_eq!(std::fs::read(&p).unwrap(), b"the old save");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn days_from_dates() {
        assert_eq!(stamp_days("19700101-000000"), Some(0));
        assert_eq!(stamp_days("20000301-000000"), Some(11017));
        assert_eq!(stamp_days("20261023-120000").unwrap() - stamp_days("20261022-235959").unwrap(), 1);
    }

    /// Thinning keeps the 10 newest, one a day for a month, one a week for
    /// half a year; originals stay whatever their age.
    #[test]
    fn thinning() {
        let root = tmp("thin");
        let mk = |stamp: &str, kind: &str| {
            let d = root.join(format!("{}-{}-save01", stamp, kind));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("save01.sav"), stamp).unwrap();
        };
        // 12 today, 3 on each of the 40 days before, an old original
        for h in 0..12 {
            mk(&format!("20261023-{:02}0000", h), "auto");
        }
        for back in 1..=40 {
            let day = stamp_days("20261023-000000").unwrap() - back;
            // (back to a stamp through the save manager's own conversion)
            let (y, m, d) = crate::savemgr::civil(day);
            for h in [9, 12, 18] {
                mk(&format!("{:04}{:02}{:02}-{:02}0000", y, m, d, h), "auto");
            }
        }
        mk("20250101-000000", "original");
        // another file's copy: untouched
        let other = root.join("20000101-110000-auto-save02");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("save02.sav"), "x").unwrap();
        thin(&root, "save01.sav", "20261023-235959").unwrap();
        let left = copies(&root, "save01.sav");
        let autos: Vec<&String> = left.iter().filter(|c| !c.2).map(|c| &c.1).collect();
        // 10 newest (today), then one per day for days 1..=30, then weeks
        assert!(autos.len() >= 10 + 30 && autos.len() <= 10 + 30 + 3, "{} kept: {:?}", autos.len(), autos);
        assert!(left.iter().any(|c| c.2), "the original is kept");
        // the day-old ones keep their newest (18:00)
        assert!(autos.iter().any(|s| s.ends_with("-180000") && !s.starts_with("20261023")));
        assert!(!autos.iter().any(|s| s.ends_with("-090000") && !s.starts_with("20261023")));
        assert!(other.join("save02.sav").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
