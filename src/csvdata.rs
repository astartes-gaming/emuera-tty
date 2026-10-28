//! CSV data: name tables (ABL.csv, TALENT.csv, ...), character templates
//! (Chara*.csv), GAMEBASE.csv, VariableSize.csv, _Rename.csv, *.ERD.
//!
//! Name tables are what make `ABL:技巧` work: each row `index,name` lets a
//! script use the name instead of the number. [`table_for_var`] says which
//! table a variable's index is looked up in (MAXBASE uses BASE.csv…).
//! Everything here is loaded once at startup and read-only afterwards.

use crate::fxhash::HashMap;
use crate::loader::{csv_fields, find_file_ci, read_text, walk};
use std::path::Path;

/// One `index,name` table, searchable both ways.
#[derive(Default, Debug, Clone)]
pub struct NameTable {
    pub by_name: HashMap<String, i64>,
    pub by_index: HashMap<i64, String>,
}

impl NameTable {
    /// Add a row; the first index wins if a name is repeated.
    pub fn insert(&mut self, idx: i64, name: &str) {
        if !name.is_empty() {
            self.by_name.entry(name.to_string()).or_insert(idx);
            self.by_name.entry(name.to_uppercase()).or_insert(idx);
            self.by_index.insert(idx, name.to_string());
        }
    }

    /// Another name for a row (EMEE `.als` files): found by lookups,
    /// never shown.
    pub fn add_alias(&mut self, idx: i64, name: &str) {
        if !name.is_empty() {
            self.by_name.entry(name.to_string()).or_insert(idx);
            self.by_name.entry(name.to_uppercase()).or_insert(idx);
        }
    }
}

/// A `Chara*.csv` file: the initial values ADDCHARA copies into a new
/// character.
#[derive(Default, Debug, Clone)]
pub struct CharaTemplate {
    /// 番号: the number ADDCHARA / EXISTCSV refer to.
    pub no: i64,
    pub name: String,
    pub callname: String,
    pub nickname: String,
    pub mastername: String,
    /// which of the four names above the CSV gives (`NAME_BITS`, even
    /// empty): Emuera leaves the others null, and doesn't save them
    pub names_set: u8,
    /// (VARIABLE, index, value)
    pub ints: Vec<(String, i64, i64)>,
    pub strs: Vec<(String, i64, String)>,
}

/// All CSV-side data of a game.
#[derive(Default, Debug)]
pub struct GameCsv {
    /// Name tables keyed by table name (`"ABL"`, `"CFLAG"`, ERD names…).
    pub tables: HashMap<String, NameTable>,
    /// Item.csv third column.
    pub item_price: HashMap<i64, i64>,
    pub charas: Vec<CharaTemplate>,
    /// GAMEBASE.csv `key,value` (タイトル, 作者, バージョン…).
    pub gamebase: HashMap<String, String>,
    /// _Replace.csv `key,value`: game-specific engine settings
    /// (販売アイテム数 = how many item numbers the SHOP sells, …).
    pub replace: HashMap<String, String>,
    /// VariableSize.csv: array sizes overriding the built-in defaults.
    pub varsize: HashMap<String, Vec<usize>>,
    /// _Rename.csv `(name, replacement)` for `[[name]]` in scripts.
    pub rename: Vec<(String, String)>,
    /// Str.csv: initial STR values.
    pub str_init: Vec<(i64, String)>,
    /// Emuera-Anchor translations (*_TR.csv)
    pub tr: crate::translate::Tr,
    /// Emuera-Anchor: the parameter names came from PARAM.CSV.
    pub param_csv: bool,
    pub warnings: Vec<String>,
}

/// Which name table a variable's last index is resolved against.
/// Every table `table_for_var` names (so a table fits in a few bits).
pub const TABLE_NAMES: &[&str] = &[
    "ABL", "TALENT", "EXP", "MARK", "PALAM", "BASE", "SOURCE", "EX", "ITEM", "TRAIN", "EQUIP", "TEQUIP", "FLAG", "TFLAG", "CFLAG", "TCVAR", "CSTR", "STAIN",
    "MONEY", "DAY", "TIME", "STRNAME", "TSTR", "SAVESTR", "GLOBAL", "GLOBALS",
];

pub fn table_for_var(var: &str) -> Option<&'static str> {
    Some(match var {
        "ABL" | "ABLNAME" => "ABL",
        "TALENT" | "TALENTNAME" => "TALENT",
        "EXP" | "EXPNAME" => "EXP",
        "MARK" | "MARKNAME" => "MARK",
        "PALAM" | "JUEL" | "GOTJUEL" | "UP" | "DOWN" | "CUP" | "CDOWN" | "PALAMNAME" => "PALAM",
        "BASE" | "MAXBASE" | "LOSEBASE" | "DOWNBASE" | "BASENAME" => "BASE",
        "SOURCE" | "SOURCENAME" => "SOURCE",
        "EX" | "NOWEX" | "EXNAME" => "EX",
        "ITEM" | "ITEMSALES" | "ITEMPRICE" | "ITEMNAME" | "BOUGHT" => "ITEM",
        "TRAINNAME" => "TRAIN",
        "EQUIP" | "EQUIPNAME" => "EQUIP",
        "TEQUIP" | "TEQUIPNAME" => "TEQUIP",
        "FLAG" | "FLAGNAME" => "FLAG",
        "TFLAG" | "TFLAGNAME" => "TFLAG",
        "CFLAG" | "CFLAGNAME" => "CFLAG",
        "TCVAR" | "TCVARNAME" => "TCVAR",
        "CSTR" | "CSTRNAME" => "CSTR",
        "STAIN" | "STAINNAME" => "STAIN",
        "MONEY" | "MONEYNAME" => "MONEY",
        // EMEE: DAY.csv / TIME.csv name DAY and TIME like other variables
        "DAY" | "DAYNAME" => "DAY",
        "TIME" | "TIMENAME" => "TIME",
        "STR" | "STRNAME" => "STRNAME",
        "TSTR" | "TSTRNAME" => "TSTR",
        "SAVESTR" | "SAVESTRNAME" => "SAVESTR",
        "GLOBAL" | "GLOBALNAME" => "GLOBAL",
        "GLOBALS" | "GLOBALSNAME" => "GLOBALS",
        _ => return None,
    })
}

const TABLE_FILES: &[(&str, &str)] = &[
    ("ABL.CSV", "ABL"),
    ("TALENT.CSV", "TALENT"),
    ("EXP.CSV", "EXP"),
    ("MARK.CSV", "MARK"),
    ("PALAM.CSV", "PALAM"),
    ("TRAIN.CSV", "TRAIN"),
    ("ITEM.CSV", "ITEM"),
    ("BASE.CSV", "BASE"),
    ("SOURCE.CSV", "SOURCE"),
    ("EX.CSV", "EX"),
    ("EQUIP.CSV", "EQUIP"),
    ("TEQUIP.CSV", "TEQUIP"),
    ("FLAG.CSV", "FLAG"),
    ("TFLAG.CSV", "TFLAG"),
    ("CFLAG.CSV", "CFLAG"),
    ("TCVAR.CSV", "TCVAR"),
    ("CSTR.CSV", "CSTR"),
    ("STAIN.CSV", "STAIN"),
    ("MONEY.CSV", "MONEY"),
    ("DAY.CSV", "DAY"),
    ("TIME.CSV", "TIME"),
    ("CDFLAG1.CSV", "CDFLAG1"),
    ("CDFLAG2.CSV", "CDFLAG2"),
    ("STRNAME.CSV", "STRNAME"),
    ("TSTR.CSV", "TSTR"),
    ("SAVESTR.CSV", "SAVESTR"),
    ("GLOBAL.CSV", "GLOBAL"),
    ("GLOBALS.CSV", "GLOBALS"),
];

/// Decimal or `0x` hex.
fn parse_i64(s: &str) -> Option<i64> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

impl GameCsv {
    /// Read every CSV the engine knows from `csv_dir` (file names are
    /// matched case-insensitively), plus `*.ERD` tables under `erb_dir`.
    pub fn load(csv_dir: &Path, erb_dir: Option<&Path>) -> GameCsv {
        // Emuera-Anchor: PARAM.CSV, when there is one, in place of PALAM.CSV
        let mut g = GameCsv { param_csv: find_file_ci(csv_dir, "PARAM.CSV").is_some(), ..Default::default() };
        for (file, key) in TABLE_FILES {
            let mut t = NameTable::default();
            let file = if *key == "PALAM" && g.param_csv { "PARAM.CSV" } else { file };
            if let Some(p) = find_file_ci(csv_dir, file) {
                if let Ok(text) = read_text(&p) {
                    for line in crate::loader::emuera_lines(&text) {
                        let f = csv_fields(line);
                        if f.len() < 2 {
                            continue;
                        }
                        let Some(idx) = parse_i64(&f[0]) else { continue };
                        t.insert(idx, &f[1]);
                        if *key == "ITEM" {
                            if let Some(price) = f.get(2).and_then(|s| parse_i64(s)) {
                                g.item_price.insert(idx, price);
                            }
                        }
                    }
                }
            }
            // EMEE aliases: `ALS/<table>.als` (next to CSV/) gives the same
            // rows more names; lookups only, the CSV names stay the shown ones
            let als_dir = csv_dir.parent().and_then(|d| crate::loader::find_dir_ci(d, "ALS"));
            let als_name = format!("{}.ALS", file.trim_end_matches(".CSV"));
            if let Some(p) = als_dir.as_deref().and_then(|d| find_file_ci(d, &als_name)) {
                if let Ok(text) = read_text(&p) {
                    for line in crate::loader::emuera_lines(&text) {
                        let f = csv_fields(line);
                        if let (Some(idx), Some(name)) = (f.first().and_then(|s| parse_i64(s)), f.get(1)) {
                            t.add_alias(idx, name);
                        }
                    }
                }
            }
            g.tables.insert(key.to_string(), t);
        }
        if let Some(p) = find_file_ci(csv_dir, "STR.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        if let Some(i) = parse_i64(&f[0]) {
                            g.str_init.push((i, f[1..].join(",")));
                        }
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "GAMEBASE.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        g.gamebase.insert(f[0].clone(), f[1..].join(","));
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "VariableSize.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        let dims: Vec<usize> = f[1..].iter().filter_map(|s| s.parse::<i64>().ok()).map(|v| v.max(0) as usize).collect();
                        g.varsize.insert(f[0].to_uppercase(), dims);
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "_Replace.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        // keys in Japanese or English, spaces around them
                        // ignored (`COM_ABLE初期値 , 0`), as Emuera reads them
                        g.replace.insert(crate::config_names::canonical(&f[0]).to_string(), f[1..].join(","));
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "_Rename.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        // format: replacement,[[name]]
                        let key = f[1].trim_start_matches("[[").trim_end_matches("]]").to_string();
                        g.rename.push((key, f[0].clone()));
                    }
                }
            }
        }
        // Character templates.
        for p in walk(csv_dir, &["csv"]) {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
            if stem.starts_with("CHARA") {
                if let Ok(text) = read_text(&p) {
                    if let Some(c) = g.parse_chara(&text, &p) {
                        g.charas.push(c);
                    }
                }
            }
        }
        g.charas.sort_by_key(|c| c.no);
        // Emuera-Anchor translations
        g.tr = crate::translate::Tr::load(csv_dir);
        g.warnings.append(&mut g.tr.warnings);
        let w = crate::translate::apply_trchara(csv_dir, &mut g.charas);
        g.warnings.extend(w);
        // EmueraEE: name tables for user variables: *.ERD files anywhere
        // under ERB, and any CSV directly in the CSV folder named after one
        // (ErhLoader.PrepareERDFileNames: erablue_resort's RFLAG.csv for
        // `#DIM RFLAG`, eraFL's FLconfig.CSV for FLCONFIG)
        let builtin: std::collections::HashSet<String> = g.tables.keys().cloned().collect();
        let read_names = |p: &Path, t: &mut NameTable| {
            if let Ok(text) = read_text(p) {
                for line in crate::loader::emuera_lines(&text) {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        if let Some(i) = parse_i64(&f[0]) {
                            t.insert(i, &f[1]);
                        }
                    }
                }
            }
        };
        let mut csvs: Vec<std::path::PathBuf> = std::fs::read_dir(csv_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("csv")))
            .collect();
        csvs.sort();
        for p in csvs {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
            if !builtin.contains(&stem) && !stem.starts_with("CHARA") && !stem.starts_with("TRCHARA") && !stem.ends_with("_TR") {
                read_names(&p, g.tables.entry(stem).or_default());
            }
        }
        if let Some(erb) = erb_dir {
            for p in walk(erb, &["erd"]) {
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
                read_names(&p, g.tables.entry(stem).or_default());
            }
        }
        g
    }

    /// Index of `name` in a name table. Keys are stored upper-cased, so
    /// run-time strings (`GETNUM(TALENT, "Lunatic")`) match case-insensitively.
    pub fn lookup(&self, table: &str, name: &str) -> Option<i64> {
        let t = self.tables.get(table)?;
        let find = |n: &str| t.by_name.get(n).or_else(|| t.by_name.get(&n.to_uppercase())).copied();
        // Anchor games index by translated names too: `%PALAMNAME:i%` gives
        // the translation (Anchor runs with translation on), then
        // `PALAM:X:LOCALS` names it (eraSQN, eraSumireTeru; doc/anchor.md)
        find(name).or_else(|| if self.tr.active() { self.tr.original(table, name).and_then(find) } else { None })
    }

    pub fn name_of(&self, table: &str, idx: i64) -> Option<&str> {
        self.tables.get(table).and_then(|t| t.by_index.get(&idx)).map(|s| s.as_str())
    }

    /// A name as printed: translated when the game has *_TR.csv files.
    pub fn shown_name(&self, table: &str, idx: i64) -> Option<&str> {
        self.name_of(table, idx).map(|n| self.tr.or_same(table, n))
    }

    /// A Chara*.csv index column: a number or a name from `table`
    /// (warns and returns None for unknown names).
    fn idx(&mut self, table: &str, s: &str, file: &Path) -> Option<i64> {
        if let Some(v) = parse_i64(s) {
            return Some(v);
        }
        match self.lookup(table, s) {
            Some(v) => Some(v),
            None => {
                self.warnings.push(format!("{}: unknown {} name '{}'", file.display(), table, s));
                None
            }
        }
    }

    /// One character file: `番号,1` / `名前,…` / `能力,技巧,3` style rows.
    /// None when the file never gives a 番号: Emuera makes no character of
    /// it (eraMegaten keeps emptied files that hold only a comment, which
    /// made a second "character 0" here, ahead of the real one).
    fn parse_chara(&mut self, text: &str, file: &Path) -> Option<CharaTemplate> {
        let mut c = CharaTemplate::default();
        let mut numbered = false;
        for (line, joined) in joined_lines(text) {
            let line = line.as_str();
            let f = csv_fields(line);
            if f.is_empty() {
                continue;
            }
            // (Emuera upper-cases the key: eraMegaten writes `cstr`)
            let key = f[0].to_uppercase();
            let key = key.as_str();
            let a1 = f.get(1).map(|s| s.as_str()).unwrap_or("");
            let a2 = f.get(2).map(|s| s.as_str());
            let ival = |s: Option<&str>, d: i64| s.and_then(parse_i64).unwrap_or(d);
            // (ConstantData.loadCharacterDataFile: data before the 番号, a
            // 番号 that isn't a number and a second 番号 are warned about
            // and skipped)
            if matches!(key, "番号" | "NO") {
                match (numbered, parse_i64(a1)) {
                    (false, Some(n)) => (c.no, numbered) = (n, true),
                    (true, _) => self.warnings.push(format!("{}: a second 番号 (skipped)", file.display())),
                    (false, None) => self.warnings.push(format!("{}: 番号 '{}' isn't a number", file.display(), a1)),
                }
                continue;
            }
            if !numbered {
                self.warnings.push(format!("{}: '{}' before the 番号 (skipped)", file.display(), key));
                continue;
            }
            match key {
                "名前" | "NAME" => (c.name, c.names_set) = (a1.to_string(), c.names_set | 1),
                "呼び名" | "CALLNAME" => (c.callname, c.names_set) = (a1.to_string(), c.names_set | 2),
                "あだ名" | "NICKNAME" => (c.nickname, c.names_set) = (a1.to_string(), c.names_set | 4),
                "主人の呼び方" | "MASTERNAME" => (c.mastername, c.names_set) = (a1.to_string(), c.names_set | 8),
                "助手" | "ISASSI" => c.ints.push(("ISASSI".into(), 0, ival(Some(a1), 1))),
                "CSTR" => {
                    if let Some(i) = self.idx("CSTR", a1, file) {
                        // a joined value as Emuera keeps it: its third field
                        // whole, spaces and all (`strArray[index] = tokens[2]`)
                        let v = if joined { line.trim_start().split(',').nth(2).unwrap_or("") } else { a2.unwrap_or("") };
                        c.strs.push(("CSTR".into(), i, v.to_string()));
                    }
                }
                "相性" | "RELATION" => {
                    if let Some(i) = parse_i64(a1) {
                        c.ints.push(("RELATION".into(), i, ival(a2, 0)));
                    }
                }
                _ => {
                    let (var, table, default): (&str, &str, i64) = match key {
                        "基礎" | "BASE" => ("BASE", "BASE", 0),
                        "能力" | "ABL" => ("ABL", "ABL", 0),
                        "素質" | "TALENT" => ("TALENT", "TALENT", 1),
                        "経験" | "EXP" => ("EXP", "EXP", 0),
                        "刻印" | "MARK" => ("MARK", "MARK", 0),
                        "珠" | "JUEL" => ("JUEL", "PALAM", 0),
                        "フラグ" | "CFLAG" => ("CFLAG", "CFLAG", 0),
                        "装着物" | "EQUIP" => ("EQUIP", "EQUIP", 0),
                        "汚れ" | "STAIN" => ("STAIN", "STAIN", 0),
                        "EX" => ("EX", "EX", 0),
                        _ => {
                            self.warnings.push(format!("{}: unknown chara key '{}'", file.display(), key));
                            continue;
                        }
                    };
                    if let Some(i) = self.idx(table, a1, file) {
                        let v = ival(a2, default);
                        c.ints.push((var.to_string(), i, v));
                        if var == "BASE" {
                            c.ints.push(("MAXBASE".into(), i, v));
                        }
                    }
                }
            }
        }
        if c.callname.is_empty() {
            c.callname = c.name.clone();
        }
        numbered.then_some(c)
    }
}

/// A character file's lines, with Emuera's `{` … `}` joining
/// (`EraStreamReader.ReadEnabledLine`, which its CSV loading goes through
/// as ERB does): the lines between become one, each followed by a space
/// (`ReplaceContinuationBR`'s default). Each line comes with whether it was
/// joined; a block left open runs to the end of the file.
fn joined_lines(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut block: Option<String> = None;
    for raw in crate::loader::emuera_lines(text) {
        let t = raw.trim();
        match block.as_mut() {
            None if t == "{" => block = Some(String::new()),
            None => out.push((raw.to_string(), false)),
            Some(b) if t == "}" => {
                out.push((std::mem::take(b), true));
                block = None;
            }
            Some(b) => {
                b.push_str(raw);
                b.push(' ');
            }
        }
    }
    if let Some(b) = block {
        out.push((b, true));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brace_blocks_join() {
        let text = "番号,1\n{\nCSTR,性癖,\n精飲Lv0\n同性愛Lv1\n}\n名前,A\n{\nopen";
        let lines = joined_lines(text);
        let got: Vec<(&str, bool)> = lines.iter().map(|(l, j)| (l.as_str(), *j)).collect();
        assert_eq!(got, [("番号,1", false), ("CSTR,性癖, 精飲Lv0 同性愛Lv1 ", true), ("名前,A", false), ("open ", true)]);
    }

    #[test]
    fn chara_keys_and_joined_cstr() {
        // lower-case keys (eraMegaten's `cstr`), a joined CSTR kept whole
        let mut g = GameCsv::default();
        let t = g.parse_chara("番号,5\ncstr,0,abc\n{\nCSTR,1,\nx\ny\n}\n", Path::new("Chara5.csv")).unwrap();
        assert_eq!(t.no, 5);
        assert_eq!(t.strs, [("CSTR".to_string(), 0, "abc".to_string()), ("CSTR".to_string(), 1, " x y ".to_string())]);
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    }

    #[test]
    fn chara_files_without_a_number() {
        // no 番号: no character (not a second "character 0")
        let mut g = GameCsv::default();
        assert!(g.parse_chara(";emptied\n", Path::new("Chara4912.csv")).is_none());
        assert!(g.parse_chara("基礎,5,41\n", Path::new("Chara1.csv")).is_none());
        let t = g.parse_chara("名前,early\n番号,3\n番号,4\n名前,A\n", Path::new("Chara3.csv")).unwrap();
        assert_eq!((t.no, t.name.as_str()), (3, "A"));
        assert_eq!(g.warnings.len(), 3, "{:?}", g.warnings);
    }
}
