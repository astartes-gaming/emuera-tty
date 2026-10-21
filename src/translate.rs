//! Emuera-Anchor's translation system, for games translated with it: the
//! CSV files stay as written and translations sit beside them.
//!
//! - `XXXX_TR.csv` (`Abl_TR.csv`, `Talent_TR.csv`…) in the CSV folder
//!   itself: `original,translation` lines for one CSV name table; text
//!   after `;` is a comment. Applied when a name is read with Anchor's
//!   translate flag (printed, stored in a string, returned with RETURNF:
//!   `Vm::tr_read`), and to PRINT_ABL and the TRAIN and shop lists.
//! - `ALL_TR.csv`: a whole `%…%` value read with the translate flag.
//! - `TRCHARA*.csv`: character data (NAME, CALLNAME, NICKNAME, MASTERNAME,
//!   CSTR) in blocks from `NO,n` to `ENDCHR`; applied to the character
//!   templates, so characters added in a new game come out translated.
//! - `TR_NAME(code, index or name)`: a translated name for scripts.

use crate::ast::{AsgOp, BinOp, Expr, Form, FormPart, Instr, PrintContent};
use crate::csvdata::CharaTemplate;
use crate::fxhash::HashMap;
use crate::loader::{read_text, walk};
use std::path::Path;
use std::sync::Arc as Rc;

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

/// One character's block of a `TRCHARA*.csv` file.
#[derive(Default)]
struct TrChara {
    names: [Option<String>; 4],
    cstr: Vec<(i64, String)>,
}

/// End the open block: kept if its character is known and not given yet.
fn close_block(blocks: &mut HashMap<i64, TrChara>, cur: &mut Option<(i64, bool)>, block: &mut TrChara) {
    if let Some((no, true)) = cur.take() {
        blocks.insert(no, std::mem::take(block));
    }
    *block = TrChara::default();
}

/// Apply the `TRCHARA*.csv` files under `csv_dir` to the character
/// templates, as Emuera-Anchor does while it reads the Chara*.csv files
/// (`Translation.loadCharacterTranslationData`, `ConstantData` with
/// `translateChara`): a block from `NO,n` to `ENDCHR` / `ENDCHAR` gives
/// NAME, CALLNAME, NICKNAME, MASTERNAME (second field) and `CSTR,i,text`
/// (third field), and replaces only what the template's own Chara*.csv
/// sets. Problems are returned as load warnings, with file and line.
/// Where Anchor loses translations to a bug, this doesn't (see
/// doc/anchor.md, *Deliberate differences*): `;` lines are comments, a
/// bare `ENDCHR` ends a block, a second NO starts a new block, a last
/// block without ENDCHR applies, and a bad CSTR line or a NO given twice
/// is skipped rather than ending the file.
pub fn apply_trchara(csv_dir: &Path, charas: &mut [CharaTemplate]) -> Vec<String> {
    let mut warn = Vec::new();
    let mut blocks: HashMap<i64, TrChara> = HashMap::default();
    for p in walk(csv_dir, &["csv"]) {
        let name = p.file_name().map(|s| s.to_string_lossy().to_uppercase()).unwrap_or_default();
        if !name.starts_with("TRCHARA") {
            continue;
        }
        let Ok(text) = read_text(&p) else { continue };
        let mut w = |n: usize, m: String| warn.push(format!("{}:{}: {}", p.display(), n, m));
        // the open block: its number, and whether it is kept (a known
        // character, not given before)
        let mut cur: Option<(i64, bool)> = None;
        let mut block = TrChara::default();
        for (n, line) in enabled_lines(&text) {
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    w(n, format!("{} (skipped)", e));
                    continue;
                }
            };
            if line.starts_with(';') {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            let key = f[0].to_uppercase();
            if f.len() < 2 && !matches!(key.as_str(), "ENDCHR" | "ENDCHAR") {
                w(n, "a ',' is needed".into());
                continue;
            }
            if f[0].is_empty() {
                w(n, "the line starts with ','".into());
                continue;
            }
            if matches!(key.as_str(), "NO" | "番号") {
                if cur.is_some() {
                    w(n, "a new character number before ENDCHR (the block before it ends here)".into());
                    close_block(&mut blocks, &mut cur, &mut block);
                }
                let Ok(no) = f[1].trim().parse::<i64>() else {
                    w(n, format!("'{}' isn't an integer", f[1]));
                    continue;
                };
                let known = charas.iter().any(|c| c.no == no);
                if !known {
                    w(n, format!("no character {} to translate", no));
                } else if blocks.contains_key(&no) {
                    w(n, format!("character {} is translated twice (this block is ignored)", no));
                }
                cur = Some((no, known && !blocks.contains_key(&no)));
                continue;
            }
            if cur.is_none() {
                w(n, "data before the character number (NO)".into());
                continue;
            }
            let slot = match key.as_str() {
                "NAME" | "名前" => 0,
                "CALLNAME" | "呼び名" => 1,
                "NICKNAME" | "あだ名" => 2,
                "MASTERNAME" | "主人の呼び方" => 3,
                "ENDCHR" | "ENDCHAR" => {
                    close_block(&mut blocks, &mut cur, &mut block);
                    continue;
                }
                "CSTR" => {
                    if f.len() < 3 {
                        w(n, "CSTR needs a third field, the text (skipped)".into());
                        continue;
                    }
                    let Ok(i) = f[1].trim().parse::<i64>() else {
                        w(n, format!("CSTR index '{}' isn't an integer (skipped)", f[1]));
                        continue;
                    };
                    match block.cstr.iter_mut().find(|(k, _)| *k == i) {
                        Some(s) => {
                            w(n, format!("CSTR:{} is given twice in this block (the later one wins)", i));
                            s.1 = f[2].to_string();
                        }
                        None => block.cstr.push((i, f[2].to_string())),
                    }
                    continue;
                }
                _ => {
                    w(n, format!("'{}' cannot be interpreted", f[0]));
                    continue;
                }
            };
            block.names[slot] = Some(f[1].to_string());
        }
        if cur.is_some() {
            w(text.lines().count(), "the last character's block has no ENDCHR".into());
            close_block(&mut blocks, &mut cur, &mut block);
        }
    }
    for c in charas.iter_mut() {
        let Some(b) = blocks.get(&c.no) else { continue };
        let fields = [&mut c.name, &mut c.callname, &mut c.nickname, &mut c.mastername];
        for (k, (field, tr)) in fields.into_iter().zip(&b.names).enumerate() {
            // only a name the Chara*.csv gives (Anchor translates it as
            // it reads that line)
            if let (Some(tr), true) = (tr, c.names_set & (1 << k) != 0) {
                field.clone_from(tr);
            }
        }
        for (i, tr) in &b.cstr {
            if let Some(s) = c.strs.iter_mut().find(|(v, k, _)| v == "CSTR" && k == i) {
                s.2.clone_from(tr);
            }
        }
    }
    warn
}

// ------------------------------------------------------------ constants
//
// Anchor folds every constant part of an expression into a literal when it
// loads the script (`Restructure`), and translation hangs on that:
//
// - Under an operator, a built-in function or a FORM `%…%` (all of them
//   `FunctionMethodTerm`s), the parts are folded with the translate flag,
//   so a CSV name there comes out through its own table, whatever the
//   statement: `IF TALENTNAME:5 == "処女"` compares "Virgin" (Talent_TR)
//   and is false, where `IF TALENTNAME:LOCAL == "処女"` is true.
// - A constant that is a statement's whole argument is folded without it.
// - At run time a folded value read with the flag is looked up again, in
//   the table named by the line's text (`Term.cs` SingleTerm,
//   `searchParentFile`): the first `<TABLE>NAME` in it, else ALL_TR.
// - Some statements print or store a constant argument as folded, without
//   the flag (`ConstStr`): assignments, RETURNF, PRINT, PRINTFORM.
//
// emuera-tty doesn't fold; the parser marks the constants instead
// ([`fold_expr`], only in games with TR files), and the VM evaluates them
// as Anchor folded them (`Vm::tr_const`).

/// The game has Anchor TR files: the parser marks constants. Set once,
/// before the scripts are parsed.
pub static FOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn fold_on() -> bool {
    FOLD.load(std::sync::atomic::Ordering::Relaxed)
}

/// The internal function a marked constant is: `__TRCONST(expr, arg)`,
/// `arg` 1 when it is an operator's or function's argument.
pub const TRCONST: &str = "__TRCONST";

/// What `searchParentFile` looks for in a line, in its order (Anchor's
/// table list without ALL), and the table it means here. A plain
/// substring search: `CFLAGNAME` finds `FLAGNAME` first, as in Anchor.
const LINE_TABLES: [(&str, &str); 26] = [
    ("ABLNAME", "ABL"),
    ("TALENTNAME", "TALENT"),
    ("EXPNAME", "EXP"),
    ("MARKNAME", "MARK"),
    ("PALAMNAME", "PALAM"),
    ("PARAMNAME", "PALAM"),
    ("ITEMNAME", "ITEM"),
    ("TRAINNAME", "TRAIN"),
    ("BASENAME", "BASE"),
    ("SOURCENAME", "SOURCE"),
    ("EXNAME", "EX"),
    ("EQUIPNAME", "EQUIP"),
    ("TEQUIPNAME", "TEQUIP"),
    ("FLAGNAME", "FLAG"),
    ("TFLAGNAME", "TFLAG"),
    ("CFLAGNAME", "CFLAG"),
    ("TCVARNAME", "TCVAR"),
    ("CSTRNAME", "CSTR"),
    ("STAINNAME", "STAIN"),
    ("CDFLAG1NAME", "CDFLAG1"),
    ("CDFLAG2NAME", "CDFLAG2"),
    ("STRNAME", "STRNAME"),
    ("TSTRNAME", "TSTR"),
    ("SAVESTRNAME", "SAVESTR"),
    ("GLOBALNAME", "GLOBAL"),
    ("GLOBALSNAME", "GLOBALS"),
];

/// [`line_byte`]: the table for literals is ALL_TR.
pub const LINE_ALL: u8 = LINE_TABLES.len() as u8;
/// [`line_byte`]: the instruction's whole argument is a constant, which
/// Anchor prints or stores as folded (`ConstStr`), without the flag.
pub const LINE_CONST: u8 = 0x20;
/// [`line_byte`]: the line contains `RETURNF` (Anchor's percent fallbacks
/// skip such lines).
pub const LINE_RETURNF: u8 = 0x40;

/// The per-instruction byte of `Body::tr_line` for a line of script: the
/// table its literals are looked up in, and whether it has RETURNF.
pub fn line_byte(line: &str) -> u8 {
    // One pass over the bytes: at each "NAME", the first table (in
    // Anchor's order) whose name precedes it; and whether RETURNF occurs.
    let b = line.as_bytes();
    let (mut t, mut returnf) = (LINE_ALL, false);
    for i in 0..b.len() {
        match b[i] {
            b'N' if b[i..].starts_with(b"NAME") => {
                let before = &b[..i];
                if let Some(k) = LINE_TABLES.iter().position(|(p, _)| before.ends_with(&p.as_bytes()[..p.len() - 4])) {
                    t = t.min(k as u8);
                }
            }
            b'R' if b[i..].starts_with(b"RETURNF") => returnf = true,
            _ => {}
        }
    }
    if returnf {
        t | LINE_RETURNF
    } else {
        t
    }
}

/// The table a line's byte names ([`line_byte`]), ALL included.
pub fn line_table(byte: u8) -> &'static str {
    LINE_TABLES.get((byte & 0x1f) as usize).map_or("ALL", |t| t.1)
}

impl Tr {
    /// Is there an ALL_TR.csv?
    pub fn has_all(&self) -> bool {
        self.tables.contains_key("ALL")
    }

    /// `s` looked up in the table a line's byte names ([`line_byte`]).
    pub fn by_line(&self, byte: u8, s: &str) -> Option<&str> {
        match LINE_TABLES.get((byte & 0x1f) as usize) {
            Some((_, t)) => self.name(t, s),
            None => self.all(s).map(String::as_str),
        }
    }
}

/// Built-in functions Anchor folds when all their arguments are constant
/// (CanRestructure in its Creator.Method.cs; those with their own folding
/// rule are left out). Sorted.
const FOLDING_FNS: &[&str] = &[
    "ABS", "BARSTR", "CBRT", "CHARATU", "CHKFONT", "COLOR_FROMNAME", "COLOR_FROMRGB", "CONVERT", "CSVABL", "CSVBASE", "CSVCALLNAME", "CSVCFLAG", "CSVCSTR",
    "CSVEQUIP", "CSVEXP", "CSVJUEL", "CSVMARK", "CSVMASTERNAME", "CSVNAME", "CSVNICKNAME", "CSVRELATION", "CSVTALENT", "ENCODETOUNI", "ESCAPE", "EXISTCSV",
    "EXPONENT", "GETBIT", "GETCONFIG", "GETCONFIGS", "GETFOCUSCOLOR", "GETLINESTR", "INRANGE", "ISNUMERIC", "LIMIT", "LOG", "LOG10", "MAX", "MIN", "MONEYSTR",
    "POWER", "PRINTCLENGTH", "PRINTCPERLINE", "REPLACE", "SAVENOS", "SIGN", "SQRT", "STRCOUNT", "STRFIND", "STRFINDU", "STRLENS", "STRLENSU", "SUBSTRING",
    "SUBSTRINGU", "TOFULL", "TOHALF", "TOINT", "TOLOWER", "TOSTR", "TOUPPER", "TR_NAME", "UNICODE", "UNICODEBYTE",
];

/// A CSV name array (Anchor's constant arrays).
fn name_array(name: &str) -> bool {
    (name.ends_with("NAME") && crate::csvdata::table_for_var(name).is_some()) || matches!(name, "CDFLAGNAME1" | "CDFLAGNAME2")
}

/// A variable reference Anchor folds: an element of a CSV name array (the
/// array itself, `FINDELEMENT(BASENAME, …)`, stays a variable), or a
/// GAMEBASE value.
fn const_var(v: &crate::ast::VarRef) -> bool {
    if v.idx.is_empty() {
        v.name.starts_with("GAMEBASE_")
    } else {
        name_array(&v.name) && v.idx.iter().all(is_const)
    }
}

/// Built-ins with their own folding rule (`HasUniqueRestructure`): Anchor
/// doesn't fold their arguments, some of which are variables.
const OWN_FOLDING_FNS: &[&str] = &["CMATCH", "FINDELEMENT", "FINDLASTELEMENT", "GETNUM", "GETNUMB", "MATCH", "STRFORM", "VARSIZE"];

/// Would Anchor fold `e` into a literal?
pub fn is_const(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Str(_) | Expr::Empty => true,
        Expr::Var(v) => const_var(v),
        Expr::Unary(_, x) => is_const(x),
        Expr::Binary(_, a, b) => is_const(a) && is_const(b),
        Expr::Ternary(c, a, b) => is_const(c) && is_const(a) && is_const(b),
        Expr::Call(c) => c.0 == TRCONST || (FOLDING_FNS.binary_search(&c.0.as_str()).is_ok() && c.1.iter().all(is_const)),
        Expr::Form(f) => is_const_form(f),
        Expr::IncDec(..) => false,
    }
}

/// Would Anchor fold the whole FORM string into a literal?
pub fn is_const_form(f: &Form) -> bool {
    f.parts.iter().all(|p| match p {
        FormPart::Lit(_) => true,
        FormPart::Str(e, w, _) | FormPart::Int(e, w, _) => is_const(e) && w.as_deref().map(is_const) != Some(false),
        FormPart::Cond(c) => is_const(&c.0) && is_const_form(&c.1) && is_const_form(&c.2),
    })
}

/// Does a constant hold something folding with the flag changes: a CSV
/// name, or STRLENS (which measures the translated text)?
fn has_name(e: &Expr) -> bool {
    match e {
        Expr::Var(v) => (name_array(&v.name) && !v.idx.is_empty()) || v.idx.iter().any(has_name),
        Expr::Unary(_, x) => has_name(x),
        Expr::Binary(_, a, b) => has_name(a) || has_name(b),
        Expr::Ternary(c, a, b) => has_name(c) || has_name(a) || has_name(b),
        Expr::Call(c) => matches!(c.0.as_str(), "STRLENS" | "STRLENSU") || c.1.iter().any(has_name),
        Expr::Form(f) => form_has_name(f),
        _ => false,
    }
}

fn form_has_name(f: &Form) -> bool {
    f.parts.iter().any(|p| match p {
        FormPart::Lit(_) => false,
        FormPart::Str(e, w, _) | FormPart::Int(e, w, _) => has_name(e) || w.as_deref().is_some_and(has_name),
        FormPart::Cond(c) => has_name(&c.0) || form_has_name(&c.1) || form_has_name(&c.2),
    })
}

/// Could a constant be a string other than literal text (whose folded
/// value a flagged read looks up whole)? Not integer arithmetic.
fn has_text(e: &Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Str(_) | Expr::Empty | Expr::IncDec(..) => false,
        Expr::Unary(_, x) => has_text(x),
        Expr::Binary(_, a, b) => has_text(a) || has_text(b) || matches!((&**a, &**b), (Expr::Str(_), _) | (_, Expr::Str(_))),
        Expr::Ternary(c, a, b) => has_text(c) || has_text(a) || has_text(b),
        Expr::Form(f) => !f.parts.iter().all(|p| matches!(p, FormPart::Lit(_))),
        _ => true,
    }
}

/// Fold string literals joined or repeated (`"a" + "b"`, `"─" * 10`) into
/// one, as Anchor does when it loads the script.
fn fold_literals(e: &mut Expr) {
    let Expr::Binary(op, a, b) = e else { return };
    fold_literals(a);
    fold_literals(b);
    let s = match (*op, &**a, &**b) {
        (BinOp::Add, Expr::Str(x), Expr::Str(y)) => format!("{}{}", x, y),
        (BinOp::Mul, Expr::Str(x), Expr::Int(n)) | (BinOp::Mul, Expr::Int(n), Expr::Str(x)) if *n <= 256 => x.repeat((*n).max(0) as usize),
        _ => return,
    };
    *e = Expr::Str(s);
}

/// Mark the constants in a statement's expression (see the section notes).
pub fn fold_expr(mut e: Expr) -> Expr {
    if fold_on() && scan(&mut e) {
        finish(&mut e, false);
    }
    e
}

/// [`fold_expr`] for a statement's FORM string: each `%…%` / `{…}` part is
/// a function's argument.
pub fn fold_form(mut f: Form) -> Form {
    if fold_on() && scan_form(&mut f) {
        finish_form(&mut f);
    }
    f
}

/// A maximal constant, marked where folding matters: a CSV name in it, or
/// a whole value that a flagged read looks up by its line.
fn finish(e: &mut Expr, arg: bool) {
    fold_literals(e);
    if !has_name(e) && (arg || !has_text(e)) {
        return;
    }
    let inner = std::mem::replace(e, Expr::Empty);
    *e = Expr::Call(Box::new((TRCONST.into(), vec![inner, Expr::Int(arg as i64)], Default::default())));
}

/// Scan the children of an operator (or a FORM part), then, when they
/// aren't all constant, mark the constant ones, each a maximal constant, as
/// its arguments. Whether they are all constant.
fn scan_children<const N: usize>(mut kids: [&mut Expr; N]) -> bool {
    let mut consts = [false; N];
    for (k, c) in kids.iter_mut().zip(consts.iter_mut()) {
        *c = scan(k);
    }
    let all = consts.iter().all(|&c| c);
    if !all {
        for (k, c) in kids.into_iter().zip(consts) {
            if c {
                finish(k, true);
            }
        }
    }
    all
}

/// One pass, bottom up, in place: the maximal constants below a
/// non-constant node are marked. Whether the expression is constant itself
/// (then the node above decides).
fn scan(e: &mut Expr) -> bool {
    match e {
        Expr::Int(_) | Expr::Str(_) | Expr::Empty => true,
        // (its indices were marked when the variable was upper-cased)
        Expr::Var(_) => is_const(e),
        Expr::Unary(_, x) => scan_children([&mut **x]),
        Expr::Binary(_, a, b) => scan_children([&mut **a, &mut **b]),
        Expr::Ternary(x, a, b) => scan_children([&mut **x, &mut **a, &mut **b]),
        Expr::Call(call) => {
            let (n, args, _) = &mut **call;
            if n == TRCONST {
                return true;
            }
            // a built-in's arguments are a FunctionMethodTerm's; a game
            // function's (and PRINTFORMS' own) are folded on their own
            let arg = n != "__FORMS" && crate::vm::builtins::is_builtin_function(n);
            let consts: Vec<bool> = args.iter_mut().map(scan).collect();
            if OWN_FOLDING_FNS.contains(&n.as_str()) {
                return false;
            }
            let all = FOLDING_FNS.binary_search(&n.as_str()).is_ok() && consts.iter().all(|&c| c);
            if !all {
                for (a, c) in args.iter_mut().zip(consts) {
                    if c {
                        finish(a, arg);
                    }
                }
            }
            all
        }
        Expr::Form(f) => scan_form(Rc::make_mut(f)),
        _ => false,
    }
}

/// [`scan`] for a FORM string: its parts are functions' arguments.
fn scan_form(f: &mut Form) -> bool {
    let consts: Vec<bool> = f.parts.iter_mut().map(scan_part).collect();
    let all = consts.iter().all(|&c| c);
    if !all {
        for (p, c) in f.parts.iter_mut().zip(consts) {
            if c {
                finish_part(p);
            }
        }
    }
    all
}

fn scan_part(p: &mut FormPart) -> bool {
    match p {
        FormPart::Lit(_) => true,
        FormPart::Str(e, w, _) | FormPart::Int(e, w, _) => match w {
            Some(w) => scan_children([e, &mut **w]),
            None => scan_children([e]),
        },
        FormPart::Cond(cab) => {
            let (x, a, b) = &mut **cab;
            let cs = [scan(x), scan_form(a), scan_form(b)];
            let all = cs.iter().all(|&c| c);
            if !all {
                if cs[0] {
                    finish(x, true);
                }
                if cs[1] {
                    finish_form(a);
                }
                if cs[2] {
                    finish_form(b);
                }
            }
            all
        }
    }
}

/// The parts of a constant FORM string, each a maximal constant (a FORM
/// that is a statement's whole argument, or a constant part of one).
fn finish_form(f: &mut Form) {
    f.parts.iter_mut().for_each(finish_part);
}

fn finish_part(p: &mut FormPart) {
    match p {
        FormPart::Str(e, w, _) | FormPart::Int(e, w, _) => {
            finish(e, true);
            if let Some(w) = w {
                finish(w, true);
            }
        }
        FormPart::Cond(cab) => {
            let (x, a, b) = &mut **cab;
            finish(x, true);
            finish_form(a);
            finish_form(b);
        }
        FormPart::Lit(_) => {}
    }
}

/// Is this instruction's whole argument a constant Anchor prints or stores
/// as folded (`ConstStr`)? See [`LINE_CONST`].
pub fn const_arg(i: &Instr) -> bool {
    match i {
        Instr::Print(PrintContent::Form(f), _) => is_const_form(f),
        Instr::Assign(_, AsgOp::Set | AsgOp::SetExpr, rhs) => rhs.iter().all(is_const),
        Instr::AssignForm(_, AsgOp::Set | AsgOp::SetExpr, f) => is_const_form(f),
        Instr::ReturnF(Some(e)) => is_const(e),
        _ => false,
    }
}

/// The byte `FuncBuilder::emit` stores for an instruction, or None when
/// the game has no TR files. Its line's part is filled in afterwards by
/// [`set_line_bytes`].
pub fn instr_byte(i: &Instr) -> Option<u8> {
    fold_on().then(|| LINE_ALL | if const_arg(i) { LINE_CONST } else { 0 })
}

/// Can the instruction read with the translate flag (the only reads the
/// line's table matters for)?
fn reads_flagged(i: &Instr) -> bool {
    match i {
        Instr::Print(..) | Instr::PrintData(..) | Instr::Assign(..) | Instr::AssignForm(..) | Instr::ReturnF(_) => true,
        Instr::Command(c) => c.0 == "HTML_PRINT",
        _ => false,
    }
}

/// Fill in the line's part of the bytes of the instructions `line` made
/// (from `first` on). Only lines that read with the flag are searched.
pub fn set_line_bytes(body: &mut crate::ast::Body, first: usize, line: &str) {
    if !fold_on() || !body.code[first.min(body.code.len())..].iter().any(reads_flagged) {
        return;
    }
    let t = line_byte(line);
    for b in &mut body.tr_line[first..] {
        *b = (*b & LINE_CONST) | t;
    }
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
        let mut c = vec![chara(1, "博麗霊夢", "霊夢", &[(3, "巫女")])];
        assert!(apply_trchara(&d, &mut c).is_empty());
        assert_eq!((c[0].name.as_str(), c[0].callname.as_str()), ("Reimu Hakurei", "Reimu"));
        // the third field only, as Anchor (the text after its comma is lost)
        assert_eq!(c[0].strs, vec![("CSTR".to_string(), 3, "shrine".to_string())]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A template whose Chara*.csv gives NAME and CALLNAME, and CSTRs.
    fn chara(no: i64, name: &str, callname: &str, cstr: &[(i64, &str)]) -> CharaTemplate {
        CharaTemplate {
            no,
            name: name.into(),
            callname: callname.into(),
            names_set: 1 | 2,
            strs: cstr.iter().map(|(i, s)| ("CSTR".to_string(), *i, s.to_string())).collect(),
            ..Default::default()
        }
    }

    /// spec: TRCHARA files / Name with a suffix in a subfolder, Lower-case
    /// file name; Character blocks / Full block (ENDCHAR, any case keys)
    #[test]
    fn trchara_files_and_blocks() {
        let d = dir("trchara");
        std::fs::write(d.join("sub/TRCHARA(Reimu).csv"), "NO,1\nname,Reimu Hakurei,\nCALLNAME,Reimu,\nCSTR,3,shrine maiden\nENDCHAR,1,\n").unwrap();
        std::fs::write(d.join("trchara_main.CSV"), "番号,2\n呼び名,Marisa\nENDCHR\n").unwrap();
        let mut c = vec![chara(1, "博麗霊夢", "霊夢", &[(3, "巫女")]), chara(2, "霧雨魔理沙", "魔理沙", &[])];
        assert!(apply_trchara(&d, &mut c).is_empty(), "no warnings");
        assert_eq!((c[0].name.as_str(), c[0].callname.as_str(), c[0].strs[0].2.as_str()), ("Reimu Hakurei", "Reimu", "shrine maiden"));
        // spec: Block with only CALLNAME: the NAME stays the Chara*.csv's
        assert_eq!((c[1].name.as_str(), c[1].callname.as_str()), ("霧雨魔理沙", "Marisa"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Only what the Chara*.csv gives is replaced, as Anchor translates
    /// each field while it reads that line.
    #[test]
    fn trchara_replaces_only_given_fields() {
        let d = dir("trchara-only");
        std::fs::write(d.join("TRCHARA.csv"), "NO,1\nNICKNAME,Miko\nCSTR,4,new\nENDCHR,\n").unwrap();
        let mut c = vec![chara(1, "博麗霊夢", "霊夢", &[(3, "巫女")])];
        assert!(apply_trchara(&d, &mut c).is_empty());
        assert_eq!((c[0].nickname.as_str(), c[0].names_set), ("", 1 | 2), "no NICKNAME in the Chara*.csv");
        assert_eq!(c[0].strs.len(), 1, "no CSTR:4 in the Chara*.csv");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// spec: TRCHARA warnings / Unknown key, Missing ENDCHR; each names its
    /// file and line, and the rest still applies
    #[test]
    fn trchara_warnings() {
        let d = dir("trchara-warn");
        let text = "; a comment\nNAME,x\nNO,1\nTALENT,1,x\nCALLNAME,Reimu\nCSTR,3\nCSTR,3,a\nCSTR,3,b\nNO,2\nNAME,Marisa\nENDCHR,\nNO,9\nENDCHR,\nNO,abc\nnoComma\n,x\n";
        std::fs::write(d.join("TRCHARA.csv"), text).unwrap();
        let mut c = vec![chara(1, "博麗霊夢", "霊夢", &[(3, "巫女")]), chara(2, "霧雨魔理沙", "魔理沙", &[])];
        let w = apply_trchara(&d, &mut c);
        let at = |line: usize, what: &str| w.iter().any(|m| m.contains(&format!("TRCHARA.csv:{}: ", line)) && m.contains(what));
        assert!(at(2, "data before the character number"), "{:?}", w);
        assert!(at(4, "'TALENT' cannot be interpreted"), "{:?}", w);
        assert!(at(6, "CSTR needs a third field"), "{:?}", w);
        assert!(at(8, "CSTR:3 is given twice"), "{:?}", w);
        assert!(at(9, "a new character number before ENDCHR"), "{:?}", w);
        assert!(at(12, "no character 9"), "{:?}", w);
        assert!(at(14, "'abc' isn't an integer"), "{:?}", w);
        assert!(at(15, "a ',' is needed"), "{:?}", w);
        assert!(at(16, "starts with ','"), "{:?}", w);
        assert_eq!(w.len(), 9, "{:?}", w);
        // the rest of each block applied; NO,2 started its own block
        assert_eq!((c[0].callname.as_str(), c[0].strs[0].2.as_str()), ("Reimu", "b"));
        assert_eq!(c[1].name, "Marisa");
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

    /// The table for a line's literals, as Anchor's searchParentFile finds
    /// it: the first `<TABLE>NAME` in its text, by plain substring.
    #[test]
    fn line_tables() {
        let table = |l: &str| LINE_TABLES.get((line_byte(l) & 0x1f) as usize).map(|t| t.1);
        assert_eq!(table("PRINTS TALENTNAME:5"), Some("TALENT"));
        assert_eq!(table("PRINTS \"男\""), None, "ALL_TR");
        assert_eq!(table("PRINTV ABLNAME:1, TALENTNAME:5"), Some("ABL"));
        // CFLAGNAME contains FLAGNAME, which comes first in Anchor's order
        assert_eq!(table("PRINTS CFLAGNAME:3"), Some("FLAG"));
        assert_eq!(table("PRINTS \"x\" ; ITEMNAME in a comment counts"), Some("ITEM"));
        assert_eq!(line_byte("RETURNF TALENTNAME:5") & LINE_RETURNF, LINE_RETURNF);
        assert_eq!(line_byte("PRINTS 1") & LINE_RETURNF, 0);
    }

    /// What counts as a constant Anchor folds.
    #[test]
    fn constants() {
        let e = |s: &str| crate::parser::upper_expr(crate::expr::parse_expr(s, &Default::default()).unwrap());
        assert!(is_const(&e("TALENTNAME:5")));
        assert!(is_const(&e("\"[\" + TALENTNAME:5 + \"]\"")));
        assert!(is_const(&e("SUBSTRING(ABLNAME:1, 0, 2)")));
        assert!(!is_const(&e("TALENTNAME:LOCAL")));
        assert!(!is_const(&e("RAND(3)")), "not folded by Anchor");
        assert!(!is_const(&e("LOCALS + \"x\"")));
    }
}
