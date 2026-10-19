//! The interpreter: variable resolution, expression evaluation, the
//! instruction loop and function calls. System flow lives in `system.rs`,
//! built-in commands/functions in `builtins.rs`.
//!
//! ## Variables
//!
//! Every variable is an [`Arr`] (`vars.rs`), even scalars. Where it lives:
//!
//! | kind | storage |
//! |---|---|
//! | system globals (FLAG, RESULT…) and ERH `#DIM`s | `Vm::globals`, created on first use |
//! | character variables (CFLAG, TALENT…, `#DIM CHARADATA`) | `Vm::charas[i].vars` |
//! | a function's LOCAL/ARG and non-DYNAMIC private `#DIM`s | `Vm::statics[FUNC][NAME]` |
//! | `#DIM DYNAMIC` privates | the call's [`Frame::vars`] |
//! | `#DIM REF` parameters | an alias in [`Frame::refs`] to the caller's storage |
//!
//! [`Vm::locate`] turns a [`VarRef`] into a [`Place`] — which array
//! ([`Loc`]) and which flat element — and everything else reads and writes
//! through places. Indices that are bare names are looked up in the
//! variable's CSV name table (`ABL:技巧`).
//!
//! ## Control
//!
//! Errors and non-local exits (`BEGIN`, `QUIT`) travel as `Err(Ctrl)` so
//! `?` unwinds the Rust call stack back to the system loop. A user `CALL`
//! is a Rust call into [`Vm::call_func`], which runs [`Vm::exec`] on a new
//! [`Frame`]; deep ERB recursion therefore needs the big stack `main`
//! gives the VM thread.

pub mod builtins;
pub mod datatable;
pub mod erasav;
mod memstats;
pub mod save;
pub mod textsav;
pub mod system;
pub mod value;
pub mod vars;

use crate::ast::*;
use crate::console::Console;
use crate::csvdata::{table_for_var, GameCsv};
use crate::frontend::{Frontend, InputKind, InputReq, InputResult};
use crate::loader::sjis_len;
use crate::parser::Program;
use crate::resources::Resources;
use crate::fxhash::{HashMap, HashSet};
use std::path::PathBuf;
// Arc (aliased as Rc): parsed programs are built on several threads
use std::sync::Arc as Rc;
use value::*;
use vars::*;

/// Per-call scratch cell used by loops and SELECTCASE (see `Instr`).
#[derive(Clone, Debug)]
pub enum Slot {
    None,
    /// FOR/REPEAT: (end, step)
    For(i64, i64),
    /// SELECTCASE: the value being matched
    Sel(Value),
}

/// A variable's evaluated indices, without a heap allocation (Emuera
/// arrays have at most three dimensions, plus a character index).
#[derive(Default, Clone, Copy)]
pub struct Idx {
    n: u8,
    v: [i64; 4],
}

impl std::ops::Deref for Idx {
    type Target = [i64];
    fn deref(&self) -> &[i64] {
        &self.v[..self.n as usize]
    }
}

/// One active function call.
pub struct Frame {
    pub func: Rc<Function>,
    /// its statements (kept while it runs, whatever happens to memory)
    pub body: Rc<Body>,
    /// Index of the next instruction to run.
    pub pc: usize,
    /// `#DIM DYNAMIC` privates (fresh each call).
    pub vars: HashMap<String, Arr>,
    pub slots: Vec<Slot>,
    /// `#DIM REF` parameters: name -> the caller's variable
    pub refs: HashMap<String, Loc>,
}

/// One character added with ADDCHARA: its character-variable arrays,
/// created lazily on first access.
#[derive(Default, Clone)]
pub struct Chara {
    pub vars: HashMap<String, Arr>,
}

/// Which array a [`Place`] is in.
#[derive(Clone)]
pub enum Loc {
    /// a slot in `Vm::globals`
    Global(usize),
    /// `#DIM DYNAMIC` in the current frame
    Frame(Name),
    /// a variable in a specific (caller's) frame, for REF parameters
    FrameAt(usize, Name),
    /// function-owned storage: slot in `Vm::statics`
    Static(usize),
    /// (character index, variable)
    Chara(usize, Name),
    /// read-only computed value (CHARANUM, RAND:n, GAMEBASE_*…)
    Ro(Value),
}

/// A resolved variable element: array + row-major flat index.
pub struct Place {
    pub loc: Loc,
    pub flat: usize,
}

/// The whole interpreter state.
pub struct Vm {
    pub prog: Program,
    pub csv: GameCsv,
    pub con: Console,
    pub fe: Box<dyn Frontend>,
    pub res: Resources,
    pub globals: vars::Globals,
    /// function-owned storage; a slot keeps its number for good (emptied
    /// on RESETDATA, made again on next use), so references cache it
    pub statics: Vec<Option<Arr>>,
    /// (function name, variable) → slot in `statics`
    static_index: HashMap<(String, String), usize>,
    pub charas: Vec<Chara>,
    /// The call stack, innermost last.
    pub frames: Vec<Frame>,
    /// ERH `#DIM` declarations by name (sizes / flags for lookups).
    pub user_dims: HashMap<String, DimDecl>,
    /// inputs asked for so far, and when to run @__INJECT
    inputs_seen: u64,
    inject_at: Option<u64>,
    /// CALLTRAIN: the commands to run one after another, and how many ran
    pub ctrain: Option<(Vec<i64>, usize)>,
    /// DOTRAIN: may it be used now (inside @EVENTTRAIN, @SHOW_STATUS,
    /// @SHOW_USERCOM, @EVENTCOMEND or @USERCOM), and the command it chose,
    /// which runs once that event has ended
    pub dotrain_ok: bool,
    pub dotrain: Option<i64>,
    /// set before @EVENTCOMEND; a WAIT clears it, else one follows it
    pub comend_wait: bool,
    /// reads and parses the ERB files again (the reload hotkey)
    pub reloader: Option<Box<dyn FnMut() -> Program>>,
    /// Emuera-EM MAP_* string maps, by (case-sensitive) name
    pub maps: HashMap<String, StrMap>,
    /// Emuera-EM DT_* data tables, by (case-sensitive) name
    pub dts: HashMap<String, datatable::DataTable>,
    /// XML_DOCUMENT's documents, by key (see `xml_fn`)
    pub xml_docs: HashMap<String, crate::xml::Element>,
    /// extensions LOADTEXT/SAVETEXT may use with a path (config), plus txt
    pub text_exts: Vec<String>,
    /// save folder / compression, from the config
    pub save_opts: save::SaveOpts,
    /// VarExt*.csv: Emuera-EM data saved with the game
    pub varext: save::VarExt,
    /// REF bindings collected by `call_args` for the next `call_func`
    pending_refs: Vec<(String, Loc)>,
    /// `EMUERA_TRACE`: log calls whose name contains this.
    trace: Option<String>,
    /// `EMUERA_MEMSTATS`: the file the memory report goes to.
    memstats: Option<String>,
    /// user function calls so far
    calls: u64,
    /// roughly the memory compiled function bodies take (those that can
    /// be compiled again), and how much they may (`EMUERA_CODE_MB`)
    code_bytes: usize,
    pub code_budget: usize,
    rng: u64,
    /// Warnings already printed (each is shown once).
    warned: HashSet<String>,
    pub game_dir: PathBuf,
    /// `--strict`: unimplemented commands are errors, not warnings.
    pub strict: bool,
    /// FLOWINPUT / FLOWINPUTS settings for the shop's own input:
    /// (default, use it on empty input, string input, default string,
    /// skip the input and take the default)
    pub flow_input: (i64, bool, bool, String, bool),
    /// SKIPLOG: message skipping on (MESSKIP)
    pub mes_skip: bool,
    /// instructions run, for the periodic screen update (`tick`)
    ticks: u32,
    last_draw: std::time::Instant,
    /// ISTIMEOUT: did the last timed input expire?
    pub is_timeout: bool,
    pub last_input: String,
    /// Current system phase (TITLE, SHOP, TRAIN…), see `system.rs`.
    pub state: String,
    /// Warnings collected so far.
    pub log: Vec<String>,
    /// Keep warnings in `log` instead of printing them (full-screen UI).
    pub defer_warnings: bool,
}

/// Nested user calls allowed before reporting a stack overflow.
const MAX_DEPTH: usize = 4000;

/// A built-in variable's dimensions with VariableSize.csv applied.
pub(crate) fn sized_dims(varsize: &HashMap<String, Vec<usize>>, name: &str, def: &VarDef) -> Vec<usize> {
    match varsize.get(name) {
        Some(d) if !d.is_empty() && !def.dims.is_empty() => d.iter().take(def.dims.len().max(1)).copied().collect(),
        _ => def.dims.to_vec(),
    }
}

impl Vm {
    pub fn new(prog: Program, csv: GameCsv, res: Resources, fe: Box<dyn Frontend>, game_dir: PathBuf) -> Vm {
        if let Ok(p) = std::env::var("EMUERA_PROFILE") {
            prof::start(p);
        }
        // EMUERA_SEED=N makes runs repeatable (tests, bug reports)
        let seed = std::env::var("EMUERA_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x1234_5678)
        });
        let mut vm = Vm {
            prog,
            csv,
            con: Console::new(),
            fe,
            res,
            globals: Default::default(),
            statics: Vec::new(),
            static_index: HashMap::default(),
            charas: Vec::new(),
            frames: Vec::new(),
            user_dims: HashMap::default(),
            inputs_seen: 0,
            inject_at: std::env::var("EMUERA_INJECT_AT").ok().and_then(|n| n.parse().ok()),
            ctrain: None,
            dotrain_ok: false,
            dotrain: None,
            comend_wait: false,
            reloader: None,
            maps: HashMap::default(),
            dts: HashMap::default(),
            xml_docs: HashMap::default(),
            text_exts: vec!["txt".into()],
            save_opts: save::SaveOpts::default(),
            varext: save::VarExt::load(crate::loader::find_dir_ci(&game_dir, "CSV").as_deref()),
            pending_refs: Vec::new(),
            trace: std::env::var("EMUERA_TRACE").ok().map(|t| t.to_uppercase()),
            memstats: std::env::var("EMUERA_MEMSTATS").ok(),
            calls: 0,
            code_bytes: 0,
            code_budget: crate::sys::memory_budget("EMUERA_CODE_MB", 16),
            rng: seed | 1,
            warned: HashSet::default(),
            game_dir,
            strict: false,
            flow_input: (0, false, false, String::new(), false),
            mes_skip: false,
            ticks: 0,
            last_draw: std::time::Instant::now(),
            is_timeout: false,
            last_input: String::new(),
            state: "TITLE".into(),
            log: Vec::new(),
            defer_warnings: false,
        };
        vm.defer_warnings = vm.fe.is_fullscreen();
        vm.con.width = vm.fe.cols().saturating_sub(1).max(20);
        vm.init_data();
        vm
    }

    /// An Emuera-Anchor hotkey: back to the title (Emuera's GotoTitle: a
    /// clear screen, no SPRITECREATE sprites or client background, default
    /// colours; unwinds as BEGIN TITLE), or reload the ERB files.
    pub fn hotkey(&mut self, h: crate::frontend::Hotkey) -> VmResult<()> {
        match h {
            crate::frontend::Hotkey::Title => {
                self.con.clear_lines(usize::MAX);
                self.con.cur.clear();
                self.res.dispose_all_sprites();
                self.res.cbg.clear();
                self.con.style.fg = self.con.default_fg;
                self.con.skip_disp = false;
                self.ctrain = None;
                Err(Ctrl::Begin("TITLE".into()))
            }
            crate::frontend::Hotkey::Reload => {
                self.reload_erb();
                Ok(())
            }
            crate::frontend::Hotkey::Config => {
                self.reload_config();
                Ok(())
            }
        }
    }

    /// Read the game's settings again (changed on the settings page).
    pub fn reload_config(&mut self) {
        if let Some(csv) = crate::loader::find_dir_ci(&self.game_dir, "CSV") {
            crate::emuconfig::load(&self.game_dir, &csv, &mut self.con, &mut self.text_exts, &mut self.save_opts);
        }
    }

    /// Is `name` a `#DIM CONST` without indices, here (the function's own
    /// or a global one)?
    fn is_const_scalar(&self, name: &str) -> bool {
        let own = self.frames.last().and_then(|fr| fr.body.dims.iter().find(|d| d.name == name));
        match own {
            Some(d) => d.is_const && d.dims.is_empty(),
            None => self.user_dims.get(name).is_some_and(|d| d.is_const && !d.is_chara && d.dims.is_empty()),
        }
    }

    /// The reload hotkey (Emuera's ReloadErb): parse the ERB files again
    /// and carry on with the game as it is. Functions already running
    /// finish in their old version; `#DIM`s new in the ERH files are
    /// added, changed ones keep their current size.
    pub fn reload_erb(&mut self) {
        let Some(mut f) = self.reloader.take() else { return };
        self.con.print("Reloading ERB…");
        self.con.newline();
        self.fe.flush(&mut self.con, &mut self.res);
        let t0 = std::time::Instant::now();
        let prog = f();
        self.reloader = Some(f);
        let errors = prog.errors.len();
        self.prog = prog;
        self.code_bytes = 0;
        // caches keyed by the old functions' addresses
        let dims = self.prog.global_dims.clone();
        for d in &dims {
            self.user_dims.insert(d.name.clone(), d.clone());
            if !d.is_chara && !self.globals.contains_key(&d.name) {
                if let Ok(a) = self.make_dim(d) {
                    self.globals.insert(d.name.clone(), a);
                }
            }
        }
        let n: usize = self.prog.funcs.values().map(|v| v.len()).sum();
        self.con.print(&format!("Reloaded {} functions in {:.1?} ({} parse errors)", n, t0.elapsed(), errors));
        self.con.newline();
        for e in self.prog.errors.iter().take(10).cloned().collect::<Vec<_>>() {
            self.con.print(&format!("  {}", e));
            self.con.newline();
        }
    }

    /// (Re)initialise all non-GLOBAL data: called at start and on RESETDATA.
    pub fn init_data(&mut self) {
        let keep: Vec<(String, Arr)> = self
            .globals
            .iter()
            .filter(|(k, _)| k.as_str() == "GLOBAL" || k.as_str() == "GLOBALS" || self.user_dims.get(*k).map(|d| d.is_global).unwrap_or(false))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.globals.clear();
        self.charas.clear();
        // function-owned LOCAL/ARG and static #DIMs go back to defaults too
        for a in &mut self.statics {
            *a = None;
        }
        for (k, v) in keep {
            self.globals.insert(k, v);
        }
        // user globals from ERH
        let dims = self.prog.global_dims.clone();
        for d in &dims {
            self.user_dims.insert(d.name.clone(), d.clone());
        }
        // sizes may name CONSTs declared in later files: allocate in passes
        let mut todo: Vec<&DimDecl> = dims.iter().filter(|d| !d.is_chara && !self.globals.contains_key(&d.name)).collect();
        loop {
            let mut failed = Vec::new();
            for d in &todo {
                match self.make_dim(d) {
                    Ok(a) => {
                        self.globals.insert(d.name.clone(), a);
                    }
                    Err(e) => failed.push((*d, e)),
                }
            }
            if failed.is_empty() || failed.len() == todo.len() {
                for (d, e) in &failed {
                    self.warn(&format!("#DIM {}: {:?}", d.name, e));
                }
                break;
            }
            todo = failed.into_iter().map(|(d, _)| d).collect();
        }
        // CSV initial values
        let strs = self.csv.str_init.clone();
        for (i, s) in strs {
            self.global_arr("STR").set(i.max(0) as usize, Value::Str(s));
        }
        let prices: Vec<(i64, i64)> = self.csv.item_price.iter().map(|(a, b)| (*a, *b)).collect();
        for (i, p) in prices {
            self.global_arr("ITEMPRICE").set(i.max(0) as usize, Value::Int(p));
        }
        self.global_arr("MASTER").set_i(0, 0);
        self.global_arr("TARGET").set_i(0, 1);
        self.global_arr("ASSI").set_i(0, -1);
        // Emuera's level tables (_Replace.csv may give its own, `a/b/c…`)
        for (var, key, def) in [
            ("PALAMLV", "PALAMLVの初期値", &[0i64, 100, 500, 3000, 10000, 30000, 60000, 100000, 150000, 250000][..]),
            ("EXPLV", "EXPLVの初期値", &[0, 1, 4, 20, 50, 200][..]),
        ] {
            let vals: Vec<i64> = match self.csv.replace.get(key) {
                Some(s) => s.split('/').filter_map(|x| x.trim().parse().ok()).collect(),
                None => def.to_vec(),
            };
            let a = self.global_arr(var);
            for (i, v) in vals.into_iter().enumerate().take(a.len()) {
                a.set_i(i, v);
            }
        }
    }

    /// Allocate a `#DIM` declaration: evaluate its sizes and initialisers.
    fn make_dim(&mut self, d: &DimDecl) -> VmResult<Arr> {
        let mut dims = Vec::new();
        for e in &d.dims {
            dims.push(self.eval(e)?.as_int().max(0) as usize);
        }
        if dims.is_empty() {
            dims.push(d.init.len().max(1));
        }
        // a #DIM without size is a scalar unless initialised with several values
        let dims_eff: Vec<usize> = if d.dims.is_empty() && d.init.len() <= 1 { vec![] } else { dims };
        let mut a = Arr::new(d.is_str, &dims_eff);
        for (i, e) in d.init.clone().iter().enumerate() {
            let v = self.eval(e)?;
            a.set(i, v);
        }
        Ok(a)
    }

    /// Record a warning, once per distinct message. Printed to stderr right
    /// away in headless mode; the full-screen UI shows them after quitting
    /// (`log`), since writing mid-frame would scribble over the screen.
    pub fn warn(&mut self, msg: &str) {
        if self.warned.insert(msg.to_string()) {
            if !self.defer_warnings {
                eprintln!("warning: {}", msg);
            }
            self.log.push(msg.to_string());
        }
    }

    // ---------------------------------------------------------------- rng
    /// RAND:n — uniform 0..n via xorshift64*. Not Emuera's generator, so
    /// seeded sequences differ from the original (scripts rarely rely on it).
    pub fn rand(&mut self, n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 1) % (n as u64)) as i64
    }
    /// RANDOMIZE / INITRAND: reseed.
    pub fn seed(&mut self, s: i64) {
        self.rng = (s as u64) ^ 0x9E37_79B9_7F4A_7C15 | 1;
    }

    // ------------------------------------------------------------ variables
    fn var_def_dims(&self, name: &str, def: &VarDef) -> Vec<usize> {
        sized_dims(&self.csv.varsize, name, def)
    }

    /// A global array, created with its default size on first use.
    /// The global slot `v` names (remembered in the reference).
    fn global_slot(&mut self, v: &VarRef) -> usize {
        use std::sync::atomic::Ordering::Relaxed;
        match v.gslot.load(Relaxed) {
            0 => {
                let s = self.globals.slot_of(&v.name);
                v.gslot.store(s as u32 + 1, Relaxed);
                s
            }
            s => s as usize - 1,
        }
    }

    pub fn global_arr(&mut self, name: &str) -> &mut Arr {
        // one lookup when it exists (the raw pointer only works around the
        // borrow checker's conditional-return limitation)
        if let Some(a) = self.globals.get_mut(name) {
            let a: *mut Arr = a;
            return unsafe { &mut *a };
        }
        {
            let a = match system_var(name) {
                Some(def) => Arr::new(def.is_str, &self.var_def_dims(name, def)),
                None => Arr::new(false, &[1]),
            };
            self.globals.insert(name.to_string(), a);
        }
        self.globals.get_mut(name).unwrap()
    }

    /// Character `c`'s array for `name`, created on first use (system
    /// character variable or `#DIM CHARADATA`).
    pub fn chara_arr(&mut self, c: usize, name: &str) -> &mut Arr {
        // one lookup when it exists (see global_arr)
        if let Some(a) = self.charas[c].vars.get_mut(name) {
            let a: *mut Arr = a;
            return unsafe { &mut *a };
        }
        {
            let a = if let Some(def) = system_var(name) {
                Arr::new(def.is_str, &self.var_def_dims(name, def))
            } else if let Some(d) = self.user_dims.get(name).cloned() {
                let mut dims = Vec::new();
                for e in &d.dims {
                    dims.push(self.eval(e).map(|v| v.as_int()).unwrap_or(1).max(0) as usize);
                }
                Arr::new(d.is_str, &dims)
            } else {
                Arr::new(false, &[1])
            };
            self.charas[c].vars.insert(name.to_string(), a);
        }
        self.charas[c].vars.get_mut(name).unwrap()
    }

    pub fn get_global_i(&mut self, name: &str, i: usize) -> i64 {
        self.global_arr(name).get_i(i)
    }
    pub fn set_global_i(&mut self, name: &str, i: usize, v: i64) {
        self.global_arr(name).set_i(i, v)
    }
    pub fn set_result(&mut self, v: i64) {
        self.set_global_i("RESULT", 0, v)
    }
    pub fn set_results(&mut self, s: String) {
        self.global_arr("RESULTS").set(0, Value::Str(s))
    }

    fn frame_private(&self, name: &str) -> Option<&DimDecl> {
        let f = self.frames.last()?;
        f.body.dims.iter().find(|d| d.name == name)
    }

    /// Is `name` resolvable as a variable here (used for CSV-name indices)?
    pub fn var_exists(&self, name: &str) -> bool {
        if let Some(f) = self.frames.last() {
            if f.vars.contains_key(name) || f.body.dims.iter().any(|d| d.name == name) {
                return true;
            }
        }
        matches!(name.split_once('@').map_or(name, |(b, _)| b), "LOCAL" | "LOCALS" | "ARG" | "ARGS")
            || self.user_dims.contains_key(name)
            || system_var(name).is_some()
            || pseudo_var(name)
    }

    /// One index value. A bare identifier that isn't a variable, or a
    /// string, is a CSV name looked up in `table` (or a table named after
    /// the variable, for ERD files: `NAME.ERD` for a 1-D array, `NAME@k.ERD`
    /// for index k of a multi-dimensional one, given as `dim`).
    fn eval_index(&mut self, e: &Expr, table: Option<&str>, var: &str) -> VmResult<i64> {
        self.eval_index_dim(e, table, var, None)
    }

    fn eval_index_dim(&mut self, e: &Expr, table: Option<&str>, var: &str, dim: Option<usize>) -> VmResult<i64> {
        match e {
            Expr::Var(v) if v.idx.is_empty() && !self.var_exists(&v.name) => {
                if let Some(t) = table {
                    if let Some(i) = self.csv.lookup(t, &v.name) {
                        return Ok(i);
                    }
                }
                if let Some(d) = dim {
                    if let Some(i) = self.csv.lookup(&format!("{}@{}", var.to_uppercase(), d), &v.name) {
                        return Ok(i);
                    }
                }
                if let Some(i) = self.csv.lookup(var, &v.name) {
                    return Ok(i);
                }
                err(format!("'{}' is neither a variable nor a {} name", v.name, table.unwrap_or(var)))
            }
            Expr::Str(s) => {
                let t = table.unwrap_or(var);
                self.csv.lookup(t, s).or_else(|| self.csv.lookup(t, &s.to_uppercase())).ok_or(()).or_else(|_| err(format!("unknown {} name \"{}\"", t, s)))
            }
            _ => Ok(self.eval(e)?.as_int()),
        }
    }

    /// All indices of `v` from position `from` (1 skips a character index).
    fn eval_indices(&mut self, v: &VarRef, from: usize, table: Option<&str>) -> VmResult<Idx> {
        let mut out = Idx::default();
        let n = v.idx.len();
        for (k, e) in v.idx.iter().enumerate().skip(from) {
            // the usual case: a number or a plain counter
            let i = match e {
                Expr::Int(i) => *i,
                _ => {
                    // CSV names apply to the last index (and to every index of ERD vars)
                    let t = if k + 1 == n { table } else { None };
                    let dim = (n - from > 1).then_some(k - from + 1);
                    self.eval_index_dim(e, t, &v.name, dim)?
                }
            };
            if out.n as usize == out.v.len() {
                return err(format!("{}: too many indices", v.name));
            }
            out.v[out.n as usize] = i;
            out.n += 1;
        }
        Ok(out)
    }

    /// TARGET: the default character for character variables.
    pub fn target(&mut self) -> i64 {
        self.get_global_i("TARGET", 0)
    }

    fn chara_index(&mut self, c: i64, name: &str) -> VmResult<usize> {
        if c < 0 || c as usize >= self.charas.len() {
            return err(format!("{}: character index {} out of range (CHARANUM={})", name, c, self.charas.len()));
        }
        Ok(c as usize)
    }

    /// Resolve a variable reference to a [`Place`]. Lookup order: REF
    /// aliases, the current function's privates and LOCAL/ARG, ERH `#DIM`s,
    /// CSV name tables (ABLNAME…), read-only pseudo variables, system
    /// variables. Unknown names are an error.
    /// How `v`'s name resolves in the running function (cached: the same
    /// name in the same function always means the same kind of variable).
    fn var_info(&mut self, v: &VarRef) -> VarInfo {
        let name = &*v.name;
        let ni = match v.cache.load(std::sync::atomic::Ordering::Relaxed) {
            0 => {
                let table = match table_for_var(name) {
                    Some(t) => Tbl::Static(t),
                    None if self.csv.tables.contains_key(name) => Tbl::Own,
                    None => Tbl::None,
                };
                let n = NameInfo {
                    table,
                    // Emuera's LOCAL/ARG family is per-function storage that
                    // persists across calls (reset by RESETDATA / load)
                    local_family: match name {
                        "LOCAL" => 1,
                        "LOCALS" => 2,
                        "ARG" => 3,
                        "ARGS" => 4,
                        _ => 0,
                    },
                    user: self.user_dims.get(name).map(|d| (d.is_chara, d.dims.len())),
                    csv_name: name.ends_with("NAME") && name != "NAME" && table_for_var(name).is_some(),
                    pseudo: pseudo_var(name),
                    system: system_var(name),
                };
                v.cache.store(n.pack(), std::sync::atomic::Ordering::Relaxed);
                n
            }
            packed => NameInfo::unpack(packed),
        };
        // the function's own #DIMs win over everything else
        // skey: its entry in the function's static slot cache
        let (private, is_static, skey) = match self.frames.last() {
            Some(f) if !f.body.dims.is_empty() => match f.body.dims.iter().position(|d| d.name.len() == name.len() && d.name == name) {
                Some(k) => (true, f.body.dims[k].is_static, 4 + k as u32),
                None => (false, ni.local_family != 0, ni.local_family as u32 - (ni.local_family != 0) as u32),
            },
            Some(_) => (false, ni.local_family != 0, (ni.local_family as u32).saturating_sub(1)),
            None => (false, false, 0),
        };
        VarInfo { table: ni.table, private, is_static, skey, user: ni.user, csv_name: ni.csv_name, pseudo: ni.pseudo, system: ni.system }
    }

    pub fn locate(&mut self, v: &VarRef) -> VmResult<Place> {
        let name = &*v.name;
        let info = self.var_info(v);
        let table: Option<&str> = match info.table {
            Tbl::None => None,
            Tbl::Static(t) => Some(t),
            Tbl::Own => Some(name),
        };
        // 0. REF parameter aliases
        if let Some(loc) = self.frames.last().and_then(|f| if f.refs.is_empty() { None } else { f.refs.get(name) }).cloned() {
            let idx = self.eval_indices(v, 0, table)?;
            let a = self.arr_at(&loc).ok_or_else(|| Ctrl::Error(format!("{}: referenced variable is gone", name), false))?;
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc, flat });
        }
        // 1. frame-local
        let (is_private, is_static) = (info.private, info.is_static);
        if is_static {
            // the slot is remembered in the reference, tagged with the function
            use std::sync::atomic::Ordering::Relaxed;
            let body = self.frames.last().unwrap().body.clone();
            let cache = body.static_cache.get_or_init(|| (0..4 + body.dims.len()).map(|_| std::sync::atomic::AtomicU32::new(0)).collect());
            let cached = cache[info.skey as usize].load(Relaxed);
            let slot = if cached != 0 {
                cached as usize - 1
            } else {
                // storage is per function name (event functions with one
                // name share it)
                let k = (self.frames.last().unwrap().func.name.to_string(), name.to_string());
                let s = match self.static_index.get(&k) {
                    Some(&s) => s,
                    None => {
                        self.statics.push(None);
                        self.static_index.insert(k, self.statics.len() - 1);
                        self.statics.len() - 1
                    }
                };
                cache[info.skey as usize].store(s as u32 + 1, Relaxed);
                s
            };
            if self.statics[slot].is_none() {
                let a = match self.frame_private(name).cloned() {
                    Some(d) => self.make_dim(&d)?,
                    None => {
                        // #LOCALSIZE / #LOCALSSIZE (1-D arrays still grow on write)
                        let f = &self.frames.last().unwrap().func;
                        match name {
                            "LOCAL" => Arr::new(false, &[if f.local_size > 0 { f.local_size as usize } else { 1000 }]),
                            "LOCALS" => Arr::new(true, &[if f.locals_size > 0 { f.locals_size as usize } else { 100 }]),
                            "ARG" => Arr::new(false, &[1000]),
                            _ => Arr::new(true, &[100]),
                        }
                    }
                };
                self.statics[slot] = Some(a);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let flat = self.statics[slot].as_ref().unwrap().flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Static(slot), flat });
        }
        // DYNAMIC privates: allocated per call by `call_func`
        if is_private {
            let idx = self.eval_indices(v, 0, table)?;
            let f = self.frames.last().unwrap();
            let a = f.vars.get(name).ok_or_else(|| Ctrl::Error(format!("{} not allocated", name), false))?;
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Frame(v.name.clone()), flat });
        }
        // 2. user globals
        if let Some((is_chara, nd)) = info.user {
            if is_chara {
                return self.locate_chara(v, nd, table);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let slot = self.global_slot(v);
            let a = self.globals.at_ref(slot).ok_or_else(|| Ctrl::Error(format!("{} not allocated", name), false))?;
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Global(slot), flat });
        }
        // 3a. CSV name tables (ABLNAME, BASENAME, …) as real string arrays,
        // so FINDELEMENT / VARSIZE / array builtins can take them
        if info.csv_name {
            if let Some(t) = table_for_var(name) {
                let slot = self.global_slot(v);
                if self.globals.at_ref(slot).is_none() {
                    let tbl = self.csv.tables.get(t);
                    let max = tbl.and_then(|t| t.by_index.keys().max().copied()).unwrap_or(-1) + 1;
                    let size = match system_var(t) {
                        Some(def) => self.var_def_dims(t, def).last().copied().unwrap_or(0),
                        None => 0,
                    }
                    .max(max.max(1) as usize);
                    let mut a = Arr::new(true, &[size]);
                    if let Some(tbl) = tbl {
                        for (i, s) in &tbl.by_index {
                            a.set((*i).max(0) as usize, Value::Str(s.clone()));
                        }
                    }
                    self.globals.insert(name.to_string(), a);
                }
                let idx = self.eval_indices(v, 0, table)?;
                let flat = self.globals.at_ref(slot).unwrap().flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
                return Ok(Place { loc: Loc::Global(slot), flat });
            }
        }
        // 3. pseudo variables
        if info.pseudo {
            let val = self.pseudo_value(v, table)?;
            return Ok(Place { loc: Loc::Ro(val), flat: 0 });
        }
        // 4. system variables
        if let Some(def) = info.system {
            if def.chara {
                return self.locate_chara(v, def.dims.len(), table);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let slot = self.global_slot(v);
            if self.globals.at_ref(slot).is_none() {
                self.global_arr(name);
            }
            let a = self.globals.at_ref(slot).unwrap();
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Global(slot), flat });
        }
        if let Some((base, func)) = name.split_once('@') {
            return self.locate_other_local(v, base, func);
        }
        err(format!("undefined variable '{}'", name))
    }

    /// `LOCAL@FUNC` (LOCALS, ARG, ARGS): that function's own LOCAL, the
    /// storage it uses itself (Emuera allows it, with a warning).
    fn locate_other_local(&mut self, v: &VarRef, base: &str, func: &str) -> VmResult<Place> {
        if !matches!(base, "LOCAL" | "LOCALS" | "ARG" | "ARGS") {
            return err(format!("{}: only LOCAL, LOCALS, ARG and ARGS can be another function's (@{})", v.name, func));
        }
        let Some(f) = self.prog.funcs.get(func).and_then(|fs| fs.first()).cloned() else {
            return err(format!("{}: there is no function @{}", v.name, func));
        };
        let k = (f.name.to_string(), base.to_string());
        let slot = match self.static_index.get(&k) {
            Some(&s) => s,
            None => {
                self.statics.push(None);
                self.static_index.insert(k, self.statics.len() - 1);
                self.statics.len() - 1
            }
        };
        if self.statics[slot].is_none() {
            self.statics[slot] = Some(match base {
                "LOCAL" => Arr::new(false, &[if f.local_size > 0 { f.local_size as usize } else { 1000 }]),
                "LOCALS" => Arr::new(true, &[if f.locals_size > 0 { f.locals_size as usize } else { 100 }]),
                "ARG" => Arr::new(false, &[1000]),
                _ => Arr::new(true, &[100]),
            });
        }
        let idx = self.eval_indices(v, 0, None)?;
        let flat = self.statics[slot].as_ref().unwrap().flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", v.name, e), false))?;
        Ok(Place { loc: Loc::Static(slot), flat })
    }

    /// Character variables: `CFLAG:c:i` names the character, `CFLAG:i`
    /// (one index fewer than the variable has) means TARGET's.
    fn locate_chara(&mut self, v: &VarRef, ndim: usize, table: Option<&str>) -> VmResult<Place> {
        let (c, rest) = if v.idx.len() > ndim {
            (self.eval(&v.idx[0])?.as_int(), 1)
        } else {
            (self.target(), 0)
        };
        let ci = self.chara_index(c, &v.name)?;
        let idx = self.eval_indices(v, rest, table)?;
        let a = self.chara_arr(ci, &v.name);
        let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", v.name, e), false))?;
        Ok(Place { loc: Loc::Chara(ci, v.name.clone()), flat })
    }

    /// The array a location refers to (None for read-only values).
    pub fn arr_at(&mut self, loc: &Loc) -> Option<&mut Arr> {
        match loc {
            Loc::Global(s) => self.globals.at(*s),
            Loc::Frame(n) => self.frames.last_mut().and_then(|f| f.vars.get_mut(&**n)),
            Loc::FrameAt(d, n) => self.frames.get_mut(*d).and_then(|f| f.vars.get_mut(&**n)),
            Loc::Static(slot) => self.statics.get_mut(*slot).and_then(|a| a.as_mut()),
            Loc::Chara(c, n) => Some(self.chara_arr(*c, n)),
            Loc::Ro(_) => None,
        }
    }

    pub fn get_place(&mut self, p: &Place) -> Value {
        if let Loc::Ro(v) = &p.loc {
            return v.clone();
        }
        let f = p.flat;
        self.arr_at(&p.loc).map(|a| a.get(f)).unwrap_or(Value::Int(0))
    }

    pub fn set_place(&mut self, p: &Place, val: Value) -> VmResult<()> {
        match &p.loc {
            Loc::Ro(_) => err("cannot assign to read-only variable"),
            loc => {
                let f = p.flat;
                if let Some(a) = self.arr_at(loc) {
                    a.set(f, val);
                }
                Ok(())
            }
        }
    }

    pub fn get_var(&mut self, v: &VarRef) -> VmResult<Value> {
        let p = self.locate(v)?;
        Ok(self.get_place(&p))
    }

    /// Assign to a variable. A few computed variables accept writes and
    /// are handled here (they have no backing array).
    pub fn set_var(&mut self, v: &VarRef, val: Value) -> VmResult<()> {
        // writable pseudo-ish strings
        if matches!(&*v.name, "WINDOW_TITLE" | "DRAWLINESTR" | "SAVEDATA_TEXT") {
            let s = val.into_string();
            match &*v.name {
                "WINDOW_TITLE" => self.con.title = s,
                "DRAWLINESTR" => self.con.drawline_str = s,
                _ => {
                    self.globals.get_or_insert_with("SAVEDATA_TEXT", || Arr::new(true, &[1])).set(0, Value::Str(s));
                }
            }
            return Ok(());
        }
        let p = self.locate(v)?;
        self.set_place(&p, val)
    }

    /// Whether the variable holds strings (decides parameter defaults etc.).
    pub fn var_is_str(&mut self, v: &VarRef) -> VmResult<bool> {
        let p = self.locate(v)?;
        Ok(match &p.loc {
            Loc::Ro(v) => v.is_str(),
            loc => self.arr_at(loc).map(|a| a.is_str()).unwrap_or(false),
        })
    }

    /// Values of the computed, read-only variables (see `vars::pseudo_var`).
    fn pseudo_value(&mut self, v: &VarRef, table: Option<&str>) -> VmResult<Value> {
        let name = &*v.name;
        let i0 = |vm: &mut Vm| -> VmResult<i64> {
            match v.idx.first() {
                Some(e) => vm.eval_index(e, table, name),
                None => Ok(0),
            }
        };
        Ok(match name {
            "RAND" => {
                let n = i0(self)?;
                Value::Int(self.rand(n))
            }
            "CHARANUM" => Value::Int(self.charas.len() as i64),
            "LINECOUNT" => Value::Int(self.con.line_count() as i64),
            "ISTIMEOUT" => Value::Int(self.is_timeout as i64),
            "__INT_MAX__" => Value::Int(i64::MAX),
            "__INT_MIN__" => Value::Int(i64::MIN),
            "WINDOW_TITLE" => Value::Str(self.con.title.clone()),
            // the full rule line DRAWLINE prints, not just its unit
            "DRAWLINESTR" => {
                let unit = if self.con.drawline_str.is_empty() { "-" } else { &self.con.drawline_str };
                Value::Str(crate::console::repeat_to_width(unit, self.con.width))
            }
            "MONEYLABEL" => Value::Str(self.money_label().0),
            "SAVEDATA_TEXT" => self.globals.get("SAVEDATA_TEXT").map(|a| a.get(0)).unwrap_or(Value::Str(String::new())),
            "LASTLOAD_TEXT" | "LASTLOAD_NO" | "LASTLOAD_VERSION" => {
                self.globals.get(name).map(|a| a.get(0)).unwrap_or(if name == "LASTLOAD_TEXT" { Value::Str(String::new()) } else { Value::Int(-1) })
            }
            "GAMEBASE_CODE" => Value::Int(self.gb_int("コード")),
            "GAMEBASE_VERSION" => Value::Int(self.gb_int("バージョン")),
            "GAMEBASE_ALLOWVERSION" => Value::Int(self.gb_int("バージョン違い認める")),
            "GAMEBASE_DEFAULTCHARA" => Value::Int(self.gb_int("最初からいるキャラ")),
            "GAMEBASE_NOITEM" => Value::Int(self.gb_int("アイテムなし")),
            "GAMEBASE_AUTHOR" | "GAMEBASE_AUTHER" => Value::Str(self.gb_str("作者")),
            "GAMEBASE_YEAR" => Value::Str(self.gb_str("製作年")),
            "GAMEBASE_TITLE" => Value::Str(self.gb_str("タイトル")),
            "GAMEBASE_INFO" => Value::Str(self.gb_str("追加情報")),
            "CDFLAGNAME1" | "CDFLAGNAME2" => {
                let i = i0(self)?;
                let t = if name.ends_with('1') { "CDFLAG1" } else { "CDFLAG2" };
                Value::Str(self.csv.name_of(t, i).unwrap_or("").to_string())
            }
            n if n.ends_with("NAME") && table_for_var(n).is_some() => {
                let i = i0(self)?;
                Value::Str(self.csv.name_of(table_for_var(n).unwrap(), i).unwrap_or("").to_string())
            }
            _ => return err(format!("pseudo variable {} not implemented", name)),
        })
    }

    fn gb_str(&self, k: &str) -> String {
        self.csv.gamebase.get(k).cloned().unwrap_or_default()
    }
    fn gb_int(&self, k: &str) -> i64 {
        self.csv.gamebase.get(k).and_then(|s| s.trim().parse().ok()).unwrap_or(0)
    }

    // ---------------------------------------------------------- expressions
    /// Evaluate an expression. Integers wrap on overflow, as in C#.
    pub fn eval(&mut self, e: &Expr) -> VmResult<Value> {
        match e {
            Expr::Int(i) => Ok(Value::Int(*i)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Empty => Ok(Value::Int(0)),
            Expr::Form(f) => Ok(Value::Str(self.eval_form(f)?)),
            Expr::Var(v) => self.get_var(v),
            Expr::Call(c) => self.call_expr(&c.0, &c.1),
            Expr::Unary(op, x) => {
                let v = self.eval(x)?;
                Ok(Value::Int(match op {
                    UnOp::Neg => v.as_int().wrapping_neg(),
                    UnOp::Not => (!v.truthy()) as i64,
                    UnOp::BitNot => !v.as_int(),
                }))
            }
            Expr::Binary(op, a, b) => self.binary(*op, a, b),
            Expr::Ternary(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)
                } else {
                    self.eval(b)
                }
            }
            Expr::IncDec(v, pre, d) => {
                let p = self.locate(v)?;
                let old = self.get_place(&p).as_int();
                let new = old.wrapping_add(*d);
                self.set_place(&p, Value::Int(new))?;
                Ok(Value::Int(if *pre { new } else { old }))
            }
        }
    }

    pub fn eval_int(&mut self, e: &Expr) -> VmResult<i64> {
        Ok(self.eval(e)?.as_int())
    }
    pub fn eval_str(&mut self, e: &Expr) -> VmResult<String> {
        Ok(self.eval(e)?.into_string())
    }

    /// Binary operators. `&&`/`||` short-circuit; if either side is a
    /// string, `+` concatenates, `*` repeats, comparisons compare text.
    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr) -> VmResult<Value> {
        match op {
            BinOp::And => {
                let x = self.eval(a)?.truthy();
                return Ok(Value::Int((x && self.eval(b)?.truthy()) as i64));
            }
            BinOp::Or => {
                let x = self.eval(a)?.truthy();
                return Ok(Value::Int((x || self.eval(b)?.truthy()) as i64));
            }
            BinOp::Nand => {
                let x = self.eval(a)?.truthy();
                let y = self.eval(b)?.truthy();
                return Ok(Value::Int(!(x && y) as i64));
            }
            BinOp::Nor => {
                let x = self.eval(a)?.truthy();
                let y = self.eval(b)?.truthy();
                return Ok(Value::Int(!(x || y) as i64));
            }
            _ => {}
        }
        let x = self.eval(a)?;
        let y = self.eval(b)?;
        if x.is_str() || y.is_str() {
            if op == BinOp::Mul {
                // "abc" * 3
                let (s, n) = match (&x, &y) {
                    (Value::Str(s), v) | (v, Value::Str(s)) => (s.clone(), v.as_int()),
                    _ => unreachable!(),
                };
                return Ok(Value::Str(s.repeat(n.max(0) as usize)));
            }
            let (xs, ys) = (x.as_string(), y.as_string());
            return Ok(match op {
                BinOp::Add => Value::Str(xs + &ys),
                BinOp::Eq => Value::Int((xs == ys) as i64),
                BinOp::Ne => Value::Int((xs != ys) as i64),
                BinOp::Lt => Value::Int((xs < ys) as i64),
                BinOp::Le => Value::Int((xs <= ys) as i64),
                BinOp::Gt => Value::Int((xs > ys) as i64),
                BinOp::Ge => Value::Int((xs >= ys) as i64),
                _ => return err(format!("operator {:?} not valid for strings", op)),
            });
        }
        let (x, y) = (x.as_int(), y.as_int());
        Ok(Value::Int(match op {
            BinOp::Add => x.wrapping_add(y),
            BinOp::Sub => x.wrapping_sub(y),
            BinOp::Mul => x.wrapping_mul(y),
            BinOp::Div => {
                if y == 0 {
                    return err("division by zero");
                }
                x.wrapping_div(y)
            }
            BinOp::Mod => {
                if y == 0 {
                    return err("modulo by zero");
                }
                x.wrapping_rem(y)
            }
            BinOp::Shl => x.wrapping_shl(y as u32),
            BinOp::Shr => x.wrapping_shr(y as u32),
            BinOp::Lt => (x < y) as i64,
            BinOp::Le => (x <= y) as i64,
            BinOp::Gt => (x > y) as i64,
            BinOp::Ge => (x >= y) as i64,
            BinOp::Eq => (x == y) as i64,
            BinOp::Ne => (x != y) as i64,
            BinOp::BitAnd => x & y,
            BinOp::BitOr => x | y,
            BinOp::BitXor => x ^ y,
            _ => unreachable!(),
        }))
    }

    /// Build a FORM string's text.
    pub fn eval_form(&mut self, f: &Form) -> VmResult<String> {
        // a lone literal (common for FORM arguments): no building
        if let [FormPart::Lit(s)] = &f.parts[..] {
            return Ok(s.clone());
        }
        let mut out = String::new();
        self.eval_form_into(f, &mut out, false)?;
        Ok(out)
    }

    /// A FORM string that is printed (Emuera-Anchor translation).
    pub(crate) fn eval_form_tr(&mut self, f: &Form) -> VmResult<String> {
        let mut out = String::new();
        self.eval_form_into(f, &mut out, true)?;
        Ok(out)
    }

    /// Emuera-Anchor: a printed `%…%` value, translated — a CSV name
    /// (`%ABLNAME:2%`) through its table's *_TR.csv, anything else
    /// through ALL_TR.csv.
    fn translated(&self, e: &Expr, s: String) -> String {
        let tr = &self.csv.tr;
        if tr.is_empty() {
            return s;
        }
        if let Expr::Var(v) = e {
            if v.name.ends_with("NAME") {
                if let Some(t) = crate::csvdata::table_for_var(&v.name) {
                    return tr.name(t, &s).map(str::to_string).unwrap_or(s);
                }
            }
        }
        tr.all(&s).cloned().unwrap_or(s)
    }

    /// Append a FORM string; `tr`: it is being printed (see `translated`).
    fn eval_form_into(&mut self, f: &Form, out: &mut String, tr: bool) -> VmResult<()> {
        use std::fmt::Write as _;
        for p in &f.parts {
            match p {
                FormPart::Lit(s) => out.push_str(s),
                FormPart::Str(e, w, a) | FormPart::Int(e, w, a) => {
                    let mut v = self.eval(e)?;
                    if tr {
                        if let Value::Str(s) = v {
                            v = Value::Str(self.translated(e, s));
                        }
                    }
                    match w {
                        Some(w) => {
                            let w = self.eval_int(w)?.max(0) as usize;
                            out.push_str(&pad(&v.into_string(), w, *a));
                        }
                        None => match v {
                            Value::Int(i) => {
                                let _ = write!(out, "{}", i);
                            }
                            Value::Str(s) => out.push_str(&s),
                        },
                    }
                }
                FormPart::Cond(cab) => {
                    let (c, a, b) = &**cab;
                    if self.eval(c)?.truthy() {
                        self.eval_form_into(a, out, tr)?;
                    } else {
                        self.eval_form_into(b, out, tr)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// A call inside an expression: user functions first, then built-ins.
    /// A user function that returns nothing yields 0 / "".
    pub(crate) fn call_expr(&mut self, name: &str, args: &[Expr]) -> VmResult<Value> {
        if let Some(fs) = self.prog.funcs.get(name) {
            let f = fs[0].clone();
            let (vals, refs) = self.call_args(&f, args)?;
            self.pending_refs = refs;
            let r = self.call_func(f.clone(), vals)?;
            return Ok(r.unwrap_or(if f.returns_str { Value::Str(String::new()) } else { Value::Int(0) }));
        }
        match self.builtin_fn(name, args)? {
            Some(v) => Ok(v),
            None => err(format!("unknown function '{}'", name)),
        }
    }

    // ------------------------------------------------------------ functions
    pub fn has_func(&self, name: &str) -> bool {
        self.prog.funcs.contains_key(name)
    }

    /// Call a function or event by name. Returns Ok(false) if it doesn't exist.
    pub fn call_name(&mut self, name: &str, args: Vec<Option<Value>>) -> VmResult<bool> {
        let fs = match self.prog.funcs.get(name) {
            None => return Ok(false),
            Some(fs) if fs.len() == 1 => {
                let f = fs[0].clone();
                self.call_func(f, args)?;
                return Ok(true);
            }
            Some(fs) => fs.clone(),
        };
        // event function: #ONLY, then #PRI, normal, #LATER; #SINGLE stops
        if let Some(only) = fs.iter().find(|f| f.only) {
            self.call_func(only.clone(), args)?;
            return Ok(true);
        }
        let mut ordered = fs.clone();
        ordered.sort_by_key(|f| f.order);
        for f in ordered {
            self.call_func(f.clone(), args.clone())?;
            if f.single {
                break;
            }
        }
        Ok(true)
    }

    /// Evaluate call arguments; `#DIM REF` parameters instead resolve the
    /// caller's variable. Store the refs in `pending_refs` right before the
    /// call (not earlier: argument expressions may make calls of their own).
    fn call_args(&mut self, f: &Function, args: &[Expr]) -> VmResult<(Vec<Option<Value>>, Vec<(String, Loc)>)> {
        let mut vals = Vec::with_capacity(args.len());
        let mut refs = Vec::new();
        let body = f.body();
        for (i, a) in args.iter().enumerate() {
            let p = f.params.get(i).map(|p| &p.target.name);
            let is_ref = !body.dims.is_empty() && p.is_some_and(|n| body.dims.iter().any(|d| d.is_ref && *d.name == **n));
            if let (true, Expr::Var(v)) = (is_ref, a) {
                let top = self.frames.len().saturating_sub(1);
                let loc = match self.locate(&VarRef::new(v.name.clone(), vec![]))?.loc {
                    Loc::Frame(n) => Loc::FrameAt(top, n),
                    l => l,
                };
                refs.push((p.unwrap().to_string(), loc));
                vals.push(None);
                continue;
            }
            vals.push(match a {
                Expr::Empty => None,
                e => Some(self.eval(e)?),
            });
        }
        Ok((vals, refs))
    }

    /// Run one function: set up its frame (DYNAMIC privates, REF aliases
    /// from `pending_refs`), bind parameters, execute, pop. Returns the
    /// RETURNF value, if any.
    pub fn call_func(&mut self, f: Rc<Function>, mut args: Vec<Option<Value>>) -> VmResult<Option<Value>> {
        let refs: HashMap<String, Loc> = if self.pending_refs.is_empty() { HashMap::default() } else { std::mem::take(&mut self.pending_refs).into_iter().collect() };
        if prof::ON.load(std::sync::atomic::Ordering::Relaxed) {
            prof::enter(&f);
        }
        // debugging aid: EMUERA_TRACE=<substring of function name>
        if let Some(t) = &self.trace {
            if f.name.contains(t.as_str()) {
                eprintln!("trace {:8.3}s: {}@{} args={:?} CHARANUM={}", trace_clock(), "  ".repeat(self.frames.len()), f.name, args, self.charas.len());
            }
        }
        if self.frames.len() >= MAX_DEPTH {
            return err("call stack overflow");
        }
        // (compiled on its first call: report what didn't parse)
        let body = f.body();
        self.calls += 1;
        if body.used.swap(self.calls, std::sync::atomic::Ordering::Relaxed) == 0 && f.lazy.is_some() {
            self.code_bytes += body.approx_bytes();
        }
        if crate::parser::LAZY_ERRORS.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(l) = &f.lazy {
                for e in l.take_errors() {
                    self.warn(&format!("parse: {}", e));
                }
            }
        }
        // allocate private (non-static) dims in the caller context
        let mut vars = HashMap::default();
        for d in &body.dims {
            if !d.is_static {
                let a = self.make_dim(d)?;
                vars.insert(d.name.clone(), a);
            }
        }
        let slots = vec![Slot::None; body.n_slots];
        self.frames.push(Frame { func: f.clone(), body: body.clone(), pc: 0, vars, slots, refs });
        // bind parameters
        for (i, p) in f.params.iter().enumerate() {
            // (moved out: the arguments aren't used again)
            let v = match args.get_mut(i).and_then(|a| a.take()) {
                Some(v) => Some(v),
                None => match &p.default {
                    Some(e) => match self.eval(e) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            self.frames.pop();
                            return Err(e);
                        }
                    },
                    // no declared default: ARG/ARGS/private params get 0 / ""
                    // (Emuera's implicit default), never the previous call's value
                    None => {
                        let n = &*p.target.name;
                        let is_param_var = matches!(n, "ARG" | "ARGS") || body.dims.iter().any(|d| d.name == n && !d.is_ref);
                        if is_param_var {
                            match self.var_is_str(&p.target) {
                                Ok(true) => Some(Value::Str(String::new())),
                                Ok(false) => Some(Value::Int(0)),
                                Err(e) => {
                                    self.frames.pop();
                                    return Err(e);
                                }
                            }
                        } else {
                            None
                        }
                    }
                },
            };
            if let Some(v) = v {
                if let Err(e) = self.set_var(&p.target, v) {
                    self.frames.pop();
                    return Err(e);
                }
            }
        }
        if args.len() > f.params.len() && args[f.params.len()..].iter().any(|a| a.is_some()) {
            self.warn(&format!("@{}: too many arguments", f.name));
        }
        let r = self.exec();
        self.frames.pop();
        if prof::ON.load(std::sync::atomic::Ordering::Relaxed) {
            match self.frames.last() {
                Some(fr) => prof::enter(&fr.func),
                None => prof::CUR.store(0, std::sync::atomic::Ordering::Relaxed),
            }
        }
        if let (Some(t), Ok(v)) = (&self.trace, &r) {
            if f.name.contains(t.as_str()) {
                eprintln!("trace {:8.3}s: {}<- @{} = {:?}", trace_clock(), "  ".repeat(self.frames.len()), f.name, v);
            }
        }
        r
    }

    /// Keep compiled function bodies within `code_budget`: drop those
    /// called longest ago (not running now); they're compiled again from
    /// their files when next called. Big games run a few huge functions
    /// once (pops-tw fills its name lists with 170000 lines at the title).
    fn trim_code(&mut self) {
        if self.code_bytes <= self.code_budget {
            return;
        }
        use std::sync::atomic::Ordering::Relaxed;
        let mut lru: Vec<(u64, usize, Rc<Function>)> = Vec::new();
        for f in self.prog.funcs.values().flatten() {
            if f.lazy.is_none() || self.frames.iter().any(|fr| Rc::ptr_eq(&fr.func, f)) {
                continue;
            }
            if let Some(b) = f.body.get() {
                let used = b.used.load(Relaxed);
                if used != 0 {
                    lru.push((used, b.approx_bytes(), f.clone()));
                }
            }
        }
        lru.sort_unstable_by_key(|e| e.0);
        for (_, bytes, f) in lru {
            if self.code_bytes <= self.code_budget * 3 / 4 {
                break;
            }
            // SAFETY: the program runs on this thread alone
            if unsafe { f.body.clear() } {
                self.code_bytes = self.code_bytes.saturating_sub(bytes);
            }
        }
    }

    /// Prefix an error with "file:line: @FUNC:" the first time it passes
    /// through a frame, so messages point at the failing ERB line.
    fn located(&self, e: Ctrl, f: &Function, body: &Body, pc: usize) -> Ctrl {
        match e {
            Ctrl::Error(m, false) => {
                let (file, line) = body.lines.get(pc).copied().unwrap_or((f.file, f.line));
                let path = self.prog.files.get(file as usize).cloned().unwrap_or_default();
                Ctrl::Error(format!("{}:{}: @{}: {}", path, line, f.name, m), true)
            }
            e => e,
        }
    }

    /// The instruction loop of the innermost frame.
    fn exec(&mut self) -> VmResult<Option<Value>> {
        // a frame's function never changes while it runs: take it once
        let f = self.frames.last().unwrap().func.clone();
        let body = self.frames.last().unwrap().body.clone();
        loop {
            let pc = self.frames.last().unwrap().pc;
            if pc >= body.code.len() {
                return Ok(None);
            }
            self.frames.last_mut().unwrap().pc = pc + 1;
            self.ticks = self.ticks.wrapping_add(1);
            if self.ticks & 0xFFF == 0 {
                self.tick()?;
            }
            match self.step(&body, pc) {
                Ok(None) => {}
                Ok(Some(ret)) => return Ok(ret),
                Err(e) => return Err(self.located(e, &f, &body, pc)),
            }
        }
    }

    fn set_pc(&mut self, pc: usize) {
        self.frames.last_mut().unwrap().pc = pc;
    }

    /// The function / label name of a CALL/GOTO target.
    fn resolve_target(&mut self, t: &CallTarget) -> VmResult<String> {
        Ok(match t {
            CallTarget::Static(s) => s.clone(),
            CallTarget::Form(f) => self.eval_form(f)?.trim().to_uppercase(),
        })
    }

    /// Execute one instruction. Ok(Some(x)) means the function returns x.
    fn step(&mut self, body: &Body, pc: usize) -> VmResult<Option<Option<Value>>> {
        match &body.code[pc] {
            Instr::BadLine(msg, fatal) => {
                if *fatal || self.strict {
                    return err(format!("line failed to parse: {}", msg));
                }
                self.warn(&format!("skipped unparsed line: {}", msg));
            }
            Instr::Print(content, fl) => {
                let s = match content {
                    PrintContent::Raw(s) => s.clone(),
                    // (Emuera-Anchor: printed names are translated)
                    PrintContent::Form(fm) if !self.csv.tr.is_empty() => self.eval_form_tr(fm)?,
                    PrintContent::Form(fm) => self.eval_form(fm)?,
                    PrintContent::Expr(e) => {
                        let s = self.eval_str(e)?;
                        self.translated(e, s)
                    }
                    PrintContent::Exprs(es) => {
                        let mut s = String::new();
                        for e in es {
                            s.push_str(&self.eval(e)?.into_string());
                        }
                        s
                    }
                };
                self.do_print(&s, *fl)?;
            }
            Instr::PrintData(entries, fl) => {
                if !entries.is_empty() {
                    let k = self.rand(entries.len() as i64) as usize;
                    let lines = entries[k].clone();
                    let mut first = true;
                    let mut text = String::new();
                    for l in &lines {
                        if !first {
                            text.push('\n');
                        }
                        first = false;
                        if self.csv.tr.is_empty() {
                            text.push_str(&self.eval_form(l)?);
                        } else {
                            self.eval_form_into(l, &mut text, true)?;
                        }
                    }
                    self.do_print(&text, *fl)?;
                }
            }
            Instr::StrData(var, entries) => {
                if !entries.is_empty() {
                    let k = self.rand(entries.len() as i64) as usize;
                    let mut text = String::new();
                    for (i, l) in entries[k].iter().enumerate() {
                        if i > 0 {
                            text.push('\n');
                        }
                        text.push_str(&self.eval_form(l)?);
                    }
                    match var {
                        Some(v) => self.set_var(v, Value::Str(text))?,
                        None => self.set_results(text),
                    }
                }
            }
            Instr::Assign(v, op, rhs) => self.assign(v, *op, rhs)?,
            Instr::AssignForm(v, op, form) => {
                let s = self.eval_form(form)?;
                let p = self.locate(v)?;
                let new = match op {
                    AsgOp::Add => self.get_place(&p).into_string() + &s,
                    _ => s,
                };
                self.store(v, &p, Value::Str(new))?;
            }
            Instr::Eval(e) => {
                self.eval(e)?;
            }
            Instr::Jump(t) => self.set_pc(*t),
            Instr::JumpIfNot(c, t) => {
                if !self.eval(c)?.truthy() {
                    self.set_pc(*t);
                }
            }
            Instr::ForBegin(fb) => {
                let ForBegin { var, start, end, step, slot, exit } = &**fb;
                let s = self.eval_int(start)?;
                let e = self.eval_int(end)?;
                let st = match step {
                    Some(x) => self.eval_int(x)?,
                    None => 1,
                };
                self.set_var(var, Value::Int(s))?;
                self.frames.last_mut().unwrap().slots[*slot] = Slot::For(e, st);
                if !for_cond(s, e, st) {
                    self.set_pc(*exit);
                }
            }
            Instr::ForNext { var, slot, body } => {
                let (e, st) = match self.frames.last().unwrap().slots[*slot] {
                    Slot::For(e, st) => (e, st),
                    _ => (0, 1),
                };
                let p = self.locate(var)?;
                let v = self.get_place(&p).as_int() + st;
                self.set_place(&p, Value::Int(v))?;
                if for_cond(v, e, st) {
                    self.set_pc(*body);
                }
            }
            Instr::SelectEval(e, slot, table) => {
                let v = self.eval(e)?;
                // literal CASEs: straight to the matching one (same result as
                // testing them in order: the first match wins)
                let at = self.frames.last().unwrap().pc.saturating_sub(1);
                let jump = table
                    .get_or_init(|| {
                        let mut lit = |e: &Expr| -> Option<Value> {
                            match e {
                                Expr::Str(s) => Some(Value::Str(s.clone())),
                                Expr::Int(i) => Some(Value::Int(*i)),
                                // a CONST scalar: fixed for the program's life
                                Expr::Var(v) if v.idx.is_empty() && self.is_const_scalar(&v.name) => self.get_var(v).ok(),
                                _ => None,
                            }
                        };
                        CaseTable::build(&body.code, at, *slot, &mut lit)
                    })
                    .as_ref()
                    .and_then(|t| match &v {
                        Value::Str(s) if t.ints.is_empty() => Some(t.strs.get(s.as_str()).copied().unwrap_or(t.default)),
                        Value::Int(i) if t.strs.is_empty() => Some(t.ints.get(i).copied().unwrap_or(t.default)),
                        _ => None,
                    });
                self.frames.last_mut().unwrap().slots[*slot] = Slot::Sel(v);
                if let Some(pc) = jump {
                    self.set_pc(pc);
                }
            }
            Instr::CaseTest(slot, conds, next) => {
                // the selector is taken out of its slot while the CASE
                // values are evaluated (no copy of a string selector)
                let sel = std::mem::replace(&mut self.frames.last_mut().unwrap().slots[*slot], Slot::None);
                let v = match &sel {
                    Slot::Sel(v) => v,
                    _ => &Value::Int(0),
                };
                let mut hit = Ok(false);
                for c in conds {
                    match self.case_match(v, c) {
                        Ok(false) => {}
                        r => {
                            hit = r;
                            break;
                        }
                    }
                }
                // (a CASE value that called a function may have left frames)
                if let Some(f) = self.frames.last_mut() {
                    if let Some(s) = f.slots.get_mut(*slot) {
                        *s = sel;
                    }
                }
                if !hit? {
                    self.set_pc(*next);
                }
            }
            Instr::Call(ci) => {
                let CallInstr { target, args, kind, try_, catch } = &**ci;
                let formed;
                let name: &str = match target {
                    CallTarget::Static(s) => s,
                    CallTarget::Form(_) => {
                        formed = self.resolve_target(target)?;
                        &formed
                    }
                };
                // one lookup: the function (the first one for event names)
                let Some((f, single)) = self.prog.funcs.get(name).map(|fs| (fs[0].clone(), fs.len() == 1)) else {
                    if *try_ {
                        if let Some(c) = catch {
                            self.set_pc(*c);
                        }
                        return Ok(None);
                    }
                    return err(format!("function @{} not found", name));
                };
                let (vals, refs) = self.call_args(&f, args)?;
                self.pending_refs = refs;
                if single {
                    self.call_func(f, vals)?;
                } else {
                    self.call_name(name, vals)?;
                }
                if *kind == CallKind::Jump {
                    return Ok(Some(None));
                }
            }
            Instr::CallList(entries, kind) => {
                for (t, args) in entries {
                    let name = self.resolve_target(t)?;
                    if self.has_func(&name) {
                        let mut vals = Vec::new();
                        for a in args {
                            vals.push(Some(self.eval(a)?));
                        }
                        self.call_name(&name, vals)?;
                        if *kind == CallKind::Jump {
                            return Ok(Some(None));
                        }
                        break;
                    }
                }
            }
            Instr::Goto(g) => {
                let (target, try_, catch) = (&g.0, &g.1, &g.2);
                let label = self.resolve_target(target)?;
                match body.labels.get(&label) {
                    Some(&t) => self.set_pc(t),
                    None => {
                        if *try_ {
                            if let Some(c) = catch {
                                self.set_pc(*c);
                            }
                        } else {
                            return err(format!("label ${} not found", label));
                        }
                    }
                }
            }
            Instr::GotoList(ts) => {
                for t in ts {
                    let label = self.resolve_target(t)?;
                    if let Some(&p) = body.labels.get(&label) {
                        self.set_pc(p);
                        break;
                    }
                }
            }
            Instr::Return(vals) => {
                let mut ri = 0;
                let mut si = 0;
                if vals.is_empty() {
                    self.set_result(0);
                }
                for e in vals {
                    match self.eval(e)? {
                        Value::Int(i) => {
                            self.set_global_i("RESULT", ri, i);
                            ri += 1;
                        }
                        Value::Str(s) => {
                            self.global_arr("RESULTS").set(si, Value::Str(s));
                            si += 1;
                        }
                    }
                }
                return Ok(Some(None));
            }
            Instr::ReturnF(v) => {
                let v = match v {
                    Some(e) => Some(self.eval(e)?),
                    None => None,
                };
                return Ok(Some(v));
            }
            Instr::Begin(s) => return Err(Ctrl::Begin(s.clone())),
            Instr::Restart => self.set_pc(0),
            Instr::Quit => return Err(Ctrl::Quit),
            Instr::Throw(fm) => {
                let s = self.eval_form(fm)?;
                return err(format!("THROW: {}", s));
            }
            Instr::Command(c) => self.command(&c.0, &c.1)?,
        }
        Ok(None)
    }

    /// Does the SELECTCASE value match one CASE condition? Strings compare
    /// as text, everything else as integers.
    fn case_match(&mut self, v: &Value, c: &CaseCond) -> VmResult<bool> {
        Ok(match c {
            CaseCond::Eq(e) => {
                // literal CASEs compare in place
                match (v, e) {
                    (Value::Str(s), Expr::Str(lit)) => return Ok(s == lit),
                    (Value::Int(i), Expr::Int(n)) => return Ok(i == n),
                    _ => {}
                }
                let x = self.eval(e)?;
                match (v, &x) {
                    (Value::Int(a), Value::Int(b)) => a == b,
                    (Value::Str(a), Value::Str(b)) => a == b,
                    _ => v.as_string() == x.as_string(),
                }
            }
            CaseCond::Range(a, b) => {
                let a = self.eval(a)?;
                let b = self.eval(b)?;
                if v.is_str() {
                    let s = v.as_string();
                    s >= a.as_string() && s <= b.as_string()
                } else {
                    let x = v.as_int();
                    x >= a.as_int() && x <= b.as_int()
                }
            }
            CaseCond::Is(op, e) => {
                let x = self.eval(e)?;
                if v.is_str() {
                    let (a, b) = (v.as_string(), x.as_string());
                    match op {
                        BinOp::Lt => a < b,
                        BinOp::Le => a <= b,
                        BinOp::Gt => a > b,
                        BinOp::Ge => a >= b,
                        BinOp::Eq => a == b,
                        _ => a != b,
                    }
                } else {
                    let (a, b) = (v.as_int(), x.as_int());
                    match op {
                        BinOp::Lt => a < b,
                        BinOp::Le => a <= b,
                        BinOp::Gt => a > b,
                        BinOp::Ge => a >= b,
                        BinOp::Eq => a == b,
                        BinOp::BitAnd => a & b != 0,
                        _ => a != b,
                    }
                }
            }
        })
    }

    /// `v op rhs`. Several values (`A = 1, 2, 3`) fill consecutive
    /// elements starting at `v`'s index; compound operators take one.
    fn assign(&mut self, v: &VarRef, op: AsgOp, rhs: &[Expr]) -> VmResult<()> {
        let p = self.locate(v)?;
        if rhs.len() > 1 {
            if !matches!(op, AsgOp::Set | AsgOp::SetExpr) {
                return err("compound assignment cannot take multiple values");
            }
            for (k, e) in rhs.iter().enumerate() {
                let val = self.eval(e)?;
                let q = Place { loc: p.loc.clone(), flat: p.flat + k };
                self.set_place(&q, val)?;
            }
            return Ok(());
        }
        let val = self.eval(&rhs[0])?;
        let new = match op {
            AsgOp::Set | AsgOp::SetExpr => val,
            _ => {
                let old = self.get_place(&p);
                if old.is_str() {
                    match op {
                        AsgOp::Add => Value::Str(old.into_string() + &val.into_string()),
                        AsgOp::Mul => Value::Str(old.into_string().repeat(val.as_int().max(0) as usize)),
                        _ => return err("invalid compound operator for string"),
                    }
                } else {
                    let (a, b) = (old.as_int(), val.as_int());
                    Value::Int(match op {
                        AsgOp::Add => a.wrapping_add(b),
                        AsgOp::Sub => a.wrapping_sub(b),
                        AsgOp::Mul => a.wrapping_mul(b),
                        AsgOp::Div => {
                            if b == 0 {
                                return err("division by zero");
                            }
                            a / b
                        }
                        AsgOp::Mod => {
                            if b == 0 {
                                return err("modulo by zero");
                            }
                            a % b
                        }
                        AsgOp::And => a & b,
                        AsgOp::Or => a | b,
                        AsgOp::Xor => a ^ b,
                        AsgOp::Shl => a.wrapping_shl(b as u32),
                        AsgOp::Shr => a.wrapping_shr(b as u32),
                        _ => unreachable!(),
                    })
                }
            }
        };
        self.store(v, &p, new)
    }

    /// Write an assignment result; read-only places go through `set_var`,
    /// which accepts the writable pseudo variables (WINDOW_TITLE, …).
    fn store(&mut self, v: &VarRef, p: &Place, val: Value) -> VmResult<()> {
        if matches!(p.loc, Loc::Ro(_)) {
            return self.set_var(v, val);
        }
        self.set_place(p, val)
    }

    // ---------------------------------------------------------------- output
    /// Output text for a PRINT-family command: colours (D ignores
    /// SETCOLOR), PRINTC column padding, then newline / WAIT per the flags.
    pub fn do_print(&mut self, s: &str, fl: PrintFlags) -> VmResult<()> {
        let saved = if fl.default_color { Some(self.con.style.clone()) } else { None };
        if fl.default_color {
            self.con.reset_style();
        }
        // Emuera pads right-aligned items to PRINTCLENGTH and left-aligned
        // ones (PRINTLC) to one more
        let text = match fl.column {
            Some(Align::Left) => pad(s, self.con.printc_len + 1, Align::Left),
            Some(a) => pad(s, self.con.printc_len, a),
            None => s.to_string(),
        };
        if fl.single {
            // PRINTSINGLE: truncate to the console width, no wrapping
            let mut w = 0;
            let t: String = text
                .chars()
                .take_while(|c| {
                    w += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(1);
                    w <= self.con.width
                })
                .collect();
            self.con.print_ex(&t, !fl.plain);
            self.con.newline();
        } else {
            match fl.column {
                Some(_) if !fl.plain && !text.contains('\n') => self.con.print_unit(&text),
                _ => self.con.print_ex(&text, !fl.plain),
            }
            if fl.newline {
                self.con.newline();
            }
        }
        if let Some(st) = saved {
            self.con.style = st;
        }
        if fl.wait {
            self.wait()?;
        }
        Ok(())
    }

    // ----------------------------------------------------------------- input
    /// Ask the frontend for input; None means a timeout. Quitting the
    /// frontend becomes `Ctrl::Quit`.
    pub fn raw_input(&mut self, req: &InputReq) -> VmResult<Option<String>> {
        // SKIPDISP skips the WAIT family along with the prints (Emuera's
        // IS_PRINT flag), and a script INPUT under it is an error
        if self.con.skip_disp {
            if req.kind == InputKind::Wait {
                return Ok(Some(String::new()));
            }
            if !self.con.skip_user {
                // the engine's own skipping: the input is passed over
                return Ok(None);
            }
            if !self.frames.is_empty() {
                self.con.skip_disp = false;
                return err("input was requested while SKIPDISP is on");
            }
        }
        // message skip (a right click, SKIPLOG 1): the WAIT family passes
        // by itself until an input needs a value or a FORCEWAIT, as in Emuera
        if self.mes_skip {
            if req.kind == InputKind::Wait && !req.stop_skip {
                return Ok(Some(String::new()));
            }
            self.mes_skip = false;
        }
        // debugging aid (see main.rs): run @__INJECT at input number N
        self.inputs_seen += 1;
        if let Some(n) = self.inject_at {
            if self.inputs_seen == n {
                self.inject_at = None;
                self.call_name("__INJECT", Vec::new())?;
            }
        }
        // keep the unpacked graphics buffers and compiled bodies within
        // their memory budgets
        self.res.trim_graphics();
        self.trim_code();
        // EMUERA_MEMSTATS=<file>: what the memory holds, at every input
        if let Some(f) = &self.memstats {
            let _ = std::fs::write(f, self.mem_report());
        }
        let _wait = prof::Waiting::new();
        if prof::ON.load(std::sync::atomic::Ordering::Relaxed) {
            let st: Vec<String> = self.frames.iter().map(|f| format!("{}:{}", f.func.name, f.body.lines.get(f.pc.saturating_sub(1)).map_or(0, |l| l.1))).collect();
            prof::set_waiting_at(format!("{:?} at {}", req.kind, st.join(" > ")));
        }
        self.con.width = self.fe.cols().saturating_sub(1).max(20);
        // Emuera flushes an unfinished line when the script stops for input
        // (RunEmueraProgram → PrintFlush), so later output starts afresh.
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
        if req.kind != InputKind::Wait {
            let site = self.frames.last().map_or((0, 0), |f| (Rc::as_ptr(&f.func) as usize, f.pc));
            self.con.begin_input(site, req.kind == InputKind::Int);
        }
        let r = self.fe.input(&mut self.con, &mut self.res, req);
        match r {
            InputResult::Text(s) => {
                self.last_input = s.clone();
                Ok(Some(s))
            }
            InputResult::Skip(s) => {
                self.mes_skip = true;
                self.last_input = s.clone();
                Ok(Some(s))
            }
            InputResult::Timeout => Ok(None),
            InputResult::Quit => Err(Ctrl::Quit),
            InputResult::Hotkey(h) => {
                self.hotkey(h)?;
                self.raw_input(req)
            }
        }
    }

    /// WAIT: show everything, continue on any key / click.
    pub fn wait(&mut self) -> VmResult<()> {
        self.wait_ex(false)
    }

    /// WAIT, or FORCEWAIT (`force`: message skipping stops here).
    pub fn wait_ex(&mut self, force: bool) -> VmResult<()> {
        self.comend_wait = false;
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
        self.raw_input(&InputReq { kind: InputKind::Wait, one: true, timeout_ms: None, mouse: false, stop_skip: force })?;
        Ok(())
    }

    /// INPUT-style integer input: re-asks on non-numbers, empty input gives
    /// the default; full-width digits are accepted. Callers store RESULT.
    pub fn input_int(&mut self, default: Option<i64>, timeout: Option<u64>, one: bool) -> VmResult<i64> {
        loop {
            let r = self.raw_input(&InputReq { kind: InputKind::Int, one, timeout_ms: timeout, mouse: false, stop_skip: false })?;
            self.is_timeout = r.is_none();
            let s = match r {
                None => return Ok(default.unwrap_or(0)),
                Some(s) => s,
            };
            let t = s.trim();
            if t.is_empty() {
                if let Some(d) = default {
                    return Ok(d);
                }
                continue;
            }
            if let Ok(v) = t.parse::<i64>() {
                return Ok(v);
            }
            // full-width digits
            let half: String = t
                .chars()
                .map(|c| if ('０'..='９').contains(&c) { char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap() } else { c })
                .collect();
            if let Ok(v) = half.parse::<i64>() {
                return Ok(v);
            }
        }
    }

    /// INPUTS-style string input; empty input gives the default. Callers
    /// store RESULTS.
    pub fn input_str(&mut self, default: Option<String>, timeout: Option<u64>, one: bool) -> VmResult<String> {
        let r = self.raw_input(&InputReq { kind: InputKind::Str, one, timeout_ms: timeout, mouse: false, stop_skip: false })?;
        self.is_timeout = r.is_none();
        Ok(match r {
            None => default.unwrap_or_default(),
            Some(s) if s.is_empty() => default.unwrap_or_default(),
            Some(s) => s,
        })
    }

    /// Every few thousand instructions: while a script computes for a long
    /// time, keep the screen up to date (as Emuera does) and let Ctrl+C
    /// quit, so a long or endless loop never looks like a dead program.
    fn tick(&mut self) -> VmResult<()> {
        if self.last_draw.elapsed() < std::time::Duration::from_millis(150) {
            return Ok(());
        }
        self.last_draw = std::time::Instant::now();
        if self.fe.poll_interrupt() {
            return Err(Ctrl::Quit);
        }
        if self.con.dirty && self.con.redraw && self.fe.live_updates() {
            self.flush();
        }
        Ok(())
    }

    /// Push pending output to the frontend without waiting.
    pub fn flush(&mut self) {
        self.con.width = self.fe.cols().saturating_sub(1).max(20);
        self.fe.flush(&mut self.con, &mut self.res);
    }
}


/// Is a FOR loop at `v` still running? (counts down for negative steps)
fn for_cond(v: i64, end: i64, step: i64) -> bool {
    if step >= 0 { v < end } else { v > end }
}

/// Variables computed on read (CHARANUM, RAND, GAMEBASE_*, *NAME…) that
/// have no storage of their own.
pub fn pseudo_var(name: &str) -> bool {
    matches!(
        name,
        "RAND" | "CHARANUM" | "LINECOUNT" | "ISTIMEOUT" | "__INT_MAX__" | "__INT_MIN__" | "WINDOW_TITLE" | "DRAWLINESTR"
            | "MONEYLABEL" | "SAVEDATA_TEXT" | "LASTLOAD_TEXT" | "LASTLOAD_NO" | "LASTLOAD_VERSION" | "GAMEBASE_CODE"
            | "GAMEBASE_VERSION" | "GAMEBASE_ALLOWVERSION" | "GAMEBASE_DEFAULTCHARA" | "GAMEBASE_NOITEM" | "GAMEBASE_AUTHOR"
            | "GAMEBASE_AUTHER" | "GAMEBASE_YEAR" | "GAMEBASE_TITLE" | "GAMEBASE_INFO" | "CDFLAGNAME1" | "CDFLAGNAME2"
    ) || (name.ends_with("NAME") && table_for_var(name).is_some() && name != "NAME")
}

/// Pad to `w` half-width columns (Shift_JIS byte width). Right-align by default.
pub fn pad(s: &str, w: usize, a: Align) -> String {
    let len = sjis_len(s);
    if len >= w {
        return s.to_string();
    }
    let sp = " ".repeat(w - len);
    match a {
        Align::Left => format!("{}{}", s, sp),
        _ => format!("{}{}", sp, s),
    }
}

/// Seconds since the first trace line (EMUERA_TRACE timing).
fn trace_clock() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// Which CSV name table indexes a variable.
#[derive(Clone, Copy, Debug)]
pub enum Tbl {
    None,
    Static(&'static str),
    /// the variable's own name (a table named like an ERH variable)
    Own,
}

/// What a variable name means whatever function uses it (cached in the
/// `VarRef`, packed in a u64; `Vm::var_info` adds the function's own #DIMs).
#[derive(Clone, Copy)]
pub struct NameInfo {
    table: Tbl,
    /// LOCAL 1, LOCALS 2, ARG 3, ARGS 4, else 0
    local_family: u8,
    user: Option<(bool, usize)>,
    csv_name: bool,
    pseudo: bool,
    system: Option<&'static VarDef>,
}

/// What a variable name means inside a function (see `Vm::var_info`).
#[derive(Clone, Copy)]
pub struct VarInfo {
    table: Tbl,
    private: bool,
    is_static: bool,
    /// index in the function's `static_cache`
    skey: u32,
    /// user `#DIM`: (CHARADATA, dimensions)
    user: Option<(bool, usize)>,
    csv_name: bool,
    pseudo: bool,
    system: Option<&'static VarDef>,
}

impl NameInfo {
    /// bits 0-5 table (0 none, 1.. TABLE_NAMES + 1, 63 own), 6-8 LOCAL
    /// family, 9 user #DIM, 10 CHARADATA, 11-12 its dimensions, 13 CSV
    /// name array, 14 pseudo, 16-31 system variable + 1, 63 set
    fn pack(&self) -> u64 {
        let t: u64 = match self.table {
            Tbl::None => 0,
            Tbl::Own => 63,
            Tbl::Static(t) => crate::csvdata::TABLE_NAMES.iter().position(|x| *x == t).map_or(0, |i| i as u64 + 1),
        };
        let mut p = t | (self.local_family as u64) << 6;
        if let Some((chara, dims)) = self.user {
            p |= 1 << 9 | (chara as u64) << 10 | (dims.min(3) as u64) << 11;
        }
        p |= (self.csv_name as u64) << 13 | (self.pseudo as u64) << 14;
        if let Some(d) = self.system {
            let i = (d as *const VarDef as usize - vars::SYSTEM_VARS.as_ptr() as usize) / std::mem::size_of::<VarDef>();
            p |= (i as u64 + 1) << 16;
        }
        p | 1 << 63
    }

    fn unpack(p: u64) -> NameInfo {
        let table = match p & 63 {
            0 => Tbl::None,
            63 => Tbl::Own,
            i => Tbl::Static(crate::csvdata::TABLE_NAMES[i as usize - 1]),
        };
        let sys = (p >> 16) & 0xFFFF;
        NameInfo {
            table,
            local_family: ((p >> 6) & 7) as u8,
            user: (p & 1 << 9 != 0).then(|| (p & 1 << 10 != 0, ((p >> 11) & 3) as usize)),
            csv_name: p & 1 << 13 != 0,
            pseudo: p & 1 << 14 != 0,
            system: (sys != 0).then(|| &vars::SYSTEM_VARS[sys as usize - 1]),
        }
    }
}

impl std::fmt::Debug for NameInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("NameInfo")
    }
}

impl std::fmt::Debug for VarInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("VarInfo")
    }
}

/// `EMUERA_PROFILE=<file>`: which ERB functions the time goes to. A thread
/// samples the running function every millisecond and rewrites `<file>`
/// with the counts every two seconds (so it survives the game being
/// killed). Off, it costs one relaxed load per call.
pub mod prof {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    pub static ON: AtomicBool = AtomicBool::new(false);
    /// the running function, as the address of its `Function`
    pub static CUR: AtomicUsize = AtomicUsize::new(0);
    static NAMES: Mutex<Option<HashMap<usize, String>>> = Mutex::new(None);

    /// waiting for the player: not counted
    pub static WAIT: AtomicBool = AtomicBool::new(false);
    /// the call stack of the last input wait
    static WAIT_AT: Mutex<String> = Mutex::new(String::new());

    pub fn set_waiting_at(s: String) {
        *WAIT_AT.lock().unwrap() = s;
    }

    /// Marks the time until it is dropped as waiting for input.
    pub struct Waiting;
    impl Waiting {
        pub fn new() -> Waiting {
            WAIT.store(true, Ordering::Relaxed);
            Waiting
        }
    }
    impl Drop for Waiting {
        fn drop(&mut self) {
            WAIT.store(false, Ordering::Relaxed);
        }
    }

    pub fn enter(f: &std::sync::Arc<crate::ast::Function>) {
        let id = std::sync::Arc::as_ptr(f) as usize;
        if CUR.swap(id, Ordering::Relaxed) != id {
            let mut n = NAMES.lock().unwrap();
            n.get_or_insert_with(HashMap::new).entry(id).or_insert_with(|| f.name.to_string());
        }
    }

    pub fn start(path: String) {
        ON.store(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            let mut counts: HashMap<usize, u64> = HashMap::new();
            let mut total = 0u64;
            let mut last = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(1));
                let c = CUR.load(Ordering::Relaxed);
                if c != 0 && !WAIT.load(Ordering::Relaxed) {
                    *counts.entry(c).or_default() += 1;
                    total += 1;
                }
                if last.elapsed().as_secs() >= 2 {
                    last = std::time::Instant::now();
                    let names = NAMES.lock().unwrap().clone().unwrap_or_default();
                    let mut v: Vec<(&usize, &u64)> = counts.iter().collect();
                    v.sort_by(|a, b| b.1.cmp(a.1));
                    let mut out = format!("{} input: {}\n", if WAIT.load(Ordering::Relaxed) { "waiting for" } else { "last" }, WAIT_AT.lock().unwrap());
                    out += &format!("{} samples (1 ms each), time spent in each function itself:\n", total);
                    for (id, n) in v.into_iter().take(60) {
                        out += &format!("{:6.1}% {:8} {}\n", *n as f64 * 100.0 / total.max(1) as f64, n, names.get(id).map(String::as_str).unwrap_or("?"));
                    }
                    let _ = std::fs::write(&path, out);
                }
            }
        });
    }
}

#[cfg(test)]
mod nameinfo_tests {
    use super::*;

    #[test]
    fn pack_roundtrip() {
        for t in crate::csvdata::TABLE_NAMES {
            for (local, user) in [(0u8, None), (3, Some((true, 2))), (4, Some((false, 3)))] {
                let n = NameInfo { table: Tbl::Static(t), local_family: local, user, csv_name: true, pseudo: false, system: system_var("CFLAG") };
                let u = NameInfo::unpack(n.pack());
                assert!(matches!(u.table, Tbl::Static(x) if x == *t));
                assert_eq!((u.local_family, u.user, u.csv_name, u.pseudo), (local, user, true, false));
                assert!(std::ptr::eq(u.system.unwrap(), system_var("CFLAG").unwrap()));
            }
        }
        // every name table_for_var can give is in TABLE_NAMES
        for v in ["ABL", "PALAMNAME", "ITEMPRICE", "TRAINNAME", "STR", "GLOBALSNAME", "TIMENAME", "CDOWN"] {
            assert!(crate::csvdata::TABLE_NAMES.contains(&crate::csvdata::table_for_var(v).unwrap()), "{}", v);
        }
        for d in vars::SYSTEM_VARS {
            let n = NameInfo { table: Tbl::Own, local_family: 0, user: None, csv_name: false, pseudo: true, system: Some(d) };
            let u = NameInfo::unpack(n.pack());
            assert!(std::ptr::eq(u.system.unwrap(), d) && matches!(u.table, Tbl::Own) && u.pseudo);
        }
    }
}
