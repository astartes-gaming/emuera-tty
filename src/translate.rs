//! Emuera-Anchor's translation system, for games translated with it: the
//! CSV files stay as written and translations sit beside them.
//!
//! - `XXXX_TR.csv` (`Abl_TR.csv`, `Talent_TR.csv`…): `original,translation`
//!   lines for one CSV name table; text after `;` is a comment. Applied when
//!   such a name is printed (`%ABLNAME:2%`, PRINT_ABL, the TRAIN and shop
//!   lists), not when a script stores or compares it, as in Anchor.
//! - `ALL_TR.csv`: the same for any `%…%` value a PRINTFORM shows.
//! - `TRCHARA*.csv`: character data (NAME, CALLNAME, NICKNAME, MASTERNAME,
//!   CSTR) in blocks from `NO,n` to `ENDCHR`; applied to the character
//!   templates, so characters added in a new game come out translated.
//! - `TR_NAME(code, index or name)`: a translated name for scripts.

use crate::csvdata::CharaTemplate;
use crate::fxhash::HashMap;
use crate::loader::{read_text, walk};
use std::path::Path;

#[derive(Default, Debug)]
pub struct Tr {
    /// table (ABL, TALENT…, ALL) → original → translation
    tables: HashMap<String, HashMap<String, String>>,
}

/// The name table a `XXXX_TR.csv` file translates.
fn table_of(prefix: &str) -> Option<&'static str> {
    Some(match prefix {
        "ABL" => "ABL",
        "TALENT" => "TALENT",
        "EXP" => "EXP",
        "MARK" => "MARK",
        "PALAM" | "PARAM" => "PALAM",
        "ITEM" => "ITEM",
        "TRAIN" => "TRAIN",
        "BASE" => "BASE",
        "SOURCE" => "SOURCE",
        "EX" => "EX",
        "EQUIP" => "EQUIP",
        "TEQUIP" => "TEQUIP",
        "FLAG" => "FLAG",
        "TFLAG" => "TFLAG",
        "CFLAG" => "CFLAG",
        "TCVAR" => "TCVAR",
        "CSTR" => "CSTR",
        "STAIN" => "STAIN",
        "CDFLAG1" => "CDFLAG1",
        "CDFLAG2" => "CDFLAG2",
        "STR" => "STRNAME",
        "TSTR" => "TSTR",
        "SAVESTR" => "SAVESTR",
        "GLOBAL" => "GLOBAL",
        "GLOBALS" => "GLOBALS",
        "ALL" => "ALL",
        _ => return None,
    })
}

/// A line without its `;` comment, split at the first comma.
fn pair(line: &str) -> Option<(String, String)> {
    let line = line.split(';').next().unwrap_or("").trim_start_matches('\u{feff}');
    let (a, b) = line.split_once(',')?;
    let (a, b) = (a.trim(), b.trim().trim_end_matches(','));
    (!a.is_empty()).then(|| (a.to_string(), b.to_string()))
}

impl Tr {
    /// The `*_TR.csv` files under `csv_dir`.
    pub fn load(csv_dir: &Path) -> Tr {
        let mut t = Tr::default();
        for p in walk(csv_dir, &["csv"]) {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
            let Some(prefix) = stem.strip_suffix("_TR") else { continue };
            let Some(table) = table_of(prefix) else { continue };
            let Ok(text) = read_text(&p) else { continue };
            let m = t.tables.entry(table.to_string()).or_default();
            for line in text.lines() {
                if let Some((a, b)) = pair(line) {
                    m.entry(a).or_insert(b);
                }
            }
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }

    /// The translation of `orig` from `table`'s file, else from ALL_TR.
    pub fn name(&self, table: &str, orig: &str) -> Option<&str> {
        self.tables.get(table).and_then(|m| m.get(orig)).or_else(|| self.all(orig)).map(|s| s.as_str())
    }

    /// A `%…%` value's translation from ALL_TR.csv.
    pub fn all(&self, orig: &str) -> Option<&String> {
        self.tables.get("ALL").and_then(|m| m.get(orig))
    }

    /// `orig`, translated if `table` (or ALL) has it.
    pub fn or_same<'a>(&'a self, table: &str, orig: &'a str) -> &'a str {
        if self.is_empty() {
            return orig;
        }
        self.name(table, orig).unwrap_or(orig)
    }
}

/// Apply the `TRCHARA*.csv` files under `csv_dir` to the character
/// templates; returns warnings.
pub fn apply_trchara(csv_dir: &Path, charas: &mut [CharaTemplate]) -> Vec<String> {
    let mut warn = Vec::new();
    for p in walk(csv_dir, &["csv"]) {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if !stem.starts_with("TRCHARA") {
            continue;
        }
        let Ok(text) = read_text(&p) else { continue };
        let mut cur: Option<usize> = None;
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or("").trim_start_matches('\u{feff}');
            let f: Vec<&str> = line.split(',').map(str::trim).collect();
            if f.len() < 2 {
                continue;
            }
            match f[0] {
                "NO" | "番号" => {
                    let no: i64 = f[1].parse().unwrap_or(-1);
                    cur = charas.iter().position(|c| c.no == no);
                    if cur.is_none() {
                        warn.push(format!("{}: no character {} to translate", p.display(), f[1]));
                    }
                }
                "ENDCHR" => cur = None,
                key => {
                    let Some(c) = cur.map(|i| &mut charas[i]) else { continue };
                    match key {
                        "NAME" | "名前" => (c.name, c.names_set) = (f[1].to_string(), c.names_set | 1),
                        "CALLNAME" | "呼び名" => (c.callname, c.names_set) = (f[1].to_string(), c.names_set | 2),
                        "NICKNAME" | "あだ名" => (c.nickname, c.names_set) = (f[1].to_string(), c.names_set | 4),
                        "MASTERNAME" | "主人の呼び方" => (c.mastername, c.names_set) = (f[1].to_string(), c.names_set | 8),
                        "CSTR" if f.len() >= 3 => {
                            // CSTR,index,text (the text may hold commas)
                            let Ok(i) = f[1].parse::<i64>() else { continue };
                            let text = line.splitn(3, ',').nth(2).unwrap_or("").trim().to_string();
                            match c.strs.iter_mut().find(|(v, k, _)| v == "CSTR" && *k == i) {
                                Some(s) => s.2 = text,
                                None => c.strs.push(("CSTR".into(), i, text)),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    warn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tr_files() {
        let d = std::env::temp_dir().join(format!("emuera-tr-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("Abl_TR.csv"), "技巧,Technique; a comment\n;only a comment\n").unwrap();
        std::fs::write(d.join("ALL_TR.csv"), "男,Man\n").unwrap();
        std::fs::write(d.join("sub/TRCHARA(test).csv"), "NO,1\nNAME,Reimu Hakurei,\nCALLNAME,Reimu,\nCSTR,3,shrine, maiden\nENDCHR,1,\n").unwrap();
        let t = Tr::load(&d);
        assert_eq!(t.name("ABL", "技巧"), Some("Technique"));
        assert_eq!(t.name("TALENT", "男"), Some("Man"));
        assert_eq!(t.or_same("ABL", "感覚"), "感覚");
        let mut c = vec![CharaTemplate { no: 1, name: "博麗霊夢".into(), callname: "霊夢".into(), ..Default::default() }];
        assert!(apply_trchara(&d, &mut c).is_empty());
        assert_eq!((c[0].name.as_str(), c[0].callname.as_str()), ("Reimu Hakurei", "Reimu"));
        assert_eq!(c[0].strs, vec![("CSTR".to_string(), 3, "shrine, maiden".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
