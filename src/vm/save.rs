//! Save files, in Emuera's binary format (see [`super::erasav`]) so that
//! saves move freely between emuera-tty and Emuera.
//!
//! | file | what | Emuera |
//! |------|------|--------|
//! | `saveNN.sav` | SAVEDATA / the save menu | `VariableEvaluator.SaveTo` |
//! | `global.sav` | SAVEGLOBAL | `SaveGlobal` |
//! | `dat/chara_NAME.dat` | SAVECHARA | `SaveChara` |
//!
//! The `.sav` files live in `<game>/sav` when the config says so
//! (`セーブデータをsavフォルダ内に作成する`, as AnonTW's does), else in the
//! game folder itself, like Emuera. What is saved is exactly Emuera's set:
//! the system variables `VariableToken.IsSavedata` marks, `#DIM SAVEDATA`
//! (and `#DIM GLOBAL SAVEDATA` in global.sav), every character's saved
//! variables and `#DIM CHARADATA SAVEDATA`, then the Emuera-EM maps, XML
//! documents and data tables VarExt*.csv names.
//!
//! Saves from emuera-tty before this (`sav/saveNN.rsav`, its own text
//! format) still load; the next save of that slot writes a `.sav`.

use super::erasav::{Entry, FileType, Reader, Values, Writer};
use super::value::*;
use super::vars::{system_var, Arr, Data};
use super::{Chara, Vm};
use std::fs;
use std::path::{Path, PathBuf};

/// Where and how saves are written (from the game's config).
#[derive(Debug, Clone)]
pub struct SaveOpts {
    /// `セーブデータをsavフォルダ内に作成する`: saves in `<game>/sav`
    pub in_folder: bool,
    /// `セーブデータを圧縮して保存する` (with the binary format): gzip
    pub zip: bool,
    /// `オートセーブを行なう` (default on): save on entering SHOP
    pub autosave: bool,
}

impl Default for SaveOpts {
    fn default() -> SaveOpts {
        SaveOpts { in_folder: false, zip: false, autosave: true }
    }
}

/// VarExt*.csv: which Emuera-EM maps / XML documents / data tables go into
/// saveNN.sav (`SAVE_*`) and global.sav (`GLOBAL_*`).
#[derive(Debug, Clone, Default)]
pub struct VarExt {
    pub save_maps: Vec<String>,
    pub global_maps: Vec<String>,
    pub save_xmls: Vec<String>,
    pub global_xmls: Vec<String>,
    pub save_dts: Vec<String>,
    pub global_dts: Vec<String>,
}

impl VarExt {
    /// Read every VarExt*.csv under `csv_dir` (subfolders included).
    pub fn load(csv_dir: Option<&Path>) -> VarExt {
        let mut ve = VarExt::default();
        let mut files = Vec::new();
        fn walk(d: &Path, out: &mut Vec<PathBuf>) {
            let Ok(rd) = fs::read_dir(d) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.starts_with("varext") && n.ends_with(".csv") {
                        out.push(p);
                    }
                }
            }
        }
        if let Some(d) = csv_dir {
            walk(d, &mut files);
        }
        files.sort();
        for f in files {
            let Ok(text) = crate::loader::read_text(&f) else { continue };
            for line in text.lines() {
                let line = line.split(';').next().unwrap_or("");
                let mut toks = line.split(',').map(|t| t.trim());
                let Some(kind) = toks.next() else { continue };
                let list = match kind.to_uppercase().as_str() {
                    "SAVE_MAPS" => &mut ve.save_maps,
                    "GLOBAL_MAPS" => &mut ve.global_maps,
                    "SAVE_XMLS" => &mut ve.save_xmls,
                    "GLOBAL_XMLS" => &mut ve.global_xmls,
                    "SAVE_DTS" => &mut ve.save_dts,
                    "GLOBAL_DTS" => &mut ve.global_dts,
                    _ => continue,
                };
                list.extend(toks.filter(|t| !t.is_empty()).map(|t| t.to_string()));
            }
        }
        ve
    }
}

/// System variables saved in saveNN.sav (non-character, non-global
/// `IsSavedata` in Emuera: DAY…Z below code 0x3C except NOTUSE_*, plus the
/// `__SAVE_EXTENDED__` ones).
const SAVED_GLOBALS: &[&str] = &[
    "DAY", "MONEY", "ITEM", "FLAG", "TFLAG", "UP", "PALAMLV", "EXPLV", "EJAC", "DOWN", "RESULT", "COUNT", "TARGET", "ASSI",
    "MASTER", "NOITEM", "LOSEBASE", "SELECTCOM", "ASSIPLAY", "PREVCOM", "TIME", "ITEMSALES", "PLAYER", "NEXTCOM", "PBAND",
    "BOUGHT", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V",
    "W", "X", "Y", "Z", "RANDDATA", "SAVESTR", "TSTR", "DITEMTYPE", "DA", "DB", "DC", "DD", "DE", "TA", "TB",
];

/// Saved character variables, in Emuera's order.
const SAVED_CHARA: &[&str] = &[
    "ISASSI", "NO", "BASE", "MAXBASE", "ABL", "TALENT", "EXP", "MARK", "PALAM", "SOURCE", "EX", "CFLAG", "JUEL", "RELATION",
    "EQUIP", "TEQUIP", "STAIN", "GOTJUEL", "NOWEX", "DOWNBASE", "CUP", "CDOWN", "TCVAR", "NAME", "CALLNAME", "NICKNAME",
    "MASTERNAME", "CSTR", "CDFLAG",
];

/// The ones Emuera keeps as single values rather than arrays.
fn chara_scalar(name: &str) -> bool {
    matches!(name, "ISASSI" | "NO" | "NAME" | "CALLNAME" | "NICKNAME" | "MASTERNAME")
}

/// The result of checking a save file (CHKDATA's return value and RESULTS).
#[derive(Debug, Clone, PartialEq)]
pub struct SaveCheck {
    /// 0 loadable, 1 no file, 2 another game's, 3 another version's, 4 broken
    pub state: i64,
    /// the save's comment, or why it can't be loaded
    pub mes: String,
    pub version: i64,
}

impl SaveCheck {
    fn missing() -> SaveCheck {
        SaveCheck { state: 1, mes: "----".into(), version: 0 }
    }
    fn broken(why: &str) -> SaveCheck {
        SaveCheck { state: 4, mes: why.into(), version: 0 }
    }
}

/// `GameBase.tryatoi`: the whole string as a number, else its leading digits.
fn tryatoi(s: &str) -> Option<i64> {
    if let Ok(n) = s.trim().parse() {
        return Some(n);
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// File names SAVECHARA may use (no folders).
fn dat_name_ok(n: &str) -> bool {
    !n.is_empty() && !n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) && n != "." && n != ".."
}

/// `*` / `?` wildcard match (FIND_CHARADATA).
fn wildcard(pat: &[char], s: &[char]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (Some('*'), _) => wildcard(&pat[1..], s) || (!s.is_empty() && wildcard(pat, &s[1..])),
        (Some('?'), Some(_)) => wildcard(&pat[1..], &s[1..]),
        (Some(p), Some(c)) if p.eq_ignore_ascii_case(c) => wildcard(&pat[1..], &s[1..]),
        _ => false,
    }
}

/// Copy a saved array into `a`: cleared first (Emuera's `needInit`), then
/// the saved elements that fit into `a`'s shape. A saved type or shape that
/// doesn't match is skipped, as in Emuera.
fn fill(a: &mut Arr, dims: &[usize], ints: &[(usize, i64)], strs: &[(usize, String)], is_str: bool) {
    if is_str != a.is_str() {
        return;
    }
    let ours: Vec<usize> = if a.dims.is_empty() { vec![1] } else { a.dims.clone() };
    let saved: Vec<usize> = if dims.is_empty() { vec![1] } else { dims.to_vec() };
    if ours.len() != saved.len() {
        return;
    }
    a.clear();
    // saved flat index → our flat index (None if outside our array)
    let map = |f: usize| -> Option<usize> {
        let mut rest = f;
        let mut coords = vec![0usize; saved.len()];
        for k in (0..saved.len()).rev() {
            let n = saved[k].max(1);
            coords[k] = rest % n;
            rest /= n;
        }
        let mut out = 0usize;
        for k in 0..ours.len() {
            if coords[k] >= ours[k] {
                return None;
            }
            out = out * ours[k] + coords[k];
        }
        Some(out)
    };
    if is_str {
        for (f, v) in strs {
            if let Some(i) = map(*f) {
                a.set(i, Value::Str(v.clone()));
            }
        }
    } else {
        for (f, v) in ints {
            if let Some(i) = map(*f) {
                a.set_i(i, *v);
            }
        }
    }
}

/// Write one array (scalars only where Emuera has them).
fn put(w: &mut Writer, key: &str, a: &Arr, scalar: bool) {
    let dims: Vec<usize> = if scalar {
        vec![]
    } else if a.dims.is_empty() {
        vec![1]
    } else {
        a.dims.clone()
    };
    match &a.data {
        Data::I(v) => w.var(key, &dims, Values::Int(v)),
        Data::S(v) => w.var(key, &dims, Values::Str(v)),
    }
}

impl Vm {
    /// Where saveNN.sav / global.sav go (Emuera's `Config.SavDir`).
    pub fn save_dir(&self) -> PathBuf {
        if self.save_opts.in_folder {
            self.game_dir.join("sav")
        } else {
            self.game_dir.clone()
        }
    }
    pub fn save_path(&self, n: i64) -> PathBuf {
        self.save_dir().join(format!("save{:02}.sav", n))
    }
    /// emuera-tty's earlier save files.
    fn old_save_path(&self, n: i64) -> PathBuf {
        self.game_dir.join("sav").join(format!("save{:02}.rsav", n))
    }
    fn dat_dir(&self) -> PathBuf {
        self.game_dir.join("dat")
    }

    /// GAMEBASE's コード / バージョン / バージョン違い認める.
    fn game_code(&self) -> i64 {
        self.csv.gamebase.get("コード").and_then(|s| tryatoi(s)).unwrap_or(0)
    }
    fn game_version(&self) -> (i64, bool) {
        match self.csv.gamebase.get("バージョン").and_then(|s| tryatoi(s)) {
            Some(v) => (v, true),
            None => (0, false),
        }
    }
    /// `GameBase.CheckVersion`.
    fn version_ok(&self, saved: i64) -> bool {
        let (ours, defined) = self.game_version();
        let min = self.csv.gamebase.get("バージョン違い認める").and_then(|s| tryatoi(s)).unwrap_or(0);
        (!defined && saved != 1000) || min <= saved || ours == saved
    }

    /// Open a save file and read its header; on success the reader is
    /// positioned after the comment.
    fn open_save(&self, path: &Path, ft: FileType) -> (SaveCheck, Option<Reader>) {
        let Ok(bytes) = fs::read(path) else { return (SaveCheck::missing(), None) };
        let Some(mut r) = Reader::open(&bytes) else {
            return (SaveCheck::broken("not an Emuera binary save (text-format saves aren't supported)"), None);
        };
        let head = (|| -> Result<(FileType, i64, i64, String), String> { Ok((r.file_type()?, r.raw_i64()?, r.raw_i64()?, r.string()?)) })();
        let Ok((t, code, version, text)) = head else { return (SaveCheck::broken("セーブデータが壊れています"), None) };
        if t != ft {
            return (SaveCheck::broken("セーブデータが壊れています"), None);
        }
        if code != 0 && code != self.game_code() {
            return (SaveCheck { state: 2, mes: "異なるゲームのセーブデータです".into(), version }, None);
        }
        if !self.version_ok(version) {
            return (SaveCheck { state: 3, mes: "セーブデータのバーションが異なります".into(), version }, None);
        }
        (SaveCheck { state: 0, mes: text, version }, Some(r))
    }

    /// CHKDATA / the save menu: can slot `n` be loaded, and its comment.
    pub fn check_save(&self, n: i64) -> SaveCheck {
        let path = self.save_path(n);
        if !path.exists() {
            if let Some(text) = self.peek_old_save(n) {
                return SaveCheck { state: 0, mes: text, version: 0 };
            }
        }
        self.open_save(&path, FileType::Normal).0
    }

    // ------------------------------------------------------------ writing

    /// A character's saved variables (Emuera's `CharacterData.SaveToStreamBinary`).
    fn put_chara(&mut self, w: &mut Writer, c: usize) {
        for name in SAVED_CHARA {
            let a = self.chara_arr(c, name);
            // Emuera skips strings never set (null); these two usually are
            if matches!(*name, "NICKNAME" | "MASTERNAME") && a.get(0).as_string().is_empty() {
                continue;
            }
            put(w, name, a, chara_scalar(name));
        }
        let mut user: Vec<(String, String, bool)> = self
            .user_dims
            .values()
            .filter(|d| d.is_chara)
            .map(|d| (d.name.clone(), d.decl_name.clone(), d.is_savedata && !d.is_global))
            .collect();
        if !user.is_empty() {
            user.sort();
            w.separator();
            for (name, decl, saved) in user {
                if saved {
                    let a = self.chara_arr(c, &name);
                    put(w, if decl.is_empty() { &name } else { &decl }, a, false);
                }
            }
        }
        w.eoc();
    }

    /// The user's `#DIM` (non-character) variables that go into the normal
    /// (`global` false) or global save.
    /// (upper-cased name, the name as declared: the key Emuera writes)
    fn saved_user_globals(&self, global: bool) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .user_dims
            .values()
            .filter(|d| !d.is_chara && !d.is_const && d.is_savedata && d.is_global == global)
            .map(|d| (d.name.clone(), if d.decl_name.is_empty() { d.name.clone() } else { d.decl_name.clone() }))
            .collect();
        v.sort();
        v
    }

    /// Emuera-EM maps / XML / data tables of one VarExt list group.
    fn put_em(&self, w: &mut Writer, maps: &[String], xmls: &[String], dts: &[String]) {
        for k in maps {
            if let Some(m) = self.maps.get(k) {
                let pairs: Vec<(&String, &String)> = m.pairs().collect();
                w.map(k, pairs.into_iter());
            }
        }
        for k in xmls {
            if let Some(d) = self.xml_docs.get(k) {
                w.xml(k, &crate::xml::doc_to_string(d));
            }
        }
        for k in dts {
            if let Some(t) = self.dts.get(k) {
                w.dt(k, &t.to_xml_schema(), &t.to_xml());
            }
        }
    }

    fn write_file(&self, path: &Path, data: Vec<u8>) -> VmResult<()> {
        if let Some(d) = path.parent() {
            let _ = fs::create_dir_all(d);
        }
        fs::write(path, data).map_err(|e| Ctrl::Error(format!("save failed: {}: {}", path.display(), e), false))
    }

    /// SAVEDATA / the save menu: slot `n` with comment `text`.
    pub fn save_slot(&mut self, n: i64, text: &str) -> VmResult<()> {
        let (version, _) = self.game_version();
        let mut w = Writer::begin(FileType::Normal, self.game_code(), version, text);
        w.raw_i64(self.charas.len() as i64);
        for c in 0..self.charas.len() {
            self.put_chara(&mut w, c);
        }
        for name in SAVED_GLOBALS {
            let a = self.global_arr(name);
            put(&mut w, name, a, false);
        }
        for (name, decl) in self.saved_user_globals(false) {
            if let Some(a) = self.globals.get(&name) {
                put(&mut w, &decl, a, false);
            }
        }
        w.eof();
        let ve = self.varext.clone();
        self.put_em(&mut w, &ve.save_maps, &ve.save_xmls, &ve.save_dts);
        w.eof();
        self.write_file(&self.save_path(n), w.finish(self.save_opts.zip))
    }

    /// SAVEGLOBAL.
    pub fn save_global(&mut self) -> VmResult<()> {
        let (version, _) = self.game_version();
        let mut w = Writer::begin(FileType::Global, self.game_code(), version, "");
        for name in ["GLOBAL", "GLOBALS"] {
            let a = self.global_arr(name);
            put(&mut w, name, a, false);
        }
        for (name, decl) in self.saved_user_globals(true) {
            if let Some(a) = self.globals.get(&name) {
                put(&mut w, &decl, a, false);
            }
        }
        w.eof();
        let ve = self.varext.clone();
        self.put_em(&mut w, &ve.global_maps, &ve.global_xmls, &ve.global_dts);
        w.eof();
        self.write_file(&self.save_dir().join("global.sav"), w.finish(self.save_opts.zip))
    }

    /// SAVECHARA name, comment, characters…
    pub fn save_chara(&mut self, name: &str, text: &str, list: &[usize]) -> VmResult<()> {
        if !dat_name_ok(name) {
            return err(format!("SAVECHARA: bad file name '{}'", name));
        }
        let (version, _) = self.game_version();
        let mut w = Writer::begin(FileType::CharVar, self.game_code(), version, text);
        w.raw_i64(list.len() as i64);
        for &c in list {
            self.put_chara(&mut w, c);
        }
        w.eof();
        self.write_file(&self.dat_dir().join(format!("chara_{}.dat", name)), w.finish(self.save_opts.zip))
    }

    // ------------------------------------------------------------ reading

    /// Read one character's entries into character `c`.
    fn read_chara(&mut self, r: &mut Reader, c: usize) -> Result<(), String> {
        let mut user = false;
        loop {
            match r.entry()? {
                Entry::Eoc | Entry::Eof => return Ok(()),
                Entry::Separator => user = true,
                Entry::Var { key, dims, ints, strs, is_str } => {
                    // Emuera writes `#DIM`s as declared (nCharaSaveUp) and
                    // matches names regardless of case
                    let key = key.to_uppercase();
                    let known = if user {
                        self.user_dims.get(&key).is_some_and(|d| d.is_chara && d.is_savedata && !d.is_global)
                    } else {
                        system_var(&key).is_some_and(|d| d.chara)
                    };
                    if known {
                        fill(self.chara_arr(c, &key), &dims, &ints, &strs, is_str);
                    }
                }
                _ => {}
            }
        }
    }

    /// Read a section of variables / EM data up to its EOF marker.
    fn read_section(&mut self, r: &mut Reader, global: bool) -> Result<(), String> {
        loop {
            match r.entry()? {
                Entry::Eof => return Ok(()),
                Entry::Var { key, dims, ints, strs, is_str } => {
                    let key = key.to_uppercase();
                    let known = if global {
                        key == "GLOBAL" || key == "GLOBALS" || self.user_dims.get(&key).is_some_and(|d| !d.is_chara && d.is_global && d.is_savedata)
                    } else {
                        SAVED_GLOBALS.contains(&key.as_str())
                            || self.user_dims.get(&key).is_some_and(|d| !d.is_chara && !d.is_global && d.is_savedata && !d.is_const)
                    };
                    if known {
                        let a = if self.globals.contains_key(&key) { self.globals.get_mut(&key).unwrap() } else { self.global_arr(&key) };
                        fill(a, &dims, &ints, &strs, is_str);
                    }
                }
                Entry::Map { key, pairs } => {
                    let ve = &self.varext;
                    if ve.save_maps.contains(&key) || ve.global_maps.contains(&key) {
                        let mut m = StrMap::default();
                        for (k, v) in pairs {
                            m.set(k, v);
                        }
                        self.maps.insert(key, m);
                    }
                }
                Entry::Xml { key, xml } => {
                    let ve = &self.varext;
                    if ve.save_xmls.contains(&key) || ve.global_xmls.contains(&key) {
                        if let Ok(d) = crate::xml::load_doc(&xml) {
                            self.xml_docs.insert(key, d);
                        }
                    }
                }
                Entry::Dt { key, schema, data } => {
                    let ve = &self.varext;
                    if ve.save_dts.contains(&key) || ve.global_dts.contains(&key) {
                        match super::datatable::DataTable::from_xml(&key, &schema, &data) {
                            Ok(t) => {
                                self.dts.insert(key, t);
                            }
                            Err(e) => self.warn(&format!("save data: table {}: {}", key, e)),
                        }
                    }
                }
                // stray character markers: nothing to do at this level
                Entry::Eoc | Entry::Separator => {}
            }
        }
    }

    /// Before loading: empty the EM data the save file replaces
    /// (`RemoveEMSaveData` / `RemoveEMGlobalData`).
    fn clear_em(&mut self, global: bool) {
        let ve = self.varext.clone();
        let (maps, xmls, dts) = if global { (&ve.global_maps, &ve.global_xmls, &ve.global_dts) } else { (&ve.save_maps, &ve.save_xmls, &ve.save_dts) };
        for k in maps {
            if let Some(m) = self.maps.get_mut(k) {
                m.clear();
            }
        }
        for k in xmls {
            self.xml_docs.remove(k);
        }
        for k in dts {
            if let Some(t) = self.dts.get_mut(k) {
                t.rows.clear();
            }
        }
    }

    /// LOADDATA / the load menu: replace the game state with slot `n`.
    /// False if there is nothing loadable there.
    pub fn load_slot(&mut self, n: i64) -> VmResult<bool> {
        let path = self.save_path(n);
        if !path.exists() && self.old_save_path(n).exists() {
            return self.load_old_slot(n);
        }
        let (chk, r) = self.open_save(&path, FileType::Normal);
        let Some(mut r) = r else {
            if chk.state != 1 {
                self.warn(&format!("save {}: {}", n, chk.mes));
            }
            return Ok(false);
        };
        self.init_data();
        self.charas.clear();
        self.clear_em(false);
        let res = (|| -> Result<(), String> {
            let count = r.raw_i64()?;
            if !(0..=1_000_000).contains(&count) {
                return Err("bad character count".into());
            }
            for c in 0..count as usize {
                self.charas.push(Chara::default());
                self.read_chara(&mut r, c)?;
            }
            self.read_section(&mut r, false)?;
            if !r.at_end() {
                self.read_section(&mut r, false)?;
            }
            Ok(())
        })();
        if let Err(e) = res {
            return err(format!("loading {}: {}", path.display(), e));
        }
        self.set_lastload(n, chk.mes, chk.version);
        Ok(true)
    }

    fn set_lastload(&mut self, n: i64, text: String, version: i64) {
        self.globals.entry("LASTLOAD_TEXT".into()).or_insert_with(|| Arr::new(true, &[1])).set(0, Value::Str(text));
        self.globals.entry("LASTLOAD_NO".into()).or_insert_with(|| Arr::new(false, &[1])).set(0, Value::Int(n));
        self.globals.entry("LASTLOAD_VERSION".into()).or_insert_with(|| Arr::new(false, &[1])).set(0, Value::Int(version));
    }

    /// LOADGLOBAL.
    pub fn load_global(&mut self) -> VmResult<bool> {
        let path = self.save_dir().join("global.sav");
        if !path.exists() {
            let old = self.game_dir.join("sav").join("global.rsav");
            return match fs::read_to_string(old) {
                Ok(s) if s.starts_with("EMUERA-RS-SAVE 1") => {
                    self.apply_old_lines(&s, true);
                    Ok(true)
                }
                _ => Ok(false),
            };
        }
        let (_, r) = self.open_save(&path, FileType::Global);
        let Some(mut r) = r else { return Ok(false) };
        self.clear_em(true);
        let res = (|| -> Result<(), String> {
            self.read_section(&mut r, true)?;
            if !r.at_end() {
                self.read_section(&mut r, true)?;
            }
            Ok(())
        })();
        match res {
            Ok(()) => Ok(true),
            Err(e) => {
                self.warn(&format!("global.sav: {}", e));
                Ok(false)
            }
        }
    }

    /// LOADCHARA name: append the file's characters. RESULT 1 on success.
    pub fn load_chara(&mut self, name: &str) -> VmResult<bool> {
        if !dat_name_ok(name) {
            return Ok(false);
        }
        let (_, r) = self.open_save(&self.dat_dir().join(format!("chara_{}.dat", name)), FileType::CharVar);
        let Some(mut r) = r else { return Ok(false) };
        let first = self.charas.len();
        let res = (|| -> Result<(), String> {
            let count = r.raw_i64()?;
            if !(0..=1_000_000).contains(&count) {
                return Err("bad character count".into());
            }
            for k in 0..count as usize {
                self.charas.push(Chara::default());
                self.read_chara(&mut r, first + k)?;
            }
            Ok(())
        })();
        if let Err(e) = res {
            self.charas.truncate(first);
            self.warn(&format!("chara_{}.dat: {}", name, e));
            return Ok(false);
        }
        Ok(true)
    }

    /// CHKCHARADATA name.
    pub fn check_chara(&self, name: &str) -> SaveCheck {
        if !dat_name_ok(name) {
            return SaveCheck::missing();
        }
        self.open_save(&self.dat_dir().join(format!("chara_{}.dat", name)), FileType::CharVar).0
    }

    /// FIND_CHARADATA pattern: names of the chara_*.dat files that match.
    pub fn find_chara_files(&self, pattern: &str) -> Vec<String> {
        let pat: Vec<char> = format!("chara_{}.dat", if pattern.is_empty() { "*" } else { pattern }).chars().collect();
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(self.dat_dir()) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                let cs: Vec<char> = n.chars().collect();
                if wildcard(&pat, &cs) {
                    if let Some(stem) = n.strip_prefix("chara_").and_then(|s| s.strip_suffix(".dat")) {
                        if !stem.is_empty() {
                            out.push(stem.to_string());
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// DELDATA n.
    pub fn delete_slot(&mut self, n: i64) {
        let _ = fs::remove_file(self.save_path(n));
        let _ = fs::remove_file(self.old_save_path(n));
    }

    // ------------------------------------------- emuera-tty's old format

    fn peek_old_save(&self, n: i64) -> Option<String> {
        let s = fs::read_to_string(self.old_save_path(n)).ok()?;
        let mut lines = s.lines();
        if lines.next()? != "EMUERA-RS-SAVE 1" {
            return None;
        }
        Some(lines.next().and_then(|l| l.strip_prefix("TEXT ")).map(old_unesc).unwrap_or_default())
    }

    fn load_old_slot(&mut self, n: i64) -> VmResult<bool> {
        let Ok(s) = fs::read_to_string(self.old_save_path(n)) else { return Ok(false) };
        if !s.starts_with("EMUERA-RS-SAVE 1") {
            return Ok(false);
        }
        self.init_data();
        self.charas.clear();
        self.apply_old_lines(&s, false);
        let text = self.peek_old_save(n).unwrap_or_default();
        self.set_lastload(n, text, 0);
        Ok(true)
    }

    /// The old format's `D` (shape), `G` (global) / `V` (character element)
    /// and `CHARA` lines.
    fn apply_old_lines(&mut self, s: &str, global_only: bool) {
        let mut cur_chara: Option<usize> = None;
        for line in s.lines() {
            if line == "CHARA" {
                self.charas.push(Chara::default());
                cur_chara = Some(self.charas.len() - 1);
                continue;
            }
            let mut parts = line.splitn(5, ' ');
            let tag = parts.next().unwrap_or("");
            match tag {
                "D" => {
                    let t = parts.next().unwrap_or("");
                    let name = parts.next().unwrap_or("").to_string();
                    let dims: Vec<usize> = parts.next().unwrap_or("").split(',').filter_map(|d| d.parse().ok()).collect();
                    let is_str = system_var(&name).map(|d| d.is_str).or_else(|| self.user_dims.get(&name).map(|d| d.is_str)).unwrap_or(false);
                    let arr = Arr::new(is_str, &dims);
                    match (t, cur_chara) {
                        ("G", _) if !global_only || name.starts_with("GLOBAL") || self.user_dims.get(&name).map(|d| d.is_global).unwrap_or(false) => {
                            self.globals.insert(name, arr);
                        }
                        ("V", Some(c)) => {
                            self.charas[c].vars.insert(name, arr);
                        }
                        _ => {}
                    }
                }
                "G" | "V" => {
                    let name = parts.next().unwrap_or("");
                    let idx: usize = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let ty = parts.next().unwrap_or("i");
                    let raw = parts.next().unwrap_or("");
                    let v = if ty == "s" { Value::Str(old_unesc(raw)) } else { Value::Int(raw.parse().unwrap_or(0)) };
                    if tag == "G" {
                        if let Some(a) = self.globals.get_mut(name) {
                            a.set(idx, v);
                        }
                    } else if let Some(c) = cur_chara {
                        if let Some(a) = self.charas[c].vars.get_mut(name) {
                            a.set(idx, v);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The old format's string escapes (`\n`, `\\`…).
fn old_unesc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('t') => o.push('\t'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_numbers() {
        assert_eq!(tryatoi("7153"), Some(7153));
        assert_eq!(tryatoi(" 12 "), Some(12));
        assert_eq!(tryatoi("0012"), Some(12));
        assert_eq!(tryatoi("103a"), Some(103));
        assert_eq!(tryatoi("proto"), None);
    }

    #[test]
    fn fill_maps_shapes() {
        let mut a = Arr::new(false, &[2, 3]);
        // saved as 3 x 4: (1,2) → flat 6, (2,0) is outside our 2 rows
        fill(&mut a, &[3, 4], &[(6, 5), (8, 9)], &[], false);
        assert_eq!(a.get_i(5), 5);
        assert_eq!((0..6).map(|i| a.get_i(i)).sum::<i64>(), 5);
        // type mismatch: untouched
        let mut s = Arr::new(true, &[2]);
        s.set(0, Value::Str("x".into()));
        fill(&mut s, &[2], &[(0, 1)], &[], false);
        assert_eq!(s.get(0), Value::Str("x".into()));
    }

    #[test]
    fn wildcards() {
        let m = |p: &str, s: &str| wildcard(&p.chars().collect::<Vec<_>>(), &s.chars().collect::<Vec<_>>());
        assert!(m("chara_*.dat", "chara_reimu.dat"));
        assert!(m("chara_r?imu.dat", "chara_reimu.dat"));
        assert!(!m("chara_*.dat", "var_x.dat"));
    }
}
