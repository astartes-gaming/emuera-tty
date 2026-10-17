//! Save files (own text format, not compatible with Emuera's .sav).
//!
//! ```text
//! EMUERA-RS-SAVE 1
//! TEXT <escaped comment>
//! G <var> <flat> i|s <value>
//! CHARA
//! V <var> <flat> i|s <value>
//! ```
//!
//! One line per non-default element, so files stay small and diffable.
//! `G` lines are globals, each `CHARA` line starts the next character and
//! is followed by its `V` lines. Saves live in `<game>/sav/saveNN.rsav`,
//! global data (SAVEGLOBAL) in `global.rsav`. Temporary variables
//! (`is_temp_var`) are not saved. The header name predates the rename to
//! emuera-tty and stays so old saves keep loading.
//!
//! Not saved yet: Emuera-EM maps / data tables named in VarExt.csv.

use super::value::*;
use super::vars::{is_temp_var, Arr, Data};
use super::{Chara, Vm};
use std::fs;
use std::path::PathBuf;

/// Escape a string value so it fits on one line (`\n`, `\\`…).
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

/// Reverse of [`esc`].
fn unesc(s: &str) -> String {
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

/// Append one line per non-zero / non-empty element of `a`.
fn dump_arr(out: &mut String, tag: &str, name: &str, a: &Arr) {
    // record dims so multi-D shapes survive
    out.push_str(&format!("D {} {} {}\n", tag, name, a.dims.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(",")));
    match &a.data {
        Data::I(v) => {
            for (i, x) in v.iter().enumerate() {
                if *x != 0 {
                    out.push_str(&format!("{} {} {} i {}\n", tag, name, i, x));
                }
            }
        }
        Data::S(v) => {
            for (i, x) in v.iter().enumerate() {
                if !x.is_empty() {
                    out.push_str(&format!("{} {} {} s {}\n", tag, name, i, esc(x)));
                }
            }
        }
    }
}

impl Vm {
    pub fn save_dir(&self) -> PathBuf {
        self.game_dir.join("sav")
    }
    pub fn save_path(&self, n: i64) -> PathBuf {
        self.save_dir().join(format!("save{:02}.rsav", n))
    }

    /// Is this global part of SAVEGLOBAL (GLOBAL/GLOBALS, `#DIM GLOBAL`)?
    fn is_saved_global(&self, name: &str) -> bool {
        if let Some(d) = self.user_dims.get(name) {
            return d.is_savedata && !d.is_global;
        }
        !is_temp_var(name) && name != "SAVEDATA_TEXT" && name != "LASTLOAD_TEXT"
    }

    pub fn save_slot(&mut self, n: i64, text: &str) -> VmResult<()> {
        let mut out = String::from("EMUERA-RS-SAVE 1\n");
        out.push_str(&format!("TEXT {}\n", esc(text)));
        out.push_str(&format!("CODE {}\n", self.csv.gamebase.get("コード").cloned().unwrap_or_default()));
        let mut names: Vec<String> = self.globals.keys().cloned().collect();
        names.sort();
        for name in names {
            if self.is_saved_global(&name) {
                dump_arr(&mut out, "G", &name, &self.globals[&name]);
            }
        }
        for c in &self.charas {
            out.push_str("CHARA\n");
            let mut names: Vec<&String> = c.vars.keys().collect();
            names.sort();
            for name in names {
                dump_arr(&mut out, "V", name, &c.vars[name]);
            }
        }
        let _ = fs::create_dir_all(self.save_dir());
        fs::write(self.save_path(n), out).map_err(|e| Ctrl::Error(format!("save failed: {}", e), false))
    }

    /// The comment line of slot `n` (for the save/load menu), if it exists.
    pub fn peek_save(&self, n: i64) -> Option<String> {
        let s = fs::read_to_string(self.save_path(n)).ok()?;
        let mut lines = s.lines();
        if lines.next()? != "EMUERA-RS-SAVE 1" {
            return None;
        }
        Some(lines.next().and_then(|l| l.strip_prefix("TEXT ")).map(unesc).unwrap_or_default())
    }

    /// Load G/CHARA/V lines into the current state.
    fn apply_lines(&mut self, s: &str, global_only: bool) {
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
                    let is_str = super::vars::system_var(&name)
                        .map(|d| d.is_str)
                        .or_else(|| self.user_dims.get(&name).map(|d| d.is_str))
                        .unwrap_or(false);
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
                    let v = if ty == "s" { Value::Str(unesc(raw)) } else { Value::Int(raw.parse().unwrap_or(0)) };
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

    pub fn load_slot(&mut self, n: i64) -> VmResult<bool> {
        let Ok(s) = fs::read_to_string(self.save_path(n)) else { return Ok(false) };
        if !s.starts_with("EMUERA-RS-SAVE 1") {
            return Ok(false);
        }
        self.init_data();
        self.charas.clear();
        self.apply_lines(&s, false);
        let text = self.peek_save(n).unwrap_or_default();
        self.globals.entry("LASTLOAD_TEXT".into()).or_insert_with(|| Arr::new(true, &[1])).set(0, Value::Str(text));
        self.globals.entry("LASTLOAD_NO".into()).or_insert_with(|| Arr::new(false, &[1])).set(0, Value::Int(n));
        Ok(true)
    }

    pub fn save_global(&mut self) -> VmResult<()> {
        let mut out = String::from("EMUERA-RS-SAVE 1\nTEXT global\n");
        let mut names: Vec<String> = self.globals.keys().cloned().collect();
        names.sort();
        for name in names {
            let is_g = name == "GLOBAL" || name == "GLOBALS" || self.user_dims.get(&name).map(|d| d.is_global).unwrap_or(false);
            if is_g {
                dump_arr(&mut out, "G", &name, &self.globals[&name]);
            }
        }
        let _ = fs::create_dir_all(self.save_dir());
        fs::write(self.save_dir().join("global.rsav"), out).map_err(|e| Ctrl::Error(format!("save failed: {}", e), false))
    }

    pub fn load_global(&mut self) -> VmResult<bool> {
        let Ok(s) = fs::read_to_string(self.save_dir().join("global.rsav")) else { return Ok(false) };
        self.apply_lines(&s, true);
        Ok(true)
    }
}
