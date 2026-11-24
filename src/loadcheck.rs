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
//! is printed in the game's console at start in debug mode.

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
    let file = prog.files.get(w.file as usize).map_or("?", |s| s.as_str());
    em!("Error.Warning1", "警告Lv{0}:{1}:{2}行目:{3}", "Warning Lv{0}: {1}, line {2}: {3}", w.level, file, w.line, w.msg)
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
        let msg = em!(
            "Error.FuncIsAlreadyDefined",
            "関数\"@{0}\"は既に定義({1}の{2}行目)されています",
            "function \"@{0}\" is defined already (in {1}, line {2})",
            name,
            first,
            l1
        );
        found.push(Warning { level: 1, file, line, msg });
    }
    prog.warnings.extend(found);
    prog.warnings.sort_by_key(|w| (w.file, w.line));
}

/// Emuera.NET's CheckUTF8withBOM (its setting.json): a level-0 warning
/// for each script file without a UTF-8 BOM (Preload.ReadFileLines). The
/// files are read only when level 0 is shown.
pub fn bom_warnings(prog: &mut Program, cfg: &Values, game_dir: &std::path::Path) {
    if !shown(cfg, 0) || !crate::loader::net_settings(game_dir).check_utf8_bom {
        return;
    }
    let has_bom = |rel: &str| -> bool {
        use std::io::Read;
        let mut head = [0u8; 3];
        // a file that can't be read isn't this warning's business
        std::fs::File::open(game_dir.join(rel)).and_then(|mut f| f.read_exact(&mut head)).map_or(true, |_| head == [0xEF, 0xBB, 0xBF])
    };
    let found: Vec<Warning> = (0..prog.files.len())
        .filter(|&k| !prog.files[k].starts_with('(') && !has_bom(&prog.files[k]))
        // (Emuera.NET's message; its ID there, which EM's files don't have)
        .map(|k| Warning {
            level: 0,
            file: k as u32,
            line: 0,
            msg: em!("Error.FileNotUTF8BOM", "ファイルが UTF-8 with BOM ではありません", "the file isn't UTF-8 with a BOM"),
        })
        .collect();
    prog.warnings.extend(found);
    prog.warnings.sort_by_key(|w| (w.file, w.line));
}

/// The name tables Emuera reads from the CSV folder, in its order
/// (ConstantData.LoadData), as it names them in the report.
const REPORTED_CSV_TABLES: &[&str] = &[
    "VariableSize.CSV", "ABL.CSV", "EXP.CSV", "TALENT.CSV", "PALAM.CSV", "TRAIN.CSV", "MARK.CSV", "ITEM.CSV", "BASE.CSV", "SOURCE.CSV", "EX.CSV", "STR.CSV",
    "EQUIP.CSV", "TEQUIP.CSV", "FLAG.CSV", "TFLAG.CSV", "CFLAG.CSV", "TCVAR.CSV", "CSTR.CSV", "STAIN.CSV", "CDFLAG1.CSV", "CDFLAG2.CSV", "STRNAME.CSV",
    "TSTR.CSV", "SAVESTR.CSV", "GLOBAL.CSV", "GLOBALS.CSV", "DAY.CSV", "TIME.CSV", "MONEY.CSV",
];

/// Emuera's `{0}読み込み中・・・` report line for one file.
fn loading_line(name: &str) -> String {
    em!("SystemLine.LoadingFile", "{0}読み込み中・・・", "loading {0}…", name)
}

/// `path` under `base`, with `/` between folders (Emuera's relative names).
fn relative_name(path: &std::path::Path, base: &std::path::Path) -> String {
    path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Emuera's loading report (ロード時にレポートを表示する), the lines it
/// prints as it loads (Process.Initialize, ConstantData.LoadData,
/// ErhLoader / ErbLoader): macro.txt, _Replace.csv, _Rename.csv, each CSV
/// table, Chara*.csv and VarExt*.csv, each ERH and ERB file in load order,
/// then the two script passes. Empty when the setting is off. Emuera's
/// `ロード完了` comes after the warnings ([`load_complete`]).
pub fn loading_report(
    game: &std::path::Path,
    cfg: &Values,
    erb_dir: Option<&std::path::Path>,
    erhs: &[std::path::PathBuf],
    erbs: &[std::path::PathBuf],
) -> Vec<String> {
    use crate::loader::{emuera_files, find_dir_ci, find_file_ci, walk, Listing};
    if !cfg.bool("ロード時にレポートを表示する") {
        return Vec::new();
    }
    let mut out = Vec::new();
    if cfg.bool("キーボードマクロを使用する") && find_file_ci(game, "macro.txt").is_some() {
        out.push(em!("SystemLine.LoadingMacro", "macro.txt読み込み中・・・", "loading macro.txt…"));
    }
    let csv_dir = find_dir_ci(game, "CSV");
    let csv_has = |name: &str| csv_dir.as_deref().and_then(|d| find_file_ci(d, name)).is_some();
    if cfg.bool("_Replace.csvを利用する") && csv_has("_Replace.csv") {
        out.push(em!("SystemLine.LoadingReplace", "_Replace.csv読み込み中・・・", "loading _Replace.csv…"));
    }
    if cfg.bool("_Rename.csvを利用する") && csv_has("_Rename.csv") {
        out.push(em!("SystemLine.LoadingRename", "_Rename.csv読み込み中・・・", "loading _Rename.csv…"));
    }
    if let Some(csv) = csv_dir.as_deref() {
        out.extend(REPORTED_CSV_TABLES.iter().filter(|n| csv_has(n)).map(|n| loading_line(n)));
        // Chara*.csv as Emuera lists the folder (subfolders and order by
        // the same settings as the ERB folder's)
        let is_chara = |p: &std::path::PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_uppercase().starts_with("CHARA"));
        out.extend(emuera_files(csv, &["csv"], Listing::of(game, cfg)).iter().filter(|p| is_chara(p)).map(|p| loading_line(&relative_name(p, csv))));
        // VarExt*.csv anywhere under CSV, by file name
        let is_varext = |p: &std::path::PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("varext"));
        out.extend(walk(csv, &["csv"]).iter().filter(|p| is_varext(p)).filter_map(|p| p.file_name()).map(|n| loading_line(&n.to_string_lossy())));
    }
    if let Some(erb) = erb_dir {
        out.extend(erhs.iter().chain(erbs).map(|p| loading_line(&relative_name(p, erb))));
    }
    out.push(em!("SystemLine.BuildingUserFunc", "ユーザー定義関数のリストを構築中・・・", "building the list of the game's functions…"));
    out.push(em!("SystemLine.CheckingSyntax", "スクリプトの構文チェック中・・・", "checking the scripts' syntax…"));
    out
}

/// Emuera's `ロード完了`, the report's last line (after the warnings).
pub fn load_complete() -> String {
    em!("SystemLine.LoadComplete", "ロード完了", "loading finished")
}

/// The load warnings the game's 表示する最低警告レベル lets through (shown
/// in the console at start: `Vm::show_load_warnings`).
pub fn load_warnings<'a>(prog: &'a Program, cfg: &Values) -> Vec<&'a Warning> {
    prog.warnings.iter().filter(|w| shown(cfg, w.level)).collect()
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
                            let msg = em!("Error.NotDefinedFunc", "指定された関数名\"@{0}\"は存在しません", "there is no function named \"@{0}\"", n);
                            warnings.push(Warning { level: 2, file, line, msg });
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
                        let msg = em!(
                            "Error.IsDefinedErdVariable",
                            "変数名\"{0}\"は既にERD変数\"{1}\"の定義に使われており、予期せぬ配列参照を引き起こす場合があります",
                            "the variable name \"{0}\" is also a name in the ERD variable \"{1}\"; array indices may not refer to what you expect",
                            d.name,
                            table
                        );
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
                let msg = em!(
                    "Error.FuncNeverCalled",
                    "関数\"@{0}\"は定義されていますが一度も呼び出されません",
                    "function \"@{0}\" is defined but never called",
                    name
                );
                warnings.push(Warning { level: 1, file, line, msg });
            }
        }
    }
    warnings.sort_by_key(|w| (w.file, w.line));
    let mut out: Vec<String> = warnings.iter().filter(|w| shown(cfg, w.level)).map(|w| format(prog, w)).collect();
    if nc_ignored > 0 && shown(cfg, 1) && not_called != "IGNORE" {
        out.push(em!(
            "Error.IgnoredFuncNeverCalled",
            "警告Lv1:定義された関数が一度も呼び出されていない事に関する警告を{0}件無視しました",
            "Warning Lv1: {0} warnings about functions that are never called were left out",
            nc_ignored
        ));
    }
    if nf_ignored > 0 && shown(cfg, 2) && not_found != "IGNORE" {
        out.push(em!(
            "Error.IgnoredUndefinedFuncCall",
            "警告Lv2:定義されていない関数を呼び出した事に関する警告を{0}件無視しました",
            "Warning Lv2: {0} warnings about calls to functions that don't exist were left out",
            nf_ignored
        ));
    }
    if cfg.bool("ロード時にレポートを表示する") {
        let n_names = prog.funcs.names().count();
        let used = if use_callform { n_names } else { reached.len() };
        out.push(em!(
            "Error.TotalFunc",
            "非コメント行数:{0}, 全関数合計:{1}, 被呼出関数合計:{2}",
            "lines of code: {0}, functions: {1}, functions called: {2}",
            lines,
            n_names,
            used
        ));
    }
    // the built-in functions the game's own replace
    if cfg.bool("システム関数の上書きを許可する") && cfg.bool("システム関数が上書きされたとき警告を表示する") {
        let mut over: Vec<&str> = prog.funcs.names().map(|(n, _)| n).filter(|n| crate::vm::builtins::is_builtin_function(n)).collect();
        over.sort_unstable();
        if !over.is_empty() {
            out.push(em!("Error.OverWriteSystemFuncWarn1", "＊＊＊＊＊警告＊＊＊＊＊", "***** Warning *****"));
            out.extend(over.iter().map(|n| {
                em!(
                    "Error.OverWriteSystemFuncWarn2",
                    "  システム関数\"{0}\"がユーザー定義関数によって上書きされています",
                    "  the built-in function \"{0}\" is replaced by a function of the game's",
                    n
                )
            }));
            out.push(em!(
                "Error.OverWriteSystemFuncWarn3",
                " 上記の関数を利用するスクリプトは意図通りに動かない可能性があります",
                " scripts that use these functions may not work as intended"
            ));
            out.push("＊＊＊＊＊＊＊＊＊＊＊＊".into());
        }
    }
    out
}
