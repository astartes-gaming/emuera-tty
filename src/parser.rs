//! ERB/ERH statement parser: turns source files into a [`Program`].
//!
//! Pipeline per file:
//!
//! 1. `logical_lines` — strip comments-only lines, apply `[SKIPSTART]`,
//!    `[IF_DEBUG]`/`[IF X]` conditional blocks, `[[rename]]` substitution
//!    and `{ … }` line joining, yielding (line number, text) pairs.
//! 2. `.ERH` headers (`parse_erh`): `#DEFINE`s first, then global `#DIM`s.
//! 3. `.ERB` files (`parse_erb`): an `@NAME(args)` line starts a function;
//!    every following line goes through `statement`, which emits
//!    [`Instr`]s into a [`FuncBuilder`].
//!
//! Block statements are compiled with a stack of open blocks (`Ctl`): IF
//! emits a `JumpIfNot` with a placeholder target, ELSE/ENDIF *patch* it
//! once the target is known, BREAK/CONTINUE are recorded on the innermost
//! loop and patched at NEXT/WEND/LOOP. SIF is special: its jump is patched
//! after the next statement, whatever it is (`after_stmt`).
//!
//! A line that fails to parse becomes an [`Instr::BadLine`] rather than
//! disappearing, so jumps and SIF still line up and the error surfaces if
//! the line is actually executed.

use crate::ast::*;
use crate::expr::{parse_expr, parse_expr_list, parse_form, split_top, ExprParser};
use crate::fxhash::{HashMap, HashSet};
// Arc (aliased as Rc): parsed programs are built on several threads
use std::sync::Arc as Rc;

/// One loaded .ERB/.ERH file (path relative to the game, decoded text).
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

/// Everything compiled from a game: what the VM runs.
#[derive(Default)]
pub struct Program {
    pub files: Vec<String>,
    /// Functions by uppercase name; several entries == event function.
    pub funcs: Funcs,
    /// `#DIM`s from .ERH files, allocated at game start / RESETDATA.
    pub global_dims: Vec<DimDecl>,
    /// `#DEFINE NAME body`: expanded textually inside expressions.
    pub defines: HashMap<String, String>,
    /// Parse errors, "file:line: message"; shown by `--check`.
    pub errors: Vec<String>,
}

/// Functions by upper-case name; several of one name are an event's
/// handlers, in file order.
///
/// Big games have about 100,000 functions and a session calls a few
/// thousand, so a function is kept as a [`Head`] (its name, where it is and
/// its `#` attributes, 56 bytes) until it is first looked up; the
/// [`Function`] (parameters, the place for its compiled body) is made then,
/// from its file, like its body. A full `Function` for each took 20 MB of
/// AnonTW's memory, the heads take 8.
#[derive(Default)]
pub struct Funcs {
    /// every name, back to back ([`Head::name`] is a range of it)
    names: String,
    heads: Vec<Head>,
    /// the first head of each name, by name: open addressing, each entry
    /// the name's hash (its top half, to skip others without reading
    /// their names) and the head; `FREE` for none
    table: Vec<u64>,
    /// the number of names
    n_names: usize,
    /// `#LOCALSIZE` / `#LOCALSSIZE` of the heads that have them (few do)
    sizes: HashMap<u32, (u32, u32)>,
    /// for each name with several functions: all of them, made on first use
    groups: Vec<std::sync::OnceLock<Box<[Rc<Function>]>>>,
    ctx: Option<Rc<LazyCtx>>,
}

const NONE: u32 = u32::MAX;
const FREE: u64 = u64::MAX;

/// A function before it is looked up (see [`Funcs`]).
struct Head {
    name: (u32, u32),
    /// the next head of the same name, in file order
    next: u32,
    /// on the first head of a name with several: its [`Funcs::groups`] entry
    group: u32,
    src: Src,
    attrs: Attrs,
    /// the function, made on first use (set at once for those compiled at
    /// load)
    one: std::sync::OnceLock<Rc<Function>>,
}

/// Where a function's text is (see [`LazySrc`]).
#[derive(Clone, Copy)]
struct Src {
    file: u32,
    line: u32,
    start: u32,
    end: u32,
    open_ifs: u16,
    ord: u16,
}

/// The `#` attributes of a function, read at load (`#LOCALSIZE`s are in
/// [`Funcs::sizes`]).
#[derive(Clone, Copy)]
struct Attrs {
    is_function: bool,
    returns_str: bool,
    order: EventOrder,
    single: bool,
    only: bool,
}

impl Attrs {
    fn of(f: &Function) -> Attrs {
        Attrs { is_function: f.is_function, returns_str: f.returns_str, order: f.order, single: f.single, only: f.only }
    }
}

fn name_hash(name: &str) -> u64 {
    use std::hash::Hasher;
    let mut h = crate::fxhash::FxHasher::default();
    h.write(name.as_bytes());
    h.finish()
}

impl Funcs {
    fn name_of(&self, h: &Head) -> &str {
        &self.names[h.name.0 as usize..h.name.1 as usize]
    }

    /// The table slot of `name`: its entry, or the free slot it would
    /// take; and the entry's hash part.
    fn slot(&self, name: &str) -> (usize, u64) {
        let h = name_hash(name);
        let tag = h & !(NONE as u64);
        let mask = self.table.len() - 1;
        let mut i = (h >> 32 ^ h) as usize & mask;
        loop {
            let e = self.table[i];
            if e == FREE || e & !(NONE as u64) == tag && self.name_of(&self.heads[e as u32 as usize]) == name {
                return (i, tag);
            }
            i = (i + 1) & mask;
        }
    }

    fn first(&self, name: &str) -> Option<&Head> {
        if self.table.is_empty() {
            return None;
        }
        match self.table[self.slot(name).0] {
            FREE => None,
            e => Some(&self.heads[e as u32 as usize]),
        }
    }

    /// The functions named `name` (made now if not yet).
    pub fn get(&self, name: &str) -> Option<&[Rc<Function>]> {
        let h = self.first(name)?;
        if h.next == NONE {
            return Some(std::slice::from_ref(h.one.get_or_init(|| self.make(h))));
        }
        Some(self.groups[h.group as usize].get_or_init(|| {
            let mut v = Vec::new();
            let mut e = h;
            loop {
                v.push(e.one.get_or_init(|| self.make(e)).clone());
                if e.next == NONE {
                    break v.into_boxed_slice();
                }
                e = &self.heads[e.next as usize];
            }
        }))
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.first(name).is_some()
    }

    /// Whether the (first) function named `name` is a `#FUNCTION` and
    /// whether it returns a string, without making it (pops-tw asks
    /// EXISTFUNCTION about thousands of functions it never calls).
    pub fn kind(&self, name: &str) -> Option<(bool, bool)> {
        self.first(name).map(|h| (h.attrs.is_function, h.attrs.returns_str))
    }

    /// How many functions there are.
    pub fn len(&self) -> usize {
        self.heads.len()
    }

    /// The functions made so far (all of them when compiled at load).
    pub fn made(&self) -> impl Iterator<Item = &Rc<Function>> {
        self.heads.iter().filter_map(|h| h.one.get())
    }

    /// Each name and whether it is a single function that is neither
    /// `#SINGLE` nor `#ONLY` (what ENUMFUNC lists).
    pub fn names(&self) -> impl Iterator<Item = (&str, bool)> {
        self.table.iter().filter(|&&e| e != FREE).map(|&e| {
            let h = &self.heads[e as u32 as usize];
            (self.name_of(h), h.next == NONE && !h.attrs.single && !h.attrs.only)
        })
    }

    /// Add `f` after the functions of its name: made already (`src`
    /// None), or to be made from its file when first looked up.
    fn add(&mut self, f: Function, src: Option<Src>) {
        if self.table.len() * 3 <= self.n_names * 4 {
            self.grow();
        }
        let at = self.heads.len() as u32;
        let name = (self.names.len() as u32, (self.names.len() + f.name.len()) as u32);
        self.names.push_str(&f.name);
        let one = std::sync::OnceLock::new();
        let attrs = Attrs::of(&f);
        if f.local_size != 0 || f.locals_size != 0 {
            self.sizes.insert(at, (f.local_size, f.locals_size));
        }
        let src = match src {
            Some(s) => s,
            None => {
                let s = Src { file: f.file, line: f.line, start: 0, end: 0, open_ifs: 0, ord: 0 };
                let _ = one.set(Rc::new(f));
                s
            }
        };
        self.heads.push(Head { name, next: NONE, group: NONE, src, attrs, one });
        self.link(at);
    }

    /// Put head `at` (the last) in the table, or after the others of its
    /// name.
    fn link(&mut self, at: u32) {
        let h = &self.heads[at as usize];
        let (i, tag) = self.slot(&self.names[h.name.0 as usize..h.name.1 as usize]);
        match self.table[i] {
            FREE => {
                self.table[i] = tag | at as u64;
                self.n_names += 1;
            }
            first => {
                let first = first as u32;
                if self.heads[first as usize].group == NONE {
                    self.heads[first as usize].group = self.groups.len() as u32;
                    self.groups.push(Default::default());
                }
                // the last of the name's chain
                let mut e = first;
                while self.heads[e as usize].next != NONE {
                    e = self.heads[e as usize].next;
                }
                self.heads[e as usize].next = at;
            }
        }
    }

    fn grow(&mut self) {
        let size = (self.table.len() * 2).max(64);
        let old = std::mem::replace(&mut self.table, vec![FREE; size]);
        for e in old.into_iter().filter(|&e| e != FREE) {
            let i = self.slot(self.name_of(&self.heads[e as u32 as usize])).0;
            self.table[i] = e;
        }
    }

    /// Append the functions of `other` (a fork's), in order.
    fn absorb(&mut self, other: Funcs) {
        if self.ctx.is_none() {
            self.ctx = other.ctx.clone();
        }
        let Funcs { names, heads, sizes, .. } = other;
        self.names.reserve(names.len());
        self.heads.reserve(heads.len());
        let base = self.heads.len() as u32;
        self.sizes.extend(sizes.into_iter().map(|(k, v)| (k + base, v)));
        for h in heads {
            let at = self.heads.len() as u32;
            let name = (self.names.len() as u32, (self.names.len() + (h.name.1 - h.name.0) as usize) as u32);
            self.names.push_str(&names[h.name.0 as usize..h.name.1 as usize]);
            self.heads.push(Head { name, next: NONE, group: NONE, ..h });
            if self.table.len() * 3 <= self.n_names * 4 {
                self.grow();
            }
            self.link(at);
        }
    }

    /// Drop spare capacity once everything is loaded.
    pub fn shrink(&mut self) {
        self.names.shrink_to_fit();
        self.heads.shrink_to_fit();
        self.groups.shrink_to_fit();
    }

    /// Make the function of `h` from its file.
    fn make(&self, h: &Head) -> Rc<Function> {
        let ctx = self.ctx.clone().expect("functions to make need their files");
        let s = h.src;
        let src = LazySrc { ctx, file: s.file, start: s.start, end: s.end, open_ifs: s.open_ifs, ord: s.ord };
        let mut f = src.header(self.name_of(h), s.line);
        let a = h.attrs;
        (f.is_function, f.returns_str, f.order, f.single, f.only) = (a.is_function, a.returns_str, a.order, a.single, a.only);
        let at = (h as *const Head as usize - self.heads.as_ptr() as usize) / size_of::<Head>();
        (f.local_size, f.locals_size) = self.sizes.get(&(at as u32)).copied().unwrap_or((0, 0));
        f.lazy = Some(src);
        Rc::new(f)
    }

    /// A map by name, to change the functions in place (all made at load).
    pub fn into_map(self) -> HashMap<String, Vec<Rc<Function>>> {
        let mut m: HashMap<String, Vec<Rc<Function>>> = HashMap::default();
        for h in &self.heads {
            if let Some(f) = h.one.get() {
                m.entry(self.name_of(h).to_string()).or_default().push(f.clone());
            }
        }
        m
    }
}

/// System string variables (all others are integer). Whether a variable is a
/// string decides how `X = …` parses: string `=` takes FORM text.
pub fn is_system_str_var(n: &str) -> bool {
    if let Some((base, _)) = n.split_once('@') {
        return matches!(base, "LOCALS" | "ARGS");
    }
    matches!(
        n,
        "STR"
            | "SAVESTR"
            | "RESULTS"
            | "TSTR"
            | "LOCALS"
            | "ARGS"
            | "GLOBALS"
            | "NAME"
            | "CALLNAME"
            | "NICKNAME"
            | "MASTERNAME"
            | "CSTR"
            | "DRAWLINESTR"
            | "WINDOW_TITLE"
            | "MONEYLABEL"
            | "GAMEBASE_AUTHOR"
            | "GAMEBASE_INFO"
            | "GAMEBASE_YEAR"
            | "GAMEBASE_TITLE"
            | "LASTLOAD_TEXT"
            | "SAVEDATA_TEXT"
            | "ABLNAME"
            | "TALENTNAME"
            | "EXPNAME"
            | "MARKNAME"
            | "PALAMNAME"
            | "TRAINNAME"
            | "BASENAME"
            | "SOURCENAME"
            | "EXNAME"
            | "ITEMNAME"
            | "EQUIPNAME"
            | "TEQUIPNAME"
            | "FLAGNAME"
            | "TFLAGNAME"
            | "CFLAGNAME"
            | "TCVARNAME"
            | "CSTRNAME"
            | "STAINNAME"
            | "STRNAME"
            | "TSTRNAME"
            | "SAVESTRNAME"
            | "GLOBALNAME"
            | "GLOBALSNAME"
            | "DAYNAME"
            | "TIMENAME"
            | "MONEYNAME"
            | "CDFLAGNAME1"
            | "CDFLAGNAME2"
            | "TSTRNAME_"
            | "STR_"
    )
}

/// An open block while compiling, with the instruction indices still
/// waiting for a jump target.
enum Ctl {
    /// `test`: the pending JumpIfNot of the current IF/ELSEIF branch;
    /// `ends`: jumps from the end of each branch to ENDIF.
    If {
        test: Option<usize>,
        ends: Vec<usize>,
    },
    Select {
        slot: usize,
        test: Option<usize>,
        ends: Vec<usize>,
        first: bool,
    },
    For {
        begin: usize,
        slot: usize,
        var: Rc<VarRef>,
        breaks: Vec<usize>,
        conts: Vec<usize>,
    },
    While {
        top: usize,
        test: usize,
        breaks: Vec<usize>,
        conts: Vec<usize>,
    },
    Do {
        top: usize,
        breaks: Vec<usize>,
        conts: Vec<usize>,
    },
    /// TRYC*… CATCH … ENDCATCH: `call` gets its catch target patched.
    TryC {
        call: usize,
        ends: Vec<usize>,
    },
    /// PRINTDATA (`flags`) / STRDATA (`var`): collects DATA lines until
    /// ENDDATA; DATALIST…ENDLIST groups several lines into one entry.
    Data {
        var: Option<Rc<VarRef>>,
        flags: Option<PrintFlags>,
        entries: Vec<Vec<Form>>,
        list: Option<Vec<Form>>,
    },
    CallList {
        kind: CallKind,
        goto: bool,
        entries: Vec<(CallTarget, Vec<Expr>)>,
    },
}

/// The function being compiled.
struct FuncBuilder {
    /// the header (name, parameters, `#FUNCTION`…)
    f: Function,
    body: Body,
    /// Open blocks, innermost last.
    ctl: Vec<Ctl>,
    /// SIF jumps waiting for "the end of the next statement".
    sif: Vec<usize>,
    /// Private `#DIMS` names (string-typed for assignment parsing).
    str_locals: HashSet<String>,
    /// Source position recorded for each emitted instruction.
    cur_line: (u32, u32),
    /// A parse error in a block opener's expression: reported, but the
    /// block is still opened so the rest of the function nests as in Emuera.
    soft_error: Option<String>,
}

impl FuncBuilder {
    /// Append an instruction; returns its index (for later patching).
    fn emit(&mut self, i: Instr) -> usize {
        if let Some(t) = crate::translate::instr_byte(&i) {
            self.body.tr_line.push(t);
        }
        self.body.code.push(i);
        self.body.lines.push(self.cur_line);
        self.body.code.len() - 1
    }
    fn pc(&self) -> usize {
        self.body.code.len()
    }
    /// Fill in the jump / exit / catch target of instruction `at`.
    fn patch(&mut self, at: usize, target: usize) {
        match &mut self.body.code[at] {
            Instr::Jump(t) => *t = target,
            Instr::JumpIfNot(_, t) => *t = target,
            Instr::ForBegin(f) => f.exit = target,
            Instr::ForBreak { exit, .. } => *exit = target,
            Instr::CaseTest(_, _, t) => *t = target,
            Instr::Call(c) => c.catch = Some(target),
            Instr::Goto(g) => g.2 = Some(target),
            _ => {}
        }
    }
    /// Reserve a per-call scratch slot (FOR bounds, SELECTCASE value).
    fn slot(&mut self) -> usize {
        self.body.n_slots += 1;
        self.body.n_slots - 1
    }
}

pub struct Parser {
    pub prog: Program,
    /// `_Rename.csv` pairs by name, for `[[name]]` substitution (the first
    /// entry wins, as with replacing in order)
    rename_map: Rc<HashMap<String, String>>,
    /// Global `#DIMS` names.
    global_str: HashSet<String>,
    /// `--debug`: keep `;#;` lines and `[IF_DEBUG]` blocks.
    debug: bool,
    /// the game's Emuera settings (the System and Compatibility tabs
    /// change how scripts are read)
    pub cfg: std::sync::Arc<crate::emuconfig::Values>,
    /// Set: function bodies are compiled on their first call (see
    /// [`LazySrc`]), not at load.
    lazy: Option<Rc<LazyCtx>>,
}

/// Set when lazily compiled bodies had errors the VM hasn't reported.
pub static LAZY_ERRORS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where a function's statements are, for compiling them on its first
/// call: a byte range of its file, from its `@` line to the next
/// function's, read again then (the source isn't kept in memory). Big
/// games run a few percent of their functions in a session, and the
/// compiled form of a function takes several times its source.
pub struct LazySrc {
    ctx: Rc<LazyCtx>,
    file: u32,
    start: u32,
    end: u32,
    /// `[IF]` blocks open at the `@` line
    open_ifs: u16,
    /// which function of its name in the file it is (0: the first), to
    /// find it again in a file changed since loading
    ord: u16,
}

impl std::fmt::Debug for LazySrc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LazySrc(file {}, bytes {}..{})", self.file, self.start, self.end)
    }
}

/// What compiling a function later needs: a parser with the headers'
/// `#DEFINE`s and string variables, and the files as they were loaded.
pub struct LazyCtx {
    parser: std::sync::Mutex<Parser>,
    files: Vec<std::sync::OnceLock<LazyFile>>,
    /// errors in bodies compiled since the VM last took them
    errors: std::sync::Mutex<Vec<String>>,
    /// files changed since loading, compiled whole: (file, its functions)
    changed: std::sync::Mutex<Vec<(u32, HashMap<String, Vec<Rc<Function>>>)>>,
}

struct LazyFile {
    path: std::path::PathBuf,
    /// the name in messages
    rel: String,
    sjis: bool,
    len: u64,
    mtime: Option<std::time::SystemTime>,
}

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A body that stops with `msg` when run.
fn failed_body(f: &Function, msg: String) -> Body {
    Body { code: vec![Instr::BadLine(msg.into(), true)], lines: vec![(f.file, f.line)], ..Body::default() }
}

/// Declare EE's VARI / VARS variable `decl` (`sizes`: its dimensions, none
/// for a single value) in the function being built, once.
fn declare_scoped(b: &mut FuncBuilder, decl: &str, sizes: &[i64], is_str: bool) {
    let name = decl.to_uppercase();
    if b.body.dims.iter().any(|d| d.name == name) {
        return;
    }
    if is_str {
        b.str_locals.insert(name.clone());
    }
    b.body.dims.push(DimDecl {
        name,
        decl_name: decl.to_string(),
        is_str,
        dims: if sizes.is_empty() { vec![Expr::Int(1)] } else { sizes.iter().map(|&n| Expr::Int(n)).collect() },
        init: vec![],
        is_const: false,
        is_static: false,
        is_global: false,
        is_savedata: false,
        is_chara: false,
        is_ref: false,
        is_dynamic: true,
    });
}

/// Byte offsets of the starts of `lines` (1-based, ascending) in `raw`,
/// counted as `loader::emuera_lines` counts them (a lone `\r` ends a line).
fn line_offsets(raw: &[u8], lines: impl Iterator<Item = u32>, lone_cr: bool) -> Vec<u32> {
    let mut out = Vec::new();
    let (mut line, mut pos) = (1u32, 0usize);
    for want in lines {
        while line < want {
            // (one pass: the bytes may be Shift_JIS, so no str search; a
            // "\r" matters only in a file that has a lone one)
            let end = if lone_cr { raw[pos..].iter().position(|&c| c == b'\n' || c == b'\r') } else { raw[pos..].iter().position(|&c| c == b'\n') };
            match end {
                Some(i) => {
                    pos += i + if raw[pos + i] == b'\r' && raw.get(pos + i + 1) == Some(&b'\n') { 2 } else { 1 };
                    line += 1;
                }
                None => {
                    pos = raw.len();
                    break;
                }
            }
        }
        out.push(pos as u32);
    }
    out
}

fn read_range(path: &std::path::Path, start: u32, end: u32) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek};
    let mut f = std::fs::File::open(path)?;
    f.seek(std::io::SeekFrom::Start(start as u64))?;
    let mut buf = vec![0; end.saturating_sub(start) as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

impl LazySrc {
    /// The function `name` whose `@` line is line `line` of this text,
    /// without its body (see [`Funcs`]); its `#` attributes are the
    /// caller's.
    fn header(&self, name: &str, line: u32) -> Function {
        let ctx = &*self.ctx;
        let file = ctx.files.get(self.file as usize).and_then(|m| m.get());
        let made = file.and_then(|file| {
            let same = std::fs::metadata(&file.path).is_ok_and(|m| m.len() == file.len && m.modified().ok() == file.mtime);
            if !same {
                return None;
            }
            let text = crate::loader::decode_as(&read_range(&file.path, self.start, self.end).ok()?, file.sjis);
            let p = lock(&ctx.parser);
            let (lines, _) = p.logical_lines(&text, line, self.open_ifs as usize);
            match lines.first() {
                Some((no, head)) if *no == line && head.starts_with('@') => p.func_header(&head[1..], self.file, *no).ok().map(|b| b.f),
                _ => None,
            }
        });
        match made {
            Some(f) if &*f.name == name => f,
            // the file changed since loading: the parameters as it has
            // them now (the body is read from it too, see changed_body)
            _ => {
                let params = file
                    .and_then(|file| self.with_changed(file, |funcs| funcs.get(name).and_then(|v| v.get(self.ord as usize)).map(|g| g.params.clone())).ok());
                let mut f = lock(&ctx.parser).func_header(name, self.file, line).map(|b| b.f).expect("a bare name parses");
                f.params = params.flatten().unwrap_or_default();
                f
            }
        }
    }

    /// Run `with` on the functions of this file as it is now (read once,
    /// when a function of it is first needed after it changed).
    fn with_changed<T>(&self, file: &LazyFile, with: impl FnOnce(&mut HashMap<String, Vec<Rc<Function>>>) -> T) -> Result<T, String> {
        let ctx = &*self.ctx;
        let mut changed = lock(&ctx.changed);
        if !changed.iter().any(|(i, _)| *i == self.file) {
            let raw = std::fs::read(&file.path).map_err(|e| format!("{}: {}", file.rel, e))?;
            let mut q = lock(&ctx.parser).fork();
            q.parse_erb_text(self.file, &file.rel, &crate::loader::decode(&raw), None);
            let mut errors = lock(&ctx.errors);
            errors.push(format!(
                "{} changed since the game was loaded: its functions are read as they are now when they first run (F5 reloads all the scripts)",
                file.rel
            ));
            errors.append(&mut q.prog.errors);
            LAZY_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
            changed.push((self.file, std::mem::take(&mut q.prog.funcs).into_map()));
        }
        let funcs = &mut changed.iter_mut().find(|(i, _)| *i == self.file).expect("just added").1;
        Ok(with(funcs))
    }

    /// Read and compile the body of `f` (whose header this is).
    pub fn compile(&self, f: &Function) -> Body {
        let ctx = &*self.ctx;
        let Some(file) = ctx.files.get(self.file as usize).and_then(|m| m.get()) else {
            return failed_body(f, format!("@{}: its file is unknown", f.name));
        };
        let same = std::fs::metadata(&file.path).is_ok_and(|m| m.len() == file.len && m.modified().ok() == file.mtime);
        if !same {
            return self.changed_body(f, file);
        }
        let text = match read_range(&file.path, self.start, self.end) {
            Ok(b) => crate::loader::decode_as(&b, file.sjis),
            Err(e) => return failed_body(f, format!("{}: {}", file.rel, e)),
        };
        let mut p = lock(&ctx.parser);
        let (lines, _) = p.logical_lines(&text, f.line, self.open_ifs as usize);
        let head = match lines.split_first() {
            Some(((no, head), rest)) if *no == f.line && head.starts_with('@') => p.func_header(&head[1..], self.file, *no).map(|b| (b, rest)),
            _ => Err(String::new()),
        };
        let Ok((mut b, rest)) = head else {
            drop(p);
            return self.changed_body(f, file);
        };
        let end = rest.iter().position(|(_, l)| l.starts_with('@')).unwrap_or(rest.len());
        // (the attributes were read at load; statements check #FUNCTION)
        b.f.is_function = f.is_function;
        p.statements(&mut b, &file.rel, &rest[..end]);
        let mut g = p.finish(b, &file.rel);
        let errors = std::mem::take(&mut p.prog.errors);
        drop(p);
        if !errors.is_empty() {
            lock(&ctx.errors).extend(errors);
            LAZY_ERRORS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        g.body.take().unwrap_or_default()
    }

    /// The body of `f` from its file as it is now: the file changed since
    /// the game was loaded.
    fn changed_body(&self, f: &Function, file: &LazyFile) -> Body {
        let b = self
            .with_changed(file, |funcs| funcs.get_mut(&*f.name).and_then(|v| v.get_mut(self.ord as usize)).and_then(Rc::get_mut).and_then(|g| g.body.take()));
        match b {
            Ok(Some(b)) => b,
            Ok(None) => failed_body(f, format!("@{} is no longer in {}, which changed since the game was loaded: reload the scripts (F5)", f.name, file.rel)),
            Err(e) => failed_body(f, e),
        }
    }

    /// Errors found in function bodies compiled since the last call.
    pub fn take_errors(&self) -> Vec<String> {
        LAZY_ERRORS.store(false, std::sync::atomic::Ordering::Relaxed);
        std::mem::take(&mut *lock(&self.ctx.errors))
    }
}

/// Cut a trailing `; comment`, ignoring `;` inside quotes (quote nesting
/// pairs up, so a plain toggle is enough).
fn strip_comment(s: &str) -> &str {
    let mut in_str = false;
    let mut prev = '\0';
    // the end of a FORM string (`@"…"`) we are in: its `%…%`, `{…}` and
    // `\@…\@` parts may hold strings of their own, which don't end it
    // (`@"%TOSTR(X, " 000;-000")%"`)
    let mut form_end = 0;
    for (i, c) in s.char_indices() {
        if i < form_end {
            prev = c;
            continue;
        }
        if c == '"' && prev == '@' && !in_str {
            if let Some(n) = crate::expr::form_string_len(&s[i..]) {
                form_end = i + n;
                prev = c;
                continue;
            }
        }
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if c == ';' && !in_str {
            return &s[..i];
        }
        prev = c;
    }
    s
}

/// Split "CMD rest" -> (CMD uppercase, rest after exactly one separator char).
fn split_cmd(line: &str) -> (String, &str) {
    let end =
        line.char_indices().find(|(_, c)| c.is_whitespace() || *c == '\u{3000}' || (!crate::expr::is_ident_char(*c))).map(|(i, _)| i).unwrap_or(line.len());
    let cmd = line[..end].to_uppercase();
    let mut rest = &line[end..];
    if let Some(c) = rest.chars().next() {
        if c == ' ' || c == '\t' || c == '\u{3000}' {
            rest = &rest[c.len_utf8()..];
        }
    }
    (cmd, rest)
}

/// Decode a PRINT-family name: `PRINT[SINGLE|PLAIN][V|S|FORM|FORMS][K|D][L|W|C|LC|N]`.
/// Returns the content kind (R raw, V, S, F form, G forms) and the flags,
/// or None if `cmd` isn't a PRINT variant.
pub fn parse_print_name(cmd: &str) -> Option<(char, PrintFlags)> {
    let mut s = cmd.strip_prefix("PRINT")?;
    let mut fl = PrintFlags::default();
    if let Some(r) = s.strip_prefix("SINGLE") {
        fl.single = true;
        s = r;
    } else if let Some(r) = s.strip_prefix("PLAIN") {
        fl.plain = true;
        s = r;
    }
    // mid: R=raw, V, S, F=FORM, G=FORMS
    let mut mid = 'R';
    for (p, m) in [("FORMS", 'G'), ("FORM", 'F'), ("V", 'V'), ("S", 'S')] {
        if let Some(r) = s.strip_prefix(p) {
            // Avoid PRINTS... eating SINGLE etc. (already handled)
            mid = m;
            s = r;
            break;
        }
    }
    if let Some(r) = s.strip_prefix('K') {
        s = r;
    } else if let Some(r) = s.strip_prefix('D') {
        fl.default_color = true;
        s = r;
    }
    match s {
        "" => {}
        "L" => fl.newline = true,
        // N: shown as W (Emuera ends the display line before any wait),
        // only not counted as a logical line end for LINECOUNT there
        "N" => {
            fl.newline = true;
            fl.wait = true;
        }
        "W" => {
            fl.newline = true;
            fl.wait = true;
        }
        "C" => fl.column = Some(Align::Right),
        "LC" => fl.column = Some(Align::Left),
        // the column forms take K/D after the C: PRINTCD, PRINTFORMLCK…
        "CK" => fl.column = Some(Align::Right),
        "LCK" => fl.column = Some(Align::Left),
        "CD" | "LCD" => {
            fl.column = Some(if s == "CD" { Align::Right } else { Align::Left });
            fl.default_color = true;
        }
        _ => return None,
    }
    Some((mid, fl))
}

/// `PRINTDATA[K|D][L|W]` → flags, or None for other commands.
fn print_data_flags(cmd: &str) -> Option<PrintFlags> {
    let s = cmd.strip_prefix("PRINTDATA")?;
    let mut fl = PrintFlags::default();
    let s = s.strip_prefix('K').or_else(|| s.strip_prefix('D')).unwrap_or(s);
    match s {
        "" => {}
        "L" => fl.newline = true,
        "W" => {
            fl.newline = true;
            fl.wait = true
        }
        _ => return None,
    }
    Some(fl)
}

/// Lines whose text is shown as written, trailing spaces included: the
/// PRINT family and PRINTDATA's DATA / DATAFORM entries.
fn keeps_line_end(line: &str) -> bool {
    let end = line.find(|c: char| !crate::expr::is_ident_char(c)).unwrap_or(line.len());
    let word = &line[..end];
    word.get(..5).is_some_and(|w| w.eq_ignore_ascii_case("PRINT")) || word.eq_ignore_ascii_case("DATA") || word.eq_ignore_ascii_case("DATAFORM")
}

/// Commands whose argument is a raw string (no expression parsing).
fn raw_arg_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "DATA"
            // (the event's name as written: Emuera's FunctionArgType.STR)
            | "CALLEVENT"
            | "CUSTOMDRAWLINE"
            | "ALIGNMENT"
            | "STRLEN"
            | "STRLENU"
            | "TIMES"
            | "DEBUGPRINT"
            | "DEBUGPRINTL"
            | "SETCOLORBYNAME"
            | "SETBGCOLORBYNAME"
            | "BEGIN"
            | "SORTCHARA"
            | "PRINTBUTTONS"
            | "DOTRAIN_"
            | "BITMAP_CACHE_ENABLE"
    )
}

/// Commands whose argument is a FORM string.
fn form_arg_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "DATAFORM"
            | "THROW"
            | "DRAWLINEFORM"
            | "STRLENFORM"
            | "STRLENFORMU"
            | "PUTFORM"
            | "REUSELASTLINE"
            | "DEBUGPRINTFORM"
            | "DEBUGPRINTFORML"
            | "ENCODETOUNI"
            | "SAVENOS_"
    )
}

/// Block markers that only matter to Emuera's message skipping.
const IGNORED_BLOCKS: &[&str] = &["NOSKIP", "ENDNOSKIP"];

impl Parser {
    pub fn new(rename: &[(String, String)], debug: bool, cfg: std::sync::Arc<crate::emuconfig::Values>) -> Self {
        let mut m: HashMap<String, String> = HashMap::default();
        for (k, v) in rename {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Parser { prog: Program::default(), rename_map: Rc::new(m), global_str: HashSet::default(), debug, lazy: None, cfg }
    }

    /// From now on (after the headers), leave function bodies to their
    /// first call. `n_files`: how many files there are in all.
    pub fn compile_lazily(&mut self, n_files: usize) {
        let mut later = self.fork();
        later.lazy = None;
        self.lazy = Some(Rc::new(LazyCtx {
            parser: std::sync::Mutex::new(later),
            files: (0..n_files).map(|_| std::sync::OnceLock::new()).collect(),
            errors: Default::default(),
            changed: Default::default(),
        }));
    }

    /// `[[name]]` → its `_Rename.csv` replacement, in one pass over the line.
    fn apply_rename(&self, line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut rest = line;
        while let Some(i) = rest.find("[[") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 2..];
            match after.find("]]") {
                Some(j) => match self.rename_map.get(&after[..j]) {
                    Some(v) => {
                        out.push_str(v);
                        rest = &after[j + 2..];
                    }
                    None => {
                        out.push_str("[[");
                        rest = after;
                    }
                },
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Preprocess raw text into logical (line_no, text) pairs. `first_no`:
    /// the number of the text's first line; `open_ifs`: `[IF]` blocks open
    /// there (all on their taken branch: a function starts there). Also
    /// returns, per `@` line, how many `[IF]` blocks are open at it.
    fn logical_lines(&self, text: &str, first_no: u32, open_ifs: usize) -> (Vec<(u32, String)>, Vec<u16>) {
        let mut out = Vec::new();
        let mut depths = Vec::new();
        let mut skip_depth = 0;
        // per open [IF]: (this branch is active, a branch was taken)
        let mut cond_stack: Vec<(bool, bool)> = vec![(true, true); open_ifs];
        let mut join: Option<(u32, String)> = None;
        for (no, raw) in crate::loader::emuera_lines(text).enumerate() {
            let no = no as u32 + first_no;
            let trimmed = raw.trim();
            // most lines are plain code: skip blanks / comments before
            // allocating anything (the `{ }` join and `[...]` checks below
            // still see every other line)
            let comment = trimmed.starts_with(';') && !trimmed.starts_with(";!;") && !trimmed.starts_with(";^;") && !trimmed.starts_with(";#;");
            if (trimmed.is_empty() || comment) && join.is_none() {
                continue;
            }
            let mut line = trimmed.to_string();
            // Emuera never trims line ends: text a PRINT / DATA line shows
            // keeps its trailing spaces (`PRINTFORM … Return　　` pads a
            // menu with full-width ones). Other statements stay trimmed.
            if raw.len() != trimmed.len() && keeps_line_end(trimmed) {
                line = raw.trim_start().to_string();
            }
            // `;!;` and `;^;` (EMEE): comments elsewhere, code here
            if line.starts_with(";!;") || line.starts_with(";^;") {
                line = line[3..].trim().to_string();
            } else if line.starts_with(";#;") {
                if !self.debug {
                    continue;
                }
                line = line[3..].trim().to_string();
            }
            // [[rename]]
            if line.contains("[[") && !self.rename_map.is_empty() {
                line = self.apply_rename(&line);
            }
            // preprocessor lines all start with '['; only they need upper-casing
            let up = if line.starts_with('[') { line.to_uppercase() } else { String::new() };
            if up.starts_with("[SKIPSTART]") {
                skip_depth += 1;
                continue;
            }
            if up.starts_with("[SKIPEND]") {
                if skip_depth > 0 {
                    skip_depth -= 1;
                }
                continue;
            }
            if skip_depth > 0 {
                continue;
            }
            if up.starts_with("[IF_DEBUG]") {
                cond_stack.push((self.debug, self.debug));
                continue;
            }
            if up.starts_with("[IF_NDEBUG]") {
                cond_stack.push((!self.debug, !self.debug));
                continue;
            }
            // as in Emuera: after a taken branch, ELSEIF / ELSE are off
            if up.starts_with("[IF ") || up.starts_with("[ELSEIF ") {
                let elseif = up.starts_with("[ELSEIF");
                let name = line.trim_start_matches(|c| c != ' ').trim().trim_end_matches(']').trim();
                let v = self.prog.defines.contains_key(&name.to_uppercase());
                if elseif {
                    if let Some((on, done)) = cond_stack.last_mut() {
                        *on = !*done && v;
                        *done |= *on;
                    }
                } else {
                    cond_stack.push((v, v));
                }
                continue;
            }
            if up.starts_with("[ELSE]") {
                if let Some((on, done)) = cond_stack.last_mut() {
                    *on = !*done;
                    *done = true;
                }
                continue;
            }
            if up.starts_with("[ENDIF]") {
                cond_stack.pop();
                continue;
            }
            if cond_stack.iter().any(|c| !c.0) {
                continue;
            }
            if line == "{" {
                join = Some((no, String::new()));
                continue;
            }
            if line == "}" {
                if let Some((n, s)) = join.take() {
                    // a block whose first line is a comment is one comment
                    // line as a whole: games take code out that way
                    if s.starts_with(';') {
                        continue;
                    }
                    if s.starts_with('@') {
                        depths.push(cond_stack.len() as u16);
                    }
                    out.push((n, s));
                }
                continue;
            }
            if let Some((_, s)) = join.as_mut() {
                if !s.is_empty() {
                    // 行連結の改行コードの置換文字列, quotes taken out as
                    // Emuera's EraStreamReader does (default `" "`)
                    s.push_str(&self.cfg.value("行連結の改行コードの置換文字列").replace('"', ""));
                }
                s.push_str(&line);
                continue;
            }
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('@') {
                depths.push(cond_stack.len() as u16);
            }
            out.push((no, line));
        }
        (out, depths)
    }

    /// A parser for ERB files on another thread: same `#DEFINE`s and global
    /// string-variable names, empty output. Merge back with [`absorb`].
    ///
    /// [`absorb`]: Parser::absorb
    pub fn fork(&self) -> Parser {
        Parser {
            prog: Program { defines: self.prog.defines.clone(), ..Program::default() },
            rename_map: self.rename_map.clone(),
            global_str: self.global_str.clone(),
            debug: self.debug,
            lazy: self.lazy.clone(),
            cfg: self.cfg.clone(),
        }
    }

    /// Take the functions and errors a forked parser produced. Absorbing
    /// forks in file order keeps event handlers in file order.
    pub fn absorb(&mut self, other: Program) {
        self.prog.funcs.absorb(other.funcs);
        self.prog.errors.extend(other.errors);
    }

    /// Header file: `#DEFINE`s and global `#DIM`s only. All headers are
    /// parsed before any .ERB, so functions can use what they declare.
    pub fn parse_erh(&mut self, file_idx: u32, src: &SourceFile) {
        let (lines, _) = self.logical_lines(&src.text, 1, 0);
        // defines first (so dims can use them)
        for (no, line) in &lines {
            let up = line.to_uppercase();
            if up.starts_with("#DEFINE") {
                let rest = strip_comment(line[7..].trim()).trim();
                let (name, body) = match rest.find(|c: char| c.is_whitespace()) {
                    Some(i) => (&rest[..i], rest[i..].trim()),
                    None => (rest, ""),
                };
                self.prog.defines.insert(name.to_uppercase(), body.to_string());
            } else if !(up.starts_with("#DIM")) {
                self.prog.errors.push(format!("{}:{}: unexpected line in ERH: {}", src.path, no, line));
            }
        }
        for (no, line) in &lines {
            let up = line.to_uppercase();
            if up.starts_with("#DIM") {
                match self.parse_dim(strip_comment(line)) {
                    Ok(d) => {
                        if d.is_str {
                            self.global_str.insert(d.name.clone());
                        }
                        self.prog.global_dims.push(d);
                    }
                    Err(e) => self.prog.errors.push(format!("{}:{}: {}", src.path, no, e)),
                }
            }
        }
        let _ = file_idx;
    }

    /// `#DIM[S] [CONST|STATIC|GLOBAL|SAVEDATA|CHARADATA|REF|DYNAMIC…] NAME[, size…][ = init…]`.
    fn parse_dim(&self, line: &str) -> Result<DimDecl, String> {
        let up = line.to_uppercase();
        let is_str = up.starts_with("#DIMS");
        let rest = if is_str { &line[5..] } else { &line[4..] };
        crate::expr::check_full_space(rest)?;
        let mut p = ExprParser::new(rest, &self.prog.defines);
        let mut d = DimDecl {
            name: String::new(),
            decl_name: String::new(),
            is_str,
            dims: vec![],
            init: vec![],
            is_const: false,
            is_static: false,
            is_global: false,
            is_savedata: false,
            is_chara: false,
            is_ref: false,
            is_dynamic: false,
        };
        loop {
            let save = p.pos;
            let Some(id) = p.ident() else { return Err("missing #DIM name".into()) };
            match id.to_uppercase().as_str() {
                "CONST" => d.is_const = true,
                "STATIC" => d.is_static = true,
                "GLOBAL" => d.is_global = true,
                "SAVEDATA" => d.is_savedata = true,
                "CHARADATA" => d.is_chara = true,
                "REF" => d.is_ref = true,
                "DYNAMIC" => d.is_dynamic = true,
                _ => {
                    let _ = save;
                    d.name = id.to_uppercase();
                    d.decl_name = id.clone();
                    break;
                }
            }
        }
        // Emuera: a function's private #DIM keeps its value across calls
        // unless DYNAMIC (or REF); only consulted for function-private dims
        if !d.is_dynamic && !d.is_ref {
            d.is_static = true;
        }
        if p.eat(",") {
            loop {
                p.skip_ws();
                // `#DIM REF X,,` leaves the sizes to the referenced array
                if d.is_ref && matches!(p.peek(), None | Some(',')) {
                    d.dims.push(Expr::Int(0));
                } else {
                    d.dims.push(p.expr()?);
                }
                if !p.eat(",") {
                    break;
                }
            }
        }
        if p.eat("=") {
            d.init = p.expr_list()?;
        }
        if !p.at_end() {
            return Err(format!("bad #DIM: {}", line));
        }
        // identifiers are case-insensitive: `#DIMS X, PNMax` names PNMAX
        d.dims = d.dims.into_iter().map(upper_expr).collect();
        d.init = d.init.into_iter().map(upper_expr).collect();
        Ok(d)
    }

    /// Script file: a sequence of `@FUNCTION` blocks, compiled now.
    pub fn parse_erb(&mut self, file_idx: u32, src: &SourceFile) {
        self.parse_erb_text(file_idx, &src.path, &src.text, None);
    }

    /// A script file as read from disk (`raw`, at `path`; `rel`: its name
    /// in messages). With [`compile_lazily`] only the function headers are
    /// read now.
    ///
    /// [`compile_lazily`]: Parser::compile_lazily
    pub fn parse_erb_file(&mut self, file_idx: u32, rel: &str, path: &std::path::Path, raw: &[u8]) {
        let (text, sjis) = crate::loader::decode_detect(raw);
        if let Some(ctx) = &self.lazy {
            let meta = std::fs::metadata(path).ok();
            let file = LazyFile { path: path.to_path_buf(), rel: rel.to_string(), sjis, len: raw.len() as u64, mtime: meta.and_then(|m| m.modified().ok()) };
            if let Some(slot) = ctx.files.get(file_idx as usize) {
                let _ = slot.set(file);
            }
        }
        self.parse_erb_text(file_idx, rel, &text, Some(raw));
    }

    fn parse_erb_text(&mut self, file_idx: u32, path: &str, text: &str, raw: Option<&[u8]>) {
        let (lines, depths) = self.logical_lines(text, 1, 0);
        // with the file's bytes at hand, bodies can be left for later
        let lazy = match (&self.lazy, raw) {
            (Some(ctx), Some(raw)) if ctx.files.get(file_idx as usize).is_some_and(|f| f.get().is_some()) => Some((ctx.clone(), raw)),
            _ => None,
        };
        let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, (_, l))| l.starts_with('@')).map(|(i, _)| i).collect();
        for (no, _) in &lines[..starts.first().copied().unwrap_or(lines.len())] {
            self.prog.errors.push(format!("{}:{}: line outside of function", path, no));
        }
        let offsets = match &lazy {
            Some((_, raw)) => line_offsets(raw, starts.iter().map(|&i| lines[i].0), crate::loader::has_lone_cr(text)),
            None => Vec::new(),
        };
        let mut ords: HashMap<String, u16> = HashMap::default();
        for (k, &i0) in starts.iter().enumerate() {
            let i1 = starts.get(k + 1).copied().unwrap_or(lines.len());
            let (no, head) = &lines[i0];
            let mut b = match self.func_header(&head[1..], file_idx, *no) {
                Ok(b) => b,
                Err(e) => {
                    self.prog.errors.push(format!("{}:{}: {}", path, no, e));
                    for (no, _) in &lines[i0 + 1..i1] {
                        self.prog.errors.push(format!("{}:{}: line outside of function", path, no));
                    }
                    continue;
                }
            };
            self.attributes(&mut b, path, &lines[i0 + 1..i1]);
            match &lazy {
                Some((ctx, raw)) => {
                    let ord = ords.entry(b.f.name.to_string()).or_default();
                    let src = Src {
                        file: file_idx,
                        line: *no,
                        start: offsets[k],
                        end: offsets.get(k + 1).copied().unwrap_or(raw.len() as u32),
                        open_ifs: depths.get(k).copied().unwrap_or(0),
                        ord: *ord,
                    };
                    *ord += 1;
                    if self.prog.funcs.ctx.is_none() {
                        self.prog.funcs.ctx = Some(ctx.clone());
                    }
                    self.prog.funcs.add(b.f, Some(src));
                }
                None => {
                    self.statements(&mut b, path, &lines[i0 + 1..i1]);
                    let f = self.finish(b, path);
                    self.register(f);
                }
            }
        }
    }

    /// A function's `#` lines but its `#DIM`s: `#FUNCTION`, `#PRI`…, known
    /// at load (an event's order, whether it's a #FUNCTION).
    fn attributes(&mut self, b: &mut FuncBuilder, path: &str, lines: &[(u32, String)]) {
        for (no, line) in lines {
            if line.starts_with('#') && !line[1..].get(..3).is_some_and(|d| d.eq_ignore_ascii_case("DIM")) {
                if let Err(e) = self.directive(b, line) {
                    self.prog.errors.push(format!("{}:{}: {} :: {}", path, no, e, line));
                }
            }
        }
    }

    /// Compile a function's body: its `#DIM`s first (Emuera only takes
    /// `#` lines right after the `@` line), then the statements.
    fn statements(&mut self, b: &mut FuncBuilder, path: &str, lines: &[(u32, String)]) {
        let file_idx = b.f.file;
        for (no, line) in lines {
            if line.starts_with('#') && line[1..].get(..3).is_some_and(|d| d.eq_ignore_ascii_case("DIM")) {
                b.cur_line = (file_idx, *no);
                if let Err(e) = self.directive(b, line) {
                    self.prog.errors.push(format!("{}:{}: {} :: {}", path, no, e, line));
                }
            } else if line.get(..3).is_some_and(|c| c.eq_ignore_ascii_case("VAR")) {
                // EE's `VARI X` / `VARS S`: the function's on every line, also
                // above it (EE declares while reading the lines, and builds
                // statements after: eraMegatenP sets a VARS string above its VARS)
                let (cmd, rest) = split_cmd(line);
                let text = strip_comment(rest);
                let left = text.split('=').next().unwrap_or("");
                let mut parts = left.split(',').map(str::trim);
                let decl = parts.next().unwrap_or("");
                let sizes: Option<Vec<i64>> = parts.map(|x| x.parse().ok()).collect();
                if let (true, true, Some(sizes)) =
                    (matches!(cmd.as_str(), "VARI" | "VARS"), decl.starts_with(|c: char| c.is_alphanumeric() || c == '_' || !c.is_ascii()), sizes)
                {
                    declare_scoped(b, decl, &sizes, cmd == "VARS");
                }
            }
        }
        for (no, line) in lines {
            if line.starts_with('#') {
                continue;
            }
            b.cur_line = (file_idx, *no);
            let first = b.body.code.len();
            let r = self.statement(b, line);
            crate::translate::set_line_bytes(&mut b.body, first, line);
            if let Some(e) = b.soft_error.take() {
                self.prog.errors.push(format!("{}:{}: {} :: {}", path, no, e, line));
            }
            if let Err(e) = r {
                self.prog.errors.push(format!("{}:{}: {} :: {}", path, no, e, line));
                // keep the line's place so a pending SIF guards it, as in Emuera
                let fatal = !e.starts_with("unknown command");
                b.emit(Instr::BadLine(format!("{} :: {}", e, line).into(), fatal));
                Self::after_stmt(b);
            }
        }
    }

    /// `@NAME(ARG, ARGS:1 = "x")` or `@NAME, ARG, …`: start a function.
    fn func_header(&self, s: &str, file_idx: u32, line: u32) -> Result<FuncBuilder, String> {
        let s = strip_comment(s).trim();
        let end = s.find(|c: char| c == '(' || c == ',' || c.is_whitespace()).unwrap_or(s.len());
        let name = s[..end].to_uppercase();
        let mut rest = s[end..].trim();
        if rest.starts_with('(') {
            rest = rest[1..].trim_end();
            rest = rest.strip_suffix(')').unwrap_or(rest);
        } else if let Some(r) = rest.strip_prefix(',') {
            rest = r;
        }
        let mut params = Vec::new();
        if !rest.trim().is_empty() {
            let mut parts = split_top(rest, ',');
            // `@F(A, B,)`: a comma before the end adds nothing (Emuera's
            // ReduceArguments stops at the bracket or the line's end)
            if parts.len() > 1 && parts.last().is_some_and(|p| p.trim().is_empty()) {
                parts.pop();
            }
            for part in parts {
                crate::expr::check_full_space(&part)?;
                let mut p = ExprParser::new(&part, &self.prog.defines);
                let mut v = p.var_ref()?;
                upper_var(&mut v);
                let default = if p.eat("=") { Some(p.expr()?) } else { None };
                params.push(Param { target: v, default });
            }
        }
        let kojo =
            name.contains("KOJO") || self.prog.files.get(file_idx as usize).is_some_and(|p| p.contains("口上") || p.to_ascii_lowercase().contains("kojo"));
        Ok(FuncBuilder {
            soft_error: None,
            body: Body::default(),
            f: Function {
                name: name.into(),
                params: params.into(),
                body: Default::default(),
                lazy: None,
                is_function: false,
                returns_str: false,
                order: EventOrder::Normal,
                single: false,
                only: false,
                kojo,
                local_size: 0,
                locals_size: 0,
                file: file_idx,
                line,
            },
            ctl: vec![],
            sif: vec![],
            str_locals: HashSet::default(),
            cur_line: (file_idx, line),
        })
    }

    /// Close a function's body: report unclosed blocks.
    fn finish(&mut self, mut b: FuncBuilder, path: &str) -> Function {
        if !b.ctl.is_empty() {
            let kinds: Vec<&str> = b
                .ctl
                .iter()
                .map(|c| match c {
                    Ctl::If { .. } => "IF",
                    Ctl::Select { .. } => "SELECTCASE",
                    Ctl::For { .. } => "FOR/REPEAT",
                    Ctl::While { .. } => "WHILE",
                    Ctl::Do { .. } => "DO",
                    Ctl::TryC { .. } => "TRYC*",
                    Ctl::Data { .. } => "PRINTDATA/STRDATA",
                    Ctl::CallList { .. } => "TRY*LIST",
                })
                .collect();
            self.prog.errors.push(format!("{}: @{}: unclosed block(s): {}", path, b.f.name, kinds.join(", ")));
        }
        // falling off the end is RETURN 0, as in Emuera (see Instr::End):
        // games test RESULT after functions that only RETURN 1 on a match
        // (AnonTW's ITEMDATA item filters)
        b.emit(if b.f.is_function { Instr::ReturnF(None) } else { Instr::End });
        // the program is kept for the whole run: no spare capacity
        b.body.code.shrink_to_fit();
        b.body.lines.shrink_to_fit();
        b.body.dims.shrink_to_fit();
        b.body.labels.shrink_to_fit();
        b.f.body = BodyCell::new(Some(b.body));
        b.f
    }

    /// Add a function to the program. Several functions with one name
    /// form an event (all are kept).
    fn register(&mut self, f: Function) {
        self.prog.funcs.add(f, None);
    }

    fn is_str_var(&self, b: &FuncBuilder, name: &str) -> bool {
        // (a function's own `#DIM X` wins over a global `#DIMS X`)
        if b.body.dims.iter().any(|d| !d.is_str && d.name == name) {
            return false;
        }
        is_system_str_var(name) || self.global_str.contains(name) || b.str_locals.contains(name)
    }

    /// Called after each complete statement: any pending SIF jumps land
    /// here, i.e. just past the statement they guarded.
    fn after_stmt(b: &mut FuncBuilder) {
        let pc = b.pc();
        for s in std::mem::take(&mut b.sif) {
            b.patch(s, pc);
        }
    }

    /// Compile one logical line. Order matters: inside PRINTDATA/TRY*LIST
    /// only their sub-commands are allowed; then labels, `#` directives,
    /// PRINT variants, control flow, other commands, and finally
    /// assignment (anything that isn't a known command).
    fn statement(&mut self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        // a name followed by a full-width space, unless 全角スペースを
        // ホワイトスペースに含める: Emuera's error after a statement's name
        // (LogicalLineParser), its lexer's anywhere else (an assignment)
        if !crate::expr::FULL_SPACE_IS_SPACE.load(std::sync::atomic::Ordering::Relaxed) {
            let (cmd, _) = split_cmd(line);
            if !cmd.is_empty() && line.chars().nth(cmd.chars().count()) == Some('\u{3000}') {
                let statement = is_command(&cmd) || parse_print_name(&cmd).is_some() || FLOW_WORDS.contains(&cmd.as_str());
                return Err(if statement {
                    "命令で行が始まっていますが、命令の直後に半角スペース・タブ以外の文字が来ています(この警告はシステムオプション「全角スペースをホワイトスペースに含める」により無視できます)".into()
                } else {
                    crate::expr::FULL_SPACE_ERROR.into()
                });
            }
        }
        // Inside PRINTDATA / STRDATA collect DATA lines.
        if let Some(Ctl::Data { .. }) = b.ctl.last() {
            let (cmd, rest) = split_cmd(line);
            return match cmd.as_str() {
                "DATA" | "DATAFORM" => {
                    let form = if cmd == "DATA" { Form::lit(rest) } else { upper_form(parse_form(rest, &self.prog.defines)?) };
                    if let Some(Ctl::Data { entries, list, .. }) = b.ctl.last_mut() {
                        match list {
                            Some(l) => l.push(form),
                            None => entries.push(vec![form]),
                        }
                    }
                    Ok(())
                }
                "DATALIST" => {
                    if let Some(Ctl::Data { list, .. }) = b.ctl.last_mut() {
                        *list = Some(vec![]);
                    }
                    Ok(())
                }
                "ENDLIST" => {
                    if let Some(Ctl::Data { list, entries, .. }) = b.ctl.last_mut() {
                        if let Some(l) = list.take() {
                            entries.push(l);
                        }
                    }
                    Ok(())
                }
                "ENDDATA" => {
                    if let Some(Ctl::Data { var, flags, entries, .. }) = b.ctl.pop() {
                        match flags {
                            Some(fl) => b.emit(Instr::PrintData(entries, fl)),
                            None => b.emit(Instr::StrData(var, entries)),
                        };
                        Self::after_stmt(b);
                    }
                    Ok(())
                }
                _ => Err("only DATA/DATAFORM/DATALIST allowed inside PRINTDATA".into()),
            };
        }
        if let Some(Ctl::CallList { .. }) = b.ctl.last() {
            let (cmd, rest) = split_cmd(line);
            return if cmd == "FUNC" {
                // FORM text, as CALLFORM's (Emuera: SP_CALLFORM): eraSDMprotoNTR
                // builds `FUNC %"KOJO_" + … + "(" + … + ")"%`
                let (t, args) = self.call_target(strip_comment(rest), true)?;
                if let Some(Ctl::CallList { entries, .. }) = b.ctl.last_mut() {
                    entries.push((t, args));
                }
                Ok(())
            } else if cmd == "ENDFUNC" {
                if let Some(Ctl::CallList { kind, goto, entries }) = b.ctl.pop() {
                    if goto {
                        b.emit(Instr::GotoList(entries.into_iter().map(|(t, _)| t).collect()));
                    } else {
                        b.emit(Instr::CallList(entries, kind));
                    }
                    Self::after_stmt(b);
                }
                Ok(())
            } else {
                Err("only FUNC allowed inside TRYCALLLIST".into())
            };
        }

        if let Some(label) = line.strip_prefix('$') {
            let label = strip_comment(label).trim().to_uppercase();
            let pc = b.pc();
            b.body.labels.insert(label, pc);
            return Ok(());
        }
        if line.starts_with('#') {
            return self.directive(b, line);
        }
        // ++X / --X statement
        if line.starts_with("++") || line.starts_with("--") {
            let e = parse_expr(strip_comment(line).trim(), &self.prog.defines)?;
            b.emit(Instr::Eval(upper_expr(e)));
            Self::after_stmt(b);
            return Ok(());
        }

        let (cmd, rest) = split_cmd(line);
        // Emuera (ErbLoader.setArgument): a #FUNCTION(S) can't use input,
        // calls, BEGIN… The line is an error when it runs.
        // (`VARS = …` assigns a variable of the game's: no statement, see below)
        let assigns = matches!(cmd.as_str(), "VARI" | "VARS") && rest.trim_start().starts_with(|c: char| "=+-*/%&|^<>'".contains(c));
        if b.f.is_function && !assigns && crate::vm::builtins::NOT_IN_FUNCTION.binary_search(&cmd.as_str()).is_ok() {
            return Err(format!("{} instruction cannot be used in #FUNCTION(S)", cmd));
        }
        let defs = &self.prog.defines;
        let mut arg_c = strip_comment(rest).trim();
        // `REPEAT X,`: a comma at the end adds no argument, as Emuera's
        // ReduceArguments stops at the line's end (not for text arguments)
        if !raw_arg_cmd(&cmd) && !form_arg_cmd(&cmd) && !matches!(cmd.as_str(), "INPUTS" | "ONEINPUTS" | "TINPUTS" | "TONEINPUTS") {
            if let Some(a) = arg_c.strip_suffix(',') {
                arg_c = a.trim_end();
            }
        }

        // PRINT family
        if let Some(fl) = print_data_flags(&cmd) {
            b.ctl.push(Ctl::Data { var: None, flags: Some(fl), entries: vec![], list: None });
            return Ok(());
        }
        if cmd == "STRDATA" {
            let var = if arg_c.is_empty() { None } else { Some(self.var(arg_c)?) };
            b.ctl.push(Ctl::Data { var, flags: None, entries: vec![], list: None });
            return Ok(());
        }
        if !matches!(
            cmd.as_str(),
            "PRINTBUTTON" | "PRINTBUTTONC" | "PRINTBUTTONLC" | "PRINT_IMG" | "PRINT_RECT" | "PRINT_SPACE" | "PRINTCPERLINE" | "PRINTCLENGTH"
        ) {
            if let Some((mid, fl)) = parse_print_name(&cmd) {
                let content = match mid {
                    'R' => PrintContent::Raw(rest.to_string()),
                    'F' => PrintContent::Form(upper_form(parse_form(rest, defs)?)),
                    'G' => PrintContent::Exprs(vec![Expr::Call(Box::new(("__FORMS".into(), vec![upper_expr(parse_expr(arg_c, defs)?)], Default::default())))]),
                    'S' => PrintContent::Expr(upper_expr(parse_expr(arg_c, defs)?)),
                    'V' => PrintContent::Exprs(parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect()),
                    _ => unreachable!(),
                };
                b.emit(Instr::Print(content, fl));
                Self::after_stmt(b);
                return Ok(());
            }
        }

        match cmd.as_str() {
            "IF" => {
                let c = self.block_expr(b, arg_c, line);
                let t = b.emit(Instr::JumpIfNot(c, 0));
                b.ctl.push(Ctl::If { test: Some(t), ends: vec![] });
            }
            "ELSEIF" => {
                if !matches!(b.ctl.last(), Some(Ctl::If { .. })) {
                    return Err("ELSEIF without IF".into());
                }
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::If { test, ends }) = b.ctl.last_mut() else { unreachable!() };
                ends.push(j);
                let old = test.take();
                if let Some(t) = old {
                    b.patch(t, pc);
                }
                // (after the previous branch's jump: a bad condition fails
                // only when this ELSEIF is reached)
                let c = self.block_expr(b, arg_c, line);
                let t = b.emit(Instr::JumpIfNot(c, 0));
                if let Some(Ctl::If { test, .. }) = b.ctl.last_mut() {
                    *test = Some(t);
                }
            }
            "ELSE" => {
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::If { test, ends }) = b.ctl.last_mut() else { return Err("ELSE without IF".into()) };
                ends.push(j);
                if let Some(t) = test.take() {
                    b.patch(t, pc);
                }
            }
            "ENDIF" => {
                let Some(Ctl::If { test, ends }) = b.ctl.pop() else { return Err("ENDIF without IF".into()) };
                let pc = b.pc();
                if let Some(t) = test {
                    b.patch(t, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "SIF" => {
                let c = self.expr(arg_c)?;
                let t = b.emit(Instr::JumpIfNot(c, 0));
                // nested SIF: keep outer pending; patch both after next real statement
                b.sif.push(t);
                return Ok(());
            }
            "SELECTCASE" => {
                let e = self.block_expr(b, arg_c, line);
                let slot = b.slot();
                b.emit(Instr::SelectEval(e, slot, Default::default()));
                b.ctl.push(Ctl::Select { slot, test: None, ends: vec![], first: true });
            }
            "CASE" | "CASEELSE" => {
                let conds = if cmd == "CASE" { Some(self.case_conds(arg_c)?) } else { None };
                let (slot, first) = match b.ctl.last() {
                    Some(Ctl::Select { slot, first, .. }) => (*slot, *first),
                    _ => return Err("CASE without SELECTCASE".into()),
                };
                let j = if !first { Some(b.emit(Instr::Jump(0))) } else { None };
                let pc = b.pc();
                let prev = if let Some(Ctl::Select { test, ends, first, .. }) = b.ctl.last_mut() {
                    if let Some(j) = j {
                        ends.push(j);
                    }
                    *first = false;
                    test.take()
                } else {
                    None
                };
                if let Some(t) = prev {
                    b.patch(t, pc);
                }
                if let Some(conds) = conds {
                    let t = b.emit(Instr::CaseTest(slot, conds, 0));
                    if let Some(Ctl::Select { test, .. }) = b.ctl.last_mut() {
                        *test = Some(t);
                    }
                }
            }
            "ENDSELECT" => {
                let Some(Ctl::Select { test, ends, .. }) = b.ctl.pop() else { return Err("ENDSELECT without SELECTCASE".into()) };
                let pc = b.pc();
                if let Some(t) = test {
                    b.patch(t, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "FOR" | "REPEAT" => {
                let (var, start, end, step) = if cmd == "FOR" {
                    let parts = split_top(arg_c, ',');
                    if parts.len() < 3 {
                        return Err("FOR needs var, start, end".into());
                    }
                    let var = self.var(parts[0].trim())?;
                    let start = self.expr_or_zero(parts[1].trim())?;
                    let end = self.expr(parts[2].trim())?;
                    let step = if parts.len() > 3 && !parts[3].trim().is_empty() { Some(self.expr(parts[3].trim())?) } else { None };
                    (var, start, end, step)
                } else {
                    (Rc::new(VarRef::new("COUNT".into(), vec![])), Expr::Int(0), self.expr(arg_c)?, None)
                };
                let slot = b.slot();
                let begin = b.emit(Instr::ForBegin(Box::new(ForBegin { var: var.clone(), start, end, step, slot, exit: 0 })));
                b.ctl.push(Ctl::For { begin, slot, var, breaks: vec![], conts: vec![] });
            }
            "NEXT" | "REND" => {
                let Some(Ctl::For { begin, slot, var, breaks, conts }) = b.ctl.pop() else { return Err(format!("{} without FOR/REPEAT", cmd)) };
                let next_pc = b.pc();
                for c in conts {
                    b.patch(c, next_pc);
                }
                b.emit(Instr::ForNext { var: var.clone(), slot, body: begin + 1 });
                // Emit a jump over ForBreak for normal loop exit
                let jump_over_break = b.emit(Instr::Jump(0));
                // Emit ForBreak for BREAK statements: increments counter then jumps to exit
                let for_break_pc = b.pc();
                b.emit(Instr::ForBreak { var, slot, exit: 0 }); // exit will be patched below
                let exit = b.pc();
                b.patch(begin, exit);
                b.patch(for_break_pc, exit); // patch the exit field of ForBreak
                b.patch(jump_over_break, exit); // patch the jump over ForBreak
                for x in breaks {
                    b.patch(x, for_break_pc);
                }
                Self::after_stmt(b);
            }
            "WHILE" => {
                let c = self.block_expr(b, arg_c, line);
                let top = b.pc();
                let test = b.emit(Instr::JumpIfNot(c, 0));
                b.ctl.push(Ctl::While { top, test, breaks: vec![], conts: vec![] });
            }
            "WEND" => {
                let Some(Ctl::While { top, test, breaks, conts }) = b.ctl.pop() else { return Err("WEND without WHILE".into()) };
                b.emit(Instr::Jump(top));
                let exit = b.pc();
                b.patch(test, exit);
                for x in breaks {
                    b.patch(x, exit);
                }
                for c in conts {
                    b.patch(c, top);
                }
                Self::after_stmt(b);
            }
            "DO" => {
                let top = b.pc();
                b.ctl.push(Ctl::Do { top, breaks: vec![], conts: vec![] });
            }
            "LOOP" => {
                // DO … LOOP cond: repeat while cond is true
                let c = self.expr(arg_c)?;
                let Some(Ctl::Do { top, breaks, conts }) = b.ctl.pop() else { return Err("LOOP without DO".into()) };
                let lp = b.pc();
                b.emit(Instr::JumpIfNot(Expr::Unary(UnOp::Not, Box::new(c)), top));
                let exit = b.pc();
                for x in breaks {
                    b.patch(x, exit);
                }
                for c in conts {
                    b.patch(c, lp);
                }
                Self::after_stmt(b);
            }
            "BREAK" | "CONTINUE" => {
                let j = b.emit(Instr::Jump(0));
                let mut found = false;
                for c in b.ctl.iter_mut().rev() {
                    match c {
                        Ctl::For { breaks, conts, .. } | Ctl::While { breaks, conts, .. } | Ctl::Do { breaks, conts, .. } => {
                            if cmd == "BREAK" {
                                breaks.push(j)
                            } else {
                                conts.push(j)
                            }
                            found = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if !found {
                    return Err(format!("{} outside loop", cmd));
                }
                Self::after_stmt(b);
            }
            "CALL" | "JUMP" | "TRYCALL" | "TRYJUMP" | "CALLF" | "TRYCALLF" | "CALLFORM" | "JUMPFORM" | "TRYCALLFORM" | "TRYJUMPFORM" | "CALLFORMF"
            | "TRYCALLFORMF" | "TRYCCALL" | "TRYCJUMP" | "TRYCCALLFORM" | "TRYCJUMPFORM" => {
                let form = cmd.contains("FORM");
                let (target, args) = self.call_target(arg_c, form)?;
                let kind = if cmd.contains("JUMP") {
                    CallKind::Jump
                } else if cmd.ends_with("CALLF") || cmd.ends_with("FORMF") {
                    CallKind::CallF
                } else {
                    CallKind::Call
                };
                let try_ = cmd.starts_with("TRY");
                let c = b.emit(Instr::Call(Box::new(CallInstr { target, args, kind, try_, catch: None })));
                if matches!(cmd.as_str(), "TRYCCALL" | "TRYCJUMP" | "TRYCCALLFORM" | "TRYCJUMPFORM") {
                    b.ctl.push(Ctl::TryC { call: c, ends: vec![] });
                    return Ok(());
                }
                Self::after_stmt(b);
            }
            "TRYCGOTO" | "TRYCGOTOFORM" => {
                let target = self.goto_target(arg_c, cmd.contains("FORM"))?;
                let c = b.emit(Instr::Goto(Box::new((target, true, None))));
                b.ctl.push(Ctl::TryC { call: c, ends: vec![] });
            }
            "CATCH" => {
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::TryC { call, ends }) = b.ctl.last_mut() else { return Err("CATCH without TRYC*".into()) };
                ends.push(j);
                let call = *call;
                b.patch(call, pc);
            }
            "ENDCATCH" => {
                let Some(Ctl::TryC { call, ends }) = b.ctl.pop() else { return Err("ENDCATCH without TRYC*".into()) };
                let pc = b.pc();
                if matches!(&b.body.code[call], Instr::Call(c) if c.catch.is_none()) || matches!(&b.body.code[call], Instr::Goto(g) if g.2.is_none()) {
                    b.patch(call, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "TRYCALLLIST" | "TRYJUMPLIST" | "TRYGOTOLIST" => {
                let kind = if cmd == "TRYCALLLIST" { CallKind::Call } else { CallKind::Jump };
                b.ctl.push(Ctl::CallList { kind, goto: cmd == "TRYGOTOLIST", entries: vec![] });
            }
            "GOTO" | "TRYGOTO" | "GOTOFORM" | "TRYGOTOFORM" => {
                let target = self.goto_target(arg_c, cmd.contains("FORM"))?;
                b.emit(Instr::Goto(Box::new((target, cmd.starts_with("TRY"), None))));
                Self::after_stmt(b);
            }
            "RETURN" => {
                let v = if arg_c.is_empty() { vec![] } else { parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect() };
                b.emit(Instr::Return(v));
                Self::after_stmt(b);
            }
            "RETURNFORM" => {
                let mut v = Vec::new();
                for p in crate::expr::split_form_top(arg_c, ',') {
                    v.push(Expr::Form(Rc::new(upper_form(parse_form(p.trim(), defs)?))));
                }
                b.emit(Instr::Return(v));
                Self::after_stmt(b);
            }
            "RETURNF" => {
                let v = if arg_c.is_empty() { None } else { Some(self.expr(arg_c)?) };
                b.emit(Instr::ReturnF(v));
                Self::after_stmt(b);
            }
            "VARI" | "VARS" => {
                // Emuera-EE: `VARI X[, sizes] [= value]` / `VARS S[, sizes]
                // [= "text"]` declare a dynamic local and set it here
                let is_str = cmd == "VARS";
                let text = strip_comment(rest);
                let (left, right) = match text.find('=') {
                    Some(k) => (&text[..k], Some(text[k + 1..].trim())),
                    None => (text, None),
                };
                let mut parts = left.split(',').map(|x| x.trim());
                let decl = parts.next().unwrap_or("").to_string();
                if right.is_some() && !decl.starts_with(|c: char| c.is_alphanumeric() || c == '_' || !c.is_ascii()) {
                    // `VARS = …`, `VARS += …`: an assignment to the game's own VARS (eraAS
                    // has `#DIMS VARS`), as in Emuera and Anchor, which have no
                    // such command (see doc/differences.md)
                    self.assignment(b, line)?;
                    Self::after_stmt(b);
                    return Ok(());
                }
                if decl.is_empty() {
                    return Err(format!("{}: missing variable name", cmd));
                }
                let sizes: Vec<i64> = parts.map(|x| x.parse::<i64>().map_err(|_| format!("{}: bad size '{}'", cmd, x))).collect::<Result<_, _>>()?;
                let name = decl.to_uppercase();
                let is_array = !sizes.is_empty();
                declare_scoped(b, &decl, &sizes, is_str);
                let var = Rc::new(VarRef::new(name.into(), vec![]));
                if is_array {
                    // a fresh, empty array each time the line runs
                    b.emit(Instr::Command(Box::new(("VARSET".into(), Args::Exprs(vec![Expr::Var(var)])))));
                } else {
                    let value = match (is_str, right) {
                        (false, Some(r)) if !r.is_empty() => self.expr(r)?,
                        (false, _) => Expr::Int(0),
                        (true, Some(r)) => {
                            let lit = match (r.find('"'), r.rfind('"')) {
                                (Some(x), Some(y)) if y > x => &r[x + 1..y],
                                _ => "",
                            };
                            Expr::Str(lit.to_string())
                        }
                        (true, None) => Expr::Str(String::new()),
                    };
                    b.emit(Instr::Assign(var, AsgOp::SetExpr, vec![value]));
                }
                Self::after_stmt(b);
            }
            "BEGIN" | "FORCE_BEGIN" => {
                b.emit(Instr::Begin(arg_c.to_uppercase()));
                Self::after_stmt(b);
            }
            "RESTART" => {
                b.emit(Instr::Restart);
                Self::after_stmt(b);
            }
            "QUIT" => {
                b.emit(Instr::Quit);
                Self::after_stmt(b);
            }
            "THROW" => {
                b.emit(Instr::Throw(upper_form(parse_form(rest, defs)?)));
                Self::after_stmt(b);
            }
            c if IGNORED_BLOCKS.contains(&c) => {}
            // the default answer is FORM text up to the first comma
            // (`INPUTS %NAME%`, `INPUTS 決定`), then numbers (mouse, skip)
            "INPUTS" | "ONEINPUTS" if !arg_c.is_empty() => {
                let parts = crate::expr::split_form_top(arg_c, ',');
                let mut args = vec![Expr::Form(Rc::new(upper_form(parse_form(parts[0].trim_matches([' ', '\t']), defs)?)))];
                for p in &parts[1..] {
                    args.push(if p.trim().is_empty() { Expr::Empty } else { self.expr(p.trim())? });
                }
                b.emit(Instr::Command(Box::new((cmd, Args::Exprs(args)))));
                Self::after_stmt(b);
            }
            _ => {
                // (PuchiEmuera's names are commands only when not assigned to)
                let assigned =
                    crate::vm::builtins::PUCHI_COMMANDS.contains(&cmd.as_str()) && rest.trim_start().starts_with(|c: char| "=+-*/%&|^<>'".contains(c));
                if is_command(&cmd) && !assigned {
                    let args = if raw_arg_cmd(&cmd) {
                        Args::Raw(arg_c.to_string())
                    } else if form_arg_cmd(&cmd) {
                        Args::Form(upper_form(parse_form(rest, defs)?))
                    } else if arg_c.is_empty() {
                        Args::None
                    } else {
                        Args::Exprs(parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect())
                    };
                    b.emit(Instr::Command(Box::new((cmd, args))));
                } else {
                    self.assignment(b, line)?;
                }
                Self::after_stmt(b);
            }
        }
        Ok(())
    }

    /// `VAR op value`. Integer and `'=` assignments take an expression list
    /// (`A = 1, 2, 3` fills A:0..2); a string variable's plain `=` takes
    /// FORM text, as in Emuera (`STR = Hello %NAME%`).
    fn assignment(&self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        let mut p = ExprParser::new(line, &self.prog.defines);
        let mut v = p.var_ref()?;
        // the variable and the space before the operator are code (a string
        // assignment's FORM text after `=` may hold full-width spaces)
        if !crate::expr::FULL_SPACE_IS_SPACE.load(std::sync::atomic::Ordering::Relaxed) {
            let rest = p.rest();
            crate::expr::check_full_space(&line[..line.len().saturating_sub(rest.len())])?;
            if rest.trim_start_matches([' ', '\t']).starts_with('\u{3000}') {
                return Err(crate::expr::FULL_SPACE_ERROR.into());
            }
        }
        upper_var(&mut v);
        p.skip_ws();
        // `FLAG:X,-= 1`: Emuera reads the left side as an argument list,
        // where a comma at its end adds nothing (eraMegaten)
        if p.eat(",") {
            p.skip_ws();
        }
        let ops: &[(&str, AsgOp)] = &[
            ("'=", AsgOp::SetExpr),
            ("+=", AsgOp::Add),
            ("-=", AsgOp::Sub),
            ("*=", AsgOp::Mul),
            ("/=", AsgOp::Div),
            ("%=", AsgOp::Mod),
            ("&=", AsgOp::And),
            ("|=", AsgOp::Or),
            ("^=", AsgOp::Xor),
            ("<<=", AsgOp::Shl),
            (">>=", AsgOp::Shr),
        ];
        if p.eat("++") {
            b.emit(Instr::Eval(Expr::IncDec(v, false, 1)));
            return Ok(());
        }
        if p.eat("--") {
            b.emit(Instr::Eval(Expr::IncDec(v, false, -1)));
            return Ok(());
        }
        let mut op = None;
        for (t, o) in ops {
            if p.eat(t) {
                op = Some(*o);
                break;
            }
        }
        let op = match op {
            Some(o) => o,
            None => {
                // `X == 1` assigns: "used a lot, so it is the rule" in Emuera
                // (LogicalLineParser, with a warning)
                if p.eat("==") || p.eat("=") {
                    AsgOp::Set
                } else {
                    return Err(format!("unknown command or bad assignment: '{}'", line));
                }
            }
        };
        let rest = p.rest();
        let is_str = self.is_str_var(b, &v.name);
        // only plain `STR = ...` takes FORM text; `+=` takes a string expression
        if is_str && op == AsgOp::Set {
            // 文字列変数の代入に文字列式を強制する: only `'=` may assign to a
            // string variable (Emuera's ArgumentBuilder)
            if self.cfg.bool("文字列変数の代入に文字列式を強制する") {
                return Err("文字列代入は禁止されています（'=を用いるかコンフィグオプションを変えてください)".into());
            }
            // Emuera skips/trims only half-width spaces and tabs here; a
            // leading full-width space is content (map rows start with them).
            // The text runs to the end of the line: a `;` in it is text, not
            // a comment (ASCII-art rows, `X = ;%PLACE%`), as in Emuera
            let raw = rest.trim_matches([' ', '\t']);
            b.emit(Instr::AssignForm(v, op, upper_form(parse_form(raw, &self.prog.defines)?)));
            return Ok(());
        }
        let rhs_src = strip_comment(&rest).trim();
        let rhs: Vec<Expr> = if rhs_src.is_empty() {
            vec![if is_str { Expr::Str(String::new()) } else { Expr::Int(0) }]
        } else {
            parse_expr_list(rhs_src, &self.prog.defines)?.into_iter().map(upper_expr).collect()
        };
        b.emit(Instr::Assign(v, op, rhs));
        Ok(())
    }

    /// Function attributes: private `#DIM`s, `#FUNCTION(S)`, event order.
    fn directive(&mut self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        let line = strip_comment(line).trim();
        let up = line.to_uppercase();
        if up.starts_with("#DIM") {
            let d = self.parse_dim(line)?;
            if d.is_str {
                b.str_locals.insert(d.name.clone());
            }
            b.body.dims.push(d);
            return Ok(());
        }
        match up.split_whitespace().next().unwrap_or("") {
            "#FUNCTION" => b.f.is_function = true,
            "#FUNCTIONS" => {
                b.f.is_function = true;
                b.f.returns_str = true
            }
            "#PRI" => b.f.order = EventOrder::Pri,
            "#LATER" => b.f.order = EventOrder::Later,
            "#SINGLE" => b.f.single = true,
            "#ONLY" => b.f.only = true,
            d @ ("#LOCALSIZE" | "#LOCALSSIZE") => {
                // a number (a constant expression is left at the default)
                let n = up.split_whitespace().nth(1).and_then(|t| t.parse::<u32>().ok()).unwrap_or(0).min(1_000_000);
                if d == "#LOCALSIZE" {
                    b.f.local_size = n;
                } else {
                    b.f.locals_size = n;
                }
            }
            _ => return Err(format!("unknown directive {}", line)),
        }
        Ok(())
    }

    fn expr(&self, s: &str) -> Result<Expr, String> {
        Ok(upper_expr(parse_expr(s, &self.prog.defines)?))
    }

    /// The condition / value of a block opener (IF, WHILE, SELECTCASE…). A
    /// bad one is reported and replaced by a fatal [`Instr::BadLine`] (the
    /// error happens when the line runs, as in Emuera), so the block still
    /// opens and its ENDIF / WEND… still match.
    fn block_expr(&self, b: &mut FuncBuilder, s: &str, line: &str) -> Expr {
        match self.expr(s) {
            Ok(e) => e,
            Err(e) => {
                b.emit(Instr::BadLine(format!("{} :: {}", e, line).into(), true));
                b.soft_error = Some(e);
                Expr::Int(0)
            }
        }
    }

    fn expr_or_zero(&self, s: &str) -> Result<Expr, String> {
        if s.is_empty() {
            Ok(Expr::Int(0))
        } else {
            self.expr(s)
        }
    }

    fn var(&self, s: &str) -> Result<Rc<VarRef>, String> {
        crate::expr::check_full_space(s)?;
        let mut p = ExprParser::new(s, &self.prog.defines);
        let mut v = p.var_ref()?;
        if !p.at_end() {
            return Err(format!("bad variable '{}'", s));
        }
        upper_var(&mut v);
        Ok(v)
    }

    /// The conditions of one CASE line: `a`, `a TO b`, `IS op a`, comma-separated.
    fn case_conds(&self, s: &str) -> Result<Vec<CaseCond>, String> {
        let mut out = Vec::new();
        for part in split_top(s, ',') {
            let part = part.trim();
            if part.is_empty() {
                // trailing comma: `CASE 1, 2,`
                continue;
            }
            let up = part.to_uppercase();
            if let Some(r) = up.strip_prefix("IS ") {
                let r_orig = &part[part.len() - r.len()..];
                let r = r_orig.trim();
                let (op, e) = if let Some(x) = r.strip_prefix("<=") {
                    (BinOp::Le, x)
                } else if let Some(x) = r.strip_prefix(">=") {
                    (BinOp::Ge, x)
                } else if let Some(x) = r.strip_prefix("==") {
                    (BinOp::Eq, x)
                } else if let Some(x) = r.strip_prefix("!=") {
                    (BinOp::Ne, x)
                } else if let Some(x) = r.strip_prefix('<') {
                    (BinOp::Lt, x)
                } else if let Some(x) = r.strip_prefix('>') {
                    (BinOp::Gt, x)
                } else if let Some(x) = r.strip_prefix('&') {
                    (BinOp::BitAnd, x)
                } else {
                    return Err(format!("bad CASE IS: {}", part));
                };
                out.push(CaseCond::Is(op, self.expr(e.trim())?));
            } else if let Some(i) = find_word(&up, "TO") {
                out.push(CaseCond::Range(self.expr(part[..i].trim())?, self.expr(part[i + 2..].trim())?));
            } else {
                out.push(CaseCond::Eq(self.expr(part)?));
            }
        }
        Ok(out)
    }

    /// CALL target parsing: `NAME, a, b` / `NAME(a, b)` / FORM variants.
    fn call_target(&self, s: &str, form: bool) -> Result<(CallTarget, Vec<Expr>), String> {
        let s = s.trim();
        // find end of target: first top-level ',' or '('
        let mut end = s.len();
        let mut paren = false;
        let mut depth = 0;
        let mut in_pct = false;
        for (i, c) in s.char_indices() {
            match c {
                // `%…%` holes, but not the `%` (modulo) of a `{…}` one:
                // `IRAI_{ID % 1000}(ARG)` calls IRAI_4 with ARG
                '%' if form && depth == 0 => in_pct = !in_pct,
                '{' if !in_pct => depth += 1,
                '}' if !in_pct => depth -= 1,
                ',' if depth == 0 && !in_pct => {
                    end = i;
                    break;
                }
                '(' if depth == 0 && !in_pct => {
                    end = i;
                    paren = true;
                    break;
                }
                _ => {}
            }
        }
        let name = s[..end].trim();
        let target = if form { CallTarget::Form(upper_form(parse_form(name, &self.prog.defines)?)) } else { CallTarget::Static(name.to_uppercase()) };
        let mut argsrc = s[end..].trim();
        if paren {
            argsrc = argsrc[1..].trim_end();
            argsrc = argsrc.strip_suffix(')').ok_or("missing ')' in CALL")?;
        } else if let Some(r) = argsrc.strip_prefix(',') {
            argsrc = r;
        }
        let args = if argsrc.trim().is_empty() { vec![] } else { parse_expr_list(argsrc, &self.prog.defines)?.into_iter().map(upper_expr).collect() };
        Ok((target, args))
    }

    fn goto_target(&self, s: &str, form: bool) -> Result<CallTarget, String> {
        Ok(if form { CallTarget::Form(upper_form(parse_form(s.trim(), &self.prog.defines)?)) } else { CallTarget::Static(s.trim().to_uppercase()) })
    }
}

/// Position of `w` as a whitespace-delimited word outside quotes
/// (`CASE 1 TO 5`), so names containing TO aren't split.
fn find_word(up: &str, w: &str) -> Option<usize> {
    let mut in_str = false;
    for (i, c) in up.char_indices() {
        if c == '"' {
            in_str = !in_str;
        }
        if in_str || !up[i..].starts_with(w) {
            continue;
        }
        // (a full-width space is one too, as Emuera's SystemAllowFullSpace
        // has it by default: eTC's `CASE 1　TO 14`)
        let full = crate::expr::FULL_SPACE_IS_SPACE.load(std::sync::atomic::Ordering::Relaxed);
        let space = |c: Option<char>| c.is_none_or(|c| c == ' ' || c == '\t' || (full && c == '\u{3000}'));
        if space(up[..i].chars().next_back()) && space(up[i + w.len()..].chars().next()) {
            return Some(i);
        }
    }
    None
}

/// Normalize identifiers to uppercase (Emuera is case-insensitive by default).
/// Upper-case the names in a parsed expression; in a game with Anchor's
/// TR files, also mark its constants as Anchor folds them
/// (`translate::fold_expr`).
pub fn upper_expr(e: Expr) -> Expr {
    crate::translate::fold_expr(upper_expr_in(e))
}

fn upper_expr_in(e: Expr) -> Expr {
    match e {
        Expr::Var(v) => {
            let mut v = v;
            upper_var(&mut v);
            Expr::Var(v)
        }
        Expr::Call(c) => {
            let (n, a, kind) = *c;
            let mut a: Vec<Expr> = a.into_iter().map(upper_expr_in).collect();
            a.shrink_to_fit();
            Expr::Call(Box::new((n.to_uppercase(), a, kind)))
        }
        Expr::Unary(o, x) => Expr::Unary(o, Box::new(upper_expr_in(*x))),
        Expr::Binary(o, a, b) => Expr::Binary(o, Box::new(upper_expr_in(*a)), Box::new(upper_expr_in(*b))),
        Expr::Ternary(a, b, c) => Expr::Ternary(Box::new(upper_expr_in(*a)), Box::new(upper_expr_in(*b)), Box::new(upper_expr_in(*c))),
        Expr::IncDec(v, p, d) => {
            let mut v = v;
            upper_var(&mut v);
            Expr::IncDec(v, p, d)
        }
        Expr::Form(f) => Expr::Form(Rc::new(upper_form_in(Rc::try_unwrap(f).unwrap_or_else(|f| (*f).clone())))),
        e => e,
    }
}

/// One shared copy of each variable name per parse thread: the same few
/// thousand names occur millions of times in a big game.
fn intern(s: String) -> Name {
    thread_local! {
        static NAMES: std::cell::RefCell<HashSet<crate::ast::Name>> = std::cell::RefCell::new(HashSet::default());
    }
    NAMES.with(|n| {
        let mut n = n.borrow_mut();
        if let Some(x) = n.get(s.as_str()) {
            return x.clone();
        }
        let x: Name = s.into();
        n.insert(x.clone());
        x
    })
}

pub fn upper_var(v: &mut Rc<VarRef>) {
    let inner = Rc::make_mut(v);
    // CSV-name indices (`ABL:技巧`) are upper-cased too: name-table
    // lookups are case-insensitive, so nothing is lost.
    let mut name = inner.name.to_uppercase();
    // Emuera-Anchor renames PARAM to PALAM in every identifier; here only
    // where that names a built-in (PARAM, PARAMNAME…), so a game's own
    // `#DIM MYPARAM` keeps its name and saves stay Emuera's (the VM falls
    // back on the renamed name for a variable it can't find)
    if let Some(r) = crate::translate::param_renamed(&name) {
        if crate::vm::vars::system_var(&r).is_some() || (r.ends_with("NAME") && crate::csvdata::table_for_var(&r).is_some()) {
            name = r;
        }
    }
    inner.name = intern(name);
    let mut idx: Vec<Expr> = std::mem::take(&mut inner.idx).into_iter().map(upper_expr).collect();
    idx.shrink_to_fit();
    inner.idx = idx;
    // a bare `LOCAL` / `RESULT` …: one shared reference per name
    if inner.idx.is_empty() {
        thread_local! {
            static BARE: std::cell::RefCell<HashMap<crate::ast::Name, Rc<VarRef>>> = std::cell::RefCell::new(HashMap::default());
        }
        let name = inner.name.clone();
        *v = BARE.with(|b| b.borrow_mut().entry(name).or_insert_with(|| v.clone()).clone());
    }
}

/// [`upper_expr`] for a FORM string.
pub fn upper_form(f: Form) -> Form {
    crate::translate::fold_form(upper_form_in(f))
}

fn upper_form_in(f: Form) -> Form {
    Form {
        parts: f
            .parts
            .into_iter()
            .map(|p| match p {
                FormPart::Str(e, w, a) => FormPart::Str(upper_expr_in(e), w.map(|w| Box::new(upper_expr_in(*w))), a),
                FormPart::Int(e, w, a) => FormPart::Int(upper_expr_in(e), w.map(|w| Box::new(upper_expr_in(*w))), a),
                FormPart::Cond(c) => {
                    let (c, a, b) = *c;
                    FormPart::Cond(Box::new((upper_expr_in(c), upper_form_in(a), upper_form_in(b))))
                }
                l => l,
            })
            .collect(),
    }
}

/// Flow-control statements the parser handles itself (not in COMMANDS).
const FLOW_WORDS: &[&str] = &[
    "IF", "ELSEIF", "ELSE", "ENDIF", "SIF", "REPEAT", "REND", "FOR", "NEXT", "WHILE", "WEND", "DO", "LOOP", "SELECTCASE", "CASE", "CASEELSE", "ENDSELECT",
    "CALL", "CALLF", "CALLFORM", "CALLFORMF", "TRYCALL", "TRYCALLFORM", "TRYCCALL", "TRYCCALLFORM", "CATCH", "ENDCATCH", "JUMP", "JUMPFORM", "TRYJUMP",
    "TRYJUMPFORM", "TRYCJUMP", "TRYCJUMPFORM", "GOTO", "GOTOFORM", "TRYGOTO", "TRYGOTOFORM", "TRYCGOTO", "TRYCGOTOFORM", "RETURN", "RETURNF", "RETURNFORM",
    "BREAK", "CONTINUE", "TRYCALLLIST", "TRYJUMPLIST", "TRYGOTOLIST", "FUNC", "ENDFUNC", "PRINTDATA", "STRDATA", "DATA", "DATAFORM", "DATALIST", "ENDLIST",
    "ENDDATA", "NOSKIP", "ENDNOSKIP", "BEGIN", "RESTART", "QUIT",
];

/// Known statement-style commands (everything that is not an assignment).
pub fn is_command(c: &str) -> bool {
    crate::vm::builtins::is_builtin_command(c) || crate::vm::builtins::is_builtin_function(c)
}

#[cfg(test)]
mod tests {
    /// Functions are made from their files on first use, with the
    /// attributes read at load, the same object each time, and an event's
    /// handlers in file order.
    #[test]
    fn functions_made_on_first_use() {
        use crate::ast::EventOrder;
        let dir = std::env::temp_dir().join(format!("emuera-funcs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("A.ERB");
        let src = "@EVENTFIRST\n#PRI\nPRINTL a\n@F(X, S = \"q\")\n#FUNCTIONS\n#LOCALSIZE 5\nRETURNF \"x\"\n@EVENTFIRST\nPRINTL b\n@G\nPRINTL c\n";
        std::fs::write(&path, src).unwrap();
        let mut p = super::Parser::new(&[], false, std::sync::Arc::new(crate::emuconfig::Values::emuera()));
        p.prog.files.push("A.ERB".into());
        p.compile_lazily(1);
        p.parse_erb_file(0, "A.ERB", &path, src.as_bytes());
        let fs = &p.prog.funcs;
        assert_eq!((fs.len(), fs.made().count()), (4, 0));
        let f = fs.get("F").unwrap()[0].clone();
        assert!(f.is_function && f.returns_str && f.local_size == 5 && f.line == 4);
        assert_eq!(f.params.len(), 2);
        assert!(f.params[0].default.is_none() && f.params[1].default.is_some());
        assert!(std::sync::Arc::ptr_eq(&fs.get("F").unwrap()[0], &f));
        let ev = fs.get("EVENTFIRST").unwrap();
        assert_eq!((ev.len(), ev[0].order, ev[1].order, ev[1].line), (2, EventOrder::Pri, EventOrder::Normal, 8));
        assert!(fs.get("H").is_none() && fs.contains_key("G") && !fs.contains_key("H"));
        assert!(fs.get("G").unwrap()[0].body().code.len() > 1);
        let mut names: Vec<(&str, bool)> = fs.names().collect();
        names.sort();
        assert_eq!(names, [("EVENTFIRST", false), ("F", true), ("G", true)]);
        assert_eq!(fs.made().count(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn line_offsets_count_lone_cr() {
        // lines 1 "a", 2 "", 3 "b", 4 "c": the lazy compiler reads from these
        let raw = b"a\r\rb\r\nc";
        assert_eq!(super::line_offsets(raw, [1, 2, 3, 4].into_iter(), true), [0, 2, 3, 6]);
        assert_eq!(super::line_offsets(b"a\r\nb\nc", [1, 2, 3].into_iter(), false), [0, 3, 5]);
        let text = std::str::from_utf8(raw).unwrap();
        assert_eq!(crate::loader::emuera_lines(text).nth(3), Some("c"));
    }
}
