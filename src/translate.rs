//! Emuera-Anchor's translation system, for games translated with it: the
//! CSV files stay as written and translations sit beside them.
//!
//! - `XXXX_TR.csv` (`Abl_TR.csv`, `Talent_TR.csv`…) in the CSV folder
//!   itself: `original,translation` lines for one CSV name table; text
//!   after `;` is a comment. Applied when such a name is printed
//!   (`%ABLNAME:2%`, PRINT_ABL, the TRAIN and shop lists), not when a
//!   script stores or compares it, as in Anchor.
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
    /// the game has `*_TR.csv` or `TRCHARA*.csv` files (see [`Tr::active`])
    files: bool,
    /// problems found in the `*_TR.csv` files, for the load warnings
    pub warnings: Vec<String>,
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

/// White space as Anchor's `LexicalAnalyzer.IsWhiteSpace` sees it.
const WHITE: [char; 3] = [' ', '\t', '\u{3000}'];

/// A file's lines as Anchor's `EraStreamReader.ReadEnabledLine` gives them:
/// split where .NET's ReadLine splits (`\r\n`, `\n` or a lone `\r`), blank
/// lines skipped, leading white space dropped, and the lines from a `{`
/// line to a `}` line joined into one, a space after each. Each item
/// carries its line number (a joined line's is its `{` line's). Err is the
/// problem that makes Anchor stop loading; here the line or block is
/// skipped instead. Unlike Anchor, a stray byte-order mark at a line's
/// start is dropped too (doc/anchor.md, *Deliberate differences*).
fn enabled_lines(text: &str) -> Vec<(usize, Result<String, &'static str>)> {
    let raw: Vec<&str> = text.split('\n').flat_map(|l| l.strip_suffix('\r').unwrap_or(l).split('\r')).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let no = i + 1;
        // (also a stray byte-order mark, which Anchor would keep in the key)
        let line = raw[i].trim_start_matches(WHITE).trim_start_matches('\u{feff}');
        i += 1;
        if line.is_empty() {
            continue;
        }
        if line.starts_with('}') {
            out.push((no, Err("'}' without a '{' before it")));
            continue;
        }
        if !line.starts_with('{') {
            out.push((no, Ok(line.to_string())));
            continue;
        }
        if raw[no - 1].trim() != "{" {
            out.push((no, Err("a '{' line can't hold anything else")));
            continue;
        }
        let mut joined = String::new();
        let mut err = None;
        loop {
            let Some(l) = raw.get(i) else {
                err = err.or(Some("'{' without a '}' after it"));
                break;
            };
            i += 1;
            let test = l.trim_start();
            if test.starts_with('}') {
                if test.trim() != "}" {
                    err = err.or(Some("a '}' line can't hold anything else"));
                }
                break;
            }
            if test.starts_with('{') {
                err = err.or(Some("'{' inside a '{' … '}' block"));
            }
            joined.push_str(l);
            joined.push(' ');
        }
        let joined = joined.trim_start_matches(WHITE);
        match err {
            Some(e) => out.push((no, Err(e))),
            None if joined.is_empty() => {}
            None => out.push((no, Ok(joined.to_string()))),
        }
    }
    out
}

/// One enabled `*_TR.csv` line read as Anchor's Translation does: `;` lines
/// ignored (None), the rest cut at the first `;` and split at commas.
/// Nothing is trimmed. Err: the line doesn't have exactly one comma.
fn tr_line(line: &str) -> Option<Result<(&str, &str), ()>> {
    if line.starts_with(';') {
        return None;
    }
    let line = line.split(';').next().unwrap_or("");
    let mut f = line.split(',');
    Some(match (f.next(), f.next(), f.next()) {
        (Some(a), Some(b), None) => Ok((a, b)),
        _ => Err(()),
    })
}

impl Tr {
    /// The `*_TR.csv` files in `csv_dir` (not its subfolders, as Anchor),
    /// and whether there are `TRCHARA*.csv` files anywhere under it.
    pub fn load(csv_dir: &Path) -> Tr {
        let mut t = Tr::default();
        let Ok(rd) = std::fs::read_dir(csv_dir) else { return t };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for p in files {
            let name = p.file_name().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
            let Some(prefix) = name.strip_suffix("_TR.CSV") else { continue };
            let Some(table) = table_of(prefix) else { continue };
            let Ok(text) = read_text(&p) else { continue };
            t.files = true;
            // where each key of this file was defined, for duplicate warnings
            let mut seen: HashMap<&str, usize> = HashMap::default();
            let m = t.tables.entry(table.to_string()).or_default();
            let lines = enabled_lines(&text);
            for (n, line) in &lines {
                let line = match line {
                    Ok(l) => l,
                    Err(e) => {
                        t.warnings.push(format!("{}:{}: {} (skipped)", p.display(), n, e));
                        continue;
                    }
                };
                match tr_line(line) {
                    None => {}
                    Some(Err(())) => {
                        t.warnings.push(format!("{}:{}: a line needs exactly one ',' between the original and its translation (skipped)", p.display(), n))
                    }
                    Some(Ok((a, b))) => match seen.get(a) {
                        Some(first) => t.warnings.push(format!("{}:{}: '{}' is already translated on line {} (this one is ignored)", p.display(), n, a, first)),
                        None => {
                            seen.insert(a, *n);
                            // a key another file of the same table has: the first file wins
                            m.entry(a.to_string()).or_insert_with(|| b.to_string());
                        }
                    },
                }
            }
        }
        if !t.files {
            t.files = walk(csv_dir, &["csv"]).iter().any(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy().to_uppercase();
                    n.starts_with("TRCHARA") && n.ends_with(".CSV")
                })
            });
        }
        t
    }

    /// Does the game use Anchor's translation system (it has `*_TR.csv` or
    /// `TRCHARA*.csv` files)? Everything Anchor-specific is skipped when not.
    pub fn active(&self) -> bool {
        self.files
    }

    /// The translation of `orig` from `table`'s own file. (ALL_TR is looked
    /// up separately, with [`Tr::all`], where Anchor does.)
    pub fn name(&self, table: &str, orig: &str) -> Option<&str> {
        self.tables.get(table).and_then(|m| m.get(orig)).map(|s| s.as_str())
    }

    /// A `%…%` value's translation from ALL_TR.csv.
    pub fn all(&self, orig: &str) -> Option<&String> {
        self.tables.get("ALL").and_then(|m| m.get(orig))
    }

    /// `orig`, translated if `table` has it.
    pub fn or_same<'a>(&'a self, table: &str, orig: &'a str) -> &'a str {
        if !self.files {
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
        assert!(t.active());
        assert_eq!(t.name("ABL", "技巧"), Some("Technique"));
        // a table's own file only: ALL_TR is a separate lookup
        assert_eq!(t.name("TALENT", "男"), None);
        assert_eq!(t.all("男").map(String::as_str), Some("Man"));
        assert_eq!(t.or_same("ABL", "感覚"), "感覚");
        let mut c = vec![CharaTemplate { no: 1, name: "博麗霊夢".into(), callname: "霊夢".into(), ..Default::default() }];
        assert!(apply_trchara(&d, &mut c).is_empty());
        assert_eq!((c[0].name.as_str(), c[0].callname.as_str()), ("Reimu Hakurei", "Reimu"));
        assert_eq!(c[0].strs, vec![("CSTR".to_string(), 3, "shrine, maiden".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("emuera-tr-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        d
    }

    /// Lines as Anchor reads them: leading white space skipped, the rest
    /// kept as written up to `;`.
    #[test]
    fn tr_lines_are_not_trimmed() {
        let d = dir("lines");
        std::fs::write(d.join("Abl_TR.csv"), "技巧, Technique; skill\r\n  ;indented comment\n\n   \n  感覚,Sense \n").unwrap();
        let t = Tr::load(&d);
        assert_eq!(t.name("ABL", "技巧"), Some(" Technique"));
        assert_eq!(t.name("ABL", "感覚"), Some("Sense "));
        assert!(t.warnings.is_empty(), "{:?}", t.warnings);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tr_problems_are_warnings() {
        let d = dir("warn");
        std::fs::write(d.join("talent_tr.CSV"), "処女,Virgin\nx\n処女,Maiden\na,b,c\n男,Man\n").unwrap();
        let t = Tr::load(&d);
        // the file name matches whatever its case; the first definition wins
        assert_eq!(t.name("TALENT", "処女"), Some("Virgin"));
        assert_eq!(t.name("TALENT", "男"), Some("Man"));
        assert_eq!(t.name("TALENT", "a"), None);
        assert_eq!(t.warnings.len(), 3, "{:?}", t.warnings);
        assert!(t.warnings[0].ends_with("talent_tr.CSV:2: a line needs exactly one ',' between the original and its translation (skipped)"));
        assert!(t.warnings[1].ends_with("talent_tr.CSV:3: '処女' is already translated on line 1 (this one is ignored)"));
        assert!(t.warnings[2].contains("talent_tr.CSV:4: a line needs exactly one ','"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// EraStreamReader's lines: a lone `\r` ends one, `{` … `}` joins them
    /// with a space after each, a misplaced brace is a warning.
    #[test]
    fn tr_lines_as_era_stream_reader() {
        let d = dir("braces");
        let text = "a,A\rb,B\r\n{\n  c,\n C\n}\n}\n{ d,D\n{\ne,E\n";
        std::fs::write(d.join("Abl_TR.csv"), text).unwrap();
        let t = Tr::load(&d);
        assert_eq!(t.name("ABL", "a"), Some("A"));
        assert_eq!(t.name("ABL", "b"), Some("B"));
        assert_eq!(t.name("ABL", "c"), Some("  C "));
        assert_eq!(t.name("ABL", "d"), None);
        assert_eq!(t.name("ABL", "e"), None, "inside an unclosed block");
        assert_eq!(t.warnings.len(), 3, "{:?}", t.warnings);
        assert!(t.warnings[0].ends_with("Abl_TR.csv:7: '}' without a '{' before it (skipped)"));
        assert!(t.warnings[1].ends_with("Abl_TR.csv:8: a '{' line can't hold anything else (skipped)"));
        assert!(t.warnings[2].ends_with("Abl_TR.csv:9: '{' without a '}' after it (skipped)"));
        // a stray byte-order mark (from files pasted together) isn't part of the key
        std::fs::write(d.join("Abl_TR.csv"), "\u{feff}a,A\n\u{feff}b,B\n").unwrap();
        let t = Tr::load(&d);
        assert_eq!(t.name("ABL", "a"), Some("A"));
        assert_eq!(t.name("ABL", "b"), Some("B"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `*_TR.csv` only in the CSV folder itself, as Anchor; `TRCHARA*.csv`
    /// anywhere under it turns the translation system on too.
    #[test]
    fn which_files_count() {
        let d = dir("where");
        std::fs::write(d.join("sub/Talent_TR.csv"), "処女,Virgin\n").unwrap();
        std::fs::write(d.join("Juel_TR.csv"), "a,b\n").unwrap();
        let t = Tr::load(&d);
        assert_eq!(t.name("TALENT", "処女"), None);
        assert!(!t.active(), "a subfolder's TR file and an unknown table don't count");
        std::fs::write(d.join("sub/trchara_x.csv"), "NO,1\nENDCHR\n").unwrap();
        assert!(Tr::load(&d).active());
        let _ = std::fs::remove_dir_all(&d);
    }
}
