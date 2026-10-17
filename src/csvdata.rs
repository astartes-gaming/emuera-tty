//! CSV data: name tables (ABL.csv, TALENT.csv, ...), character templates
//! (Chara*.csv), GAMEBASE.csv, VariableSize.csv, _Rename.csv, *.ERD.
//!
//! Name tables are what make `ABL:技巧` work: each row `index,name` lets a
//! script use the name instead of the number. [`table_for_var`] says which
//! table a variable's index is looked up in (MAXBASE uses BASE.csv…).
//! Everything here is loaded once at startup and read-only afterwards.

use crate::loader::{csv_fields, find_file_ci, read_text, walk};
use std::collections::HashMap;
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
    pub warnings: Vec<String>,
}

/// Which name table a variable's last index is resolved against.
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
        let mut g = GameCsv::default();
        for (file, key) in TABLE_FILES {
            let mut t = NameTable::default();
            if let Some(p) = find_file_ci(csv_dir, file) {
                if let Ok(text) = read_text(&p) {
                    for line in text.lines() {
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
            g.tables.insert(key.to_string(), t);
        }
        if let Some(p) = find_file_ci(csv_dir, "STR.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in text.lines() {
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
                for line in text.lines() {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        g.gamebase.insert(f[0].clone(), f[1..].join(","));
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "VariableSize.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in text.lines() {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        let dims: Vec<usize> =
                            f[1..].iter().filter_map(|s| s.parse::<i64>().ok()).map(|v| v.max(0) as usize).collect();
                        g.varsize.insert(f[0].to_uppercase(), dims);
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "_Replace.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in text.lines() {
                    let f = csv_fields(line);
                    if f.len() >= 2 {
                        g.replace.insert(f[0].clone(), f[1..].join(","));
                    }
                }
            }
        }
        if let Some(p) = find_file_ci(csv_dir, "_Rename.CSV") {
            if let Ok(text) = read_text(&p) {
                for line in text.lines() {
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
                    let c = g.parse_chara(&text, &p);
                    g.charas.push(c);
                }
            }
        }
        g.charas.sort_by_key(|c| c.no);
        // EmueraEE: *.ERD name tables for user variables.
        if let Some(erb) = erb_dir {
            for p in walk(erb, &["erd"]) {
                let stem = p.file_stem().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
                let mut t = NameTable::default();
                if let Ok(text) = read_text(&p) {
                    for line in text.lines() {
                        let f = csv_fields(line);
                        if f.len() >= 2 {
                            if let Some(i) = parse_i64(&f[0]) {
                                t.insert(i, &f[1]);
                            }
                        }
                    }
                }
                g.tables.insert(stem, t);
            }
        }
        g
    }

    /// Index of `name` in a name table. Keys are stored upper-cased, so
    /// run-time strings (`GETNUM(TALENT, "Lunatic")`) match case-insensitively.
    pub fn lookup(&self, table: &str, name: &str) -> Option<i64> {
        let t = self.tables.get(table)?;
        t.by_name.get(name).or_else(|| t.by_name.get(&name.to_uppercase())).copied()
    }

    pub fn name_of(&self, table: &str, idx: i64) -> Option<&str> {
        self.tables.get(table).and_then(|t| t.by_index.get(&idx)).map(|s| s.as_str())
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
    fn parse_chara(&mut self, text: &str, file: &Path) -> CharaTemplate {
        let mut c = CharaTemplate::default();
        for line in text.lines() {
            let f = csv_fields(line);
            if f.is_empty() {
                continue;
            }
            let key = f[0].as_str();
            let a1 = f.get(1).map(|s| s.as_str()).unwrap_or("");
            let a2 = f.get(2).map(|s| s.as_str());
            let ival = |s: Option<&str>, d: i64| s.and_then(parse_i64).unwrap_or(d);
            match key {
                "番号" | "NO" => c.no = parse_i64(a1).unwrap_or(0),
                "名前" | "NAME" => c.name = a1.to_string(),
                "呼び名" | "CALLNAME" => c.callname = a1.to_string(),
                "あだ名" | "NICKNAME" => c.nickname = a1.to_string(),
                "主人の呼び方" | "MASTERNAME" => c.mastername = a1.to_string(),
                "助手" | "ISASSI" => c.ints.push(("ISASSI".into(), 0, ival(Some(a1), 1))),
                "CSTR" => {
                    if let Some(i) = self.idx("CSTR", a1, file) {
                        c.strs.push(("CSTR".into(), i, a2.unwrap_or("").to_string()));
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
        c
    }
}
