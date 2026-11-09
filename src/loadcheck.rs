//! Emuera's load warnings (ErbLoader, LogicalLineParser): what it reports
//! about a game's functions while loading, filtered by the game's settings
//! (表示する最低警告レベル, 関数が見つからない警告の扱い, 関数が呼び出されなかった
//! 警告の扱い, 同名の非イベント関数が複数定義されたとき警告する, システム関数が
//! 上書きされたとき警告を表示する, ERDで定義した識別子とローカル変数の重複を確認
//! する, ロード時にレポートを表示する), in Emuera's words and format.
//!
//! emuera-tty compiles function bodies when first called, so what needs
//! every body (calls to functions that don't exist, functions never
//! called) is worked out by `--check`; what the function headers tell
//! (duplicates, built-in functions overridden) is known at every start and
//! goes to the debug console in debug mode.

use crate::ast::*;
use crate::emuconfig::Values;
use crate::parser::Program;
use std::collections::{HashMap, HashSet, VecDeque};

/// One warning: Emuera's level (0-3), where, and the message.
#[derive(Clone, Debug)]
pub struct Warning {
    pub level: u8,
    pub file: u32,
    pub line: u32,
    pub msg: String,
}

/// Emuera's PrintWarning: `警告Lv1:file:12行目:message`.
pub fn format(prog: &Program, w: &Warning) -> String {
    format!("警告Lv{}:{}:{}行目:{}", w.level, prog.files.get(w.file as usize).map_or("?", |s| s.as_str()), w.line, w.msg)
}

/// Is a warning of this level shown (表示する最低警告レベル)?
fn shown(cfg: &Values, level: u8) -> bool {
    level as i64 >= cfg.value("表示する最低警告レベル").trim().parse::<i64>().unwrap_or(1)
}

/// What the headers tell, added to the program's warnings at load: names
/// defined twice (with 同名の非イベント関数が複数定義されたとき警告する;
/// event functions are meant to be).
pub fn at_load(prog: &mut Program, cfg: &Values) {
    if !cfg.bool("同名の非イベント関数が複数定義されたとき警告する") {
        return;
    }
    let mut found = Vec::new();
    for (name, (f1, l1), (file, line)) in prog.funcs.defined_again() {
        if crate::vm::is_event_name(name) {
            continue;
        }
        let first = prog.files.get(f1 as usize).cloned().unwrap_or_default();
        found.push(Warning { level: 1, file, line, msg: format!("関数\"@{}\"は既に定義({}の{}行目)されています", name, first, l1) });
    }
    prog.warnings.extend(found);
    prog.warnings.sort_by_key(|w| (w.file, w.line));
}

/// The load warnings shown, formatted (the debug console's, at start).
pub fn load_warnings(prog: &Program, cfg: &Values) -> Vec<String> {
    prog.warnings.iter().filter(|w| shown(cfg, w.level)).map(|w| format(prog, w)).collect()
}

/// Functions the system calls itself, and so called at load whether any
/// script calls them (IdentifierDictionary.IsEventLabelName /
/// IsSystemLabelName); #FUNCTION(S) count as called too (LogicalLineParser).
fn called_by_system(name: &str) -> bool {
    let numbered = |p: &str| name.strip_prefix(p).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    crate::vm::is_event_name(name)
        || matches!(
            name,
            "SHOW_STATUS"
                | "SHOW_USERCOM"
                | "USERCOM"
                | "SOURCE_CHECK"
                | "CALLTRAINEND"
                | "SHOW_JUEL"
                | "SHOW_ABLUP_SELECT"
                | "USERABLUP"
                | "SHOW_SHOP"
                | "SAVEINFO"
                | "USERSHOP"
                | "TITLE_LOADGAME"
                | "SYSTEM_AUTOSAVE"
                | "SYSTEM_TITLE"
                | "SYSTEM_LOADEND"
        )
        || numbered("COM")
        || numbered("COM_ABLE")
        || numbered("ABLUP")
}

/// How a "not found" / "never called" warning is handled
/// (DisplayWarningFlag): IGNORE, LATER, ONCE (the first of each file),
/// DISPLAY.
fn flag(cfg: &Values, key: &str) -> String {
    cfg.value(key).trim().to_uppercase()
}

/// Everything Emuera reports while loading, for `--check` (every body
/// compiled): the load warnings, calls to functions that don't exist,
/// functions never called, private variables named like ERD names, and the
/// summaries and report Emuera prints after them.
pub fn check(prog: &Program, csv: &crate::csvdata::GameCsv, cfg: &Values) -> Vec<String> {
    let mut warnings: Vec<Warning> = prog.warnings.clone();
    let not_found = flag(cfg, "関数が見つからない警告の扱い");
    let not_called = flag(cfg, "関数が呼び出されなかった警告の扱い");
    // calls: by name, which functions each calls (CALL, JUMP, CALLF… to a
    // constant name); a name worked out at run time (CALLFORM, the call
    // lists) makes every function count as called, as in Emuera
    let mut calls: HashMap<String, Vec<String>> = HashMap::new();
    let mut use_callform = false;
    let (mut nf_files, mut nf_ignored) = (HashSet::new(), 0usize);
    let erd = cfg.bool("ERD機能を利用する") && cfg.bool("ERDで定義した識別子とローカル変数の重複を確認する");
    let mut lines = 0usize;
    for f in prog.funcs.made() {
        let body = f.body();
        lines += body.lines.len();
        let out = calls.entry(f.name.to_string()).or_default();
        for (pc, ins) in body.code.iter().enumerate() {
            match ins {
                Instr::Call(c) => match &c.target {
                    CallTarget::Static(n) if prog.funcs.contains_key(n) => out.push(n.to_string()),
                    CallTarget::Static(n) if !c.try_ => {
                        // (shown only if level 2 is, and as the setting says)
                        let (file, line) = body.lines.get(pc).copied().unwrap_or((f.file, f.line));
                        let ignore = match not_found.as_str() {
                            "IGNORE" => true,
                            "ONCE" => !nf_files.insert(file),
                            _ => false,
                        };
                        if ignore {
                            nf_ignored += 1;
                        } else {
                            warnings.push(Warning { level: 2, file, line, msg: format!("指定された関数名\"@{}\"は存在しません", n) });
                        }
                    }
                    CallTarget::Static(_) => {}
                    CallTarget::Form(_) => use_callform = true,
                },
                Instr::CallList(..) => use_callform = true,
                _ => {}
            }
        }
        // ERDで定義した識別子とローカル変数の重複を確認する
        if erd {
            for d in body.dims.iter().filter(|d| !d.is_ref) {
                for (table, t) in &csv.tables {
                    if !crate::csvdata::TABLE_FILES.iter().any(|(_, k)| *k == table) && t.by_index.values().any(|n| n.eq_ignore_ascii_case(&d.name)) {
                        let msg = format!("変数名\"{}\"は既にERD変数\"{}\"の定義に使われており、予期せぬ配列参照を引き起こす場合があります", d.name, table);
                        warnings.push(Warning { level: 1, file: f.file, line: f.line, msg });
                    }
                }
            }
        }
    }
    // the functions reached from those the system calls
    let mut reached: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = prog.funcs.made().filter(|f| f.is_function || called_by_system(&f.name)).map(|f| f.name.to_string()).collect();
    while let Some(n) = queue.pop_front() {
        if !reached.contains(&n) {
            queue.extend(calls.get(&n).into_iter().flatten().cloned());
            reached.insert(n);
        }
    }
    let mut nc_ignored = 0usize;
    if !use_callform {
        let ignore_all = matches!(not_called.as_str(), "IGNORE" | "LATER");
        let mut files = HashSet::new();
        let mut seen = HashSet::new();
        for (name, file, line) in prog.funcs.where_defined() {
            if reached.contains(name) || !seen.insert(name) {
                continue;
            }
            if ignore_all || (not_called == "ONCE" && !files.insert(file)) {
                nc_ignored += 1;
            } else {
                warnings
                    .push(Warning { level: 1, file, line, msg: format!("関数\"@{}\"は定義されていますが一度も呼び出されません", name) });
            }
        }
    }
    warnings.sort_by_key(|w| (w.file, w.line));
    let mut out: Vec<String> = warnings.iter().filter(|w| shown(cfg, w.level)).map(|w| format(prog, w)).collect();
    if nc_ignored > 0 && shown(cfg, 1) && not_called != "IGNORE" {
        out.push(format!("警告Lv1:定義された関数が一度も呼び出されていない事に関する警告を{}件無視しました", nc_ignored));
    }
    if nf_ignored > 0 && shown(cfg, 2) && not_found != "IGNORE" {
        out.push(format!("警告Lv2:定義されていない関数を呼び出した事に関する警告を{}件無視しました", nf_ignored));
    }
    if cfg.bool("ロード時にレポートを表示する") {
        let n_names = prog.funcs.names().count();
        let used = if use_callform { n_names } else { reached.len() };
        out.push(format!("非コメント行数:{}, 全関数合計:{}, 被呼出関数合計:{}", lines, n_names, used));
    }
    // the built-in functions the game's own replace
    if cfg.bool("システム関数の上書きを許可する") && cfg.bool("システム関数が上書きされたとき警告を表示する") {
        let mut over: Vec<&str> = prog.funcs.names().map(|(n, _)| n).filter(|n| crate::vm::builtins::is_builtin_function(n)).collect();
        over.sort_unstable();
        if !over.is_empty() {
            out.push("＊＊＊＊＊警告＊＊＊＊＊".into());
            out.extend(over.iter().map(|n| format!("  システム関数\"{}\"がユーザー定義関数によって上書きされています", n)));
            out.push(" 上記の関数を利用するスクリプトは意図通りに動かない可能性があります".into());
            out.push("＊＊＊＊＊＊＊＊＊＊＊＊".into());
        }
    }
    out
}
