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
pub mod save;
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

/// One active function call.
pub struct Frame {
    pub func: Rc<Function>,
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
    Global(Name),
    /// `#DIM DYNAMIC` in the current frame
    Frame(Name),
    /// a variable in a specific (caller's) frame, for REF parameters
    FrameAt(usize, Name),
    /// function-owned storage: `statics[function name][variable]`
    Static(Rc<Function>, Name),
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
    pub globals: HashMap<String, Arr>,
    pub statics: HashMap<String, HashMap<String, Arr>>,
    pub charas: Vec<Chara>,
    /// The call stack, innermost last.
    pub frames: Vec<Frame>,
    /// ERH `#DIM` declarations by name (sizes / flags for lookups).
    pub user_dims: HashMap<String, DimDecl>,
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
    rng: u64,
    /// Warnings already printed (each is shown once).
    warned: HashSet<String>,
    pub game_dir: PathBuf,
    /// `--strict`: unimplemented commands are errors, not warnings.
    pub strict: bool,
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
            globals: HashMap::default(),
            statics: HashMap::default(),
            charas: Vec::new(),
            frames: Vec::new(),
            user_dims: HashMap::default(),
            maps: HashMap::default(),
            dts: HashMap::default(),
            xml_docs: HashMap::default(),
            text_exts: vec!["txt".into()],
            save_opts: save::SaveOpts::default(),
            varext: save::VarExt::load(crate::loader::find_dir_ci(&game_dir, "CSV").as_deref()),
            pending_refs: Vec::new(),
            trace: std::env::var("EMUERA_TRACE").ok().map(|t| t.to_uppercase()),
            rng: seed | 1,
            warned: HashSet::default(),
            game_dir,
            strict: false,
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
        self.statics.clear();
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
    pub fn global_arr(&mut self, name: &str) -> &mut Arr {
        if !self.globals.contains_key(name) {
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
        if !self.charas[c].vars.contains_key(name) {
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
        f.func.dims.iter().find(|d| d.name == name)
    }

    /// Is `name` resolvable as a variable here (used for CSV-name indices)?
    pub fn var_exists(&self, name: &str) -> bool {
        if let Some(f) = self.frames.last() {
            if f.vars.contains_key(name) || f.func.dims.iter().any(|d| d.name == name) {
                return true;
            }
        }
        matches!(name, "LOCAL" | "LOCALS" | "ARG" | "ARGS")
            || self.user_dims.contains_key(name)
            || system_var(name).is_some()
            || pseudo_var(name)
    }

    /// One index value. A bare identifier that isn't a variable, or a
    /// string, is a CSV name looked up in `table` (or a table named after
    /// the variable, for ERD files).
    fn eval_index(&mut self, e: &Expr, table: Option<&str>, var: &str) -> VmResult<i64> {
        match e {
            Expr::Var(v) if v.idx.is_empty() && !self.var_exists(&v.name) => {
                if let Some(t) = table {
                    if let Some(i) = self.csv.lookup(t, &v.name) {
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
    fn eval_indices(&mut self, v: &VarRef, from: usize, table: Option<&str>) -> VmResult<Vec<i64>> {
        let mut out = Vec::with_capacity(v.idx.len());
        let n = v.idx.len();
        for (k, e) in v.idx.iter().enumerate().skip(from) {
            // CSV names apply to the last index (and to every index of ERD vars)
            let t = if k + 1 == n { table } else { None };
            out.push(self.eval_index(e, t, &v.name)?);
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
    pub fn locate(&mut self, v: &VarRef) -> VmResult<Place> {
        let name = &*v.name;
        let table: Option<&str> = table_for_var(name).or_else(|| self.csv.tables.contains_key(name).then_some(name));
        // 0. REF parameter aliases
        if let Some(loc) = self.frames.last().and_then(|f| f.refs.get(name)).cloned() {
            let idx = self.eval_indices(v, 0, table)?;
            let a = self.arr_at(&loc).ok_or_else(|| Ctrl::Error(format!("{}: referenced variable is gone", name), false))?;
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc, flat });
        }
        // 1. frame-local
        let (is_private, is_static) = match self.frames.last() {
            Some(f) => match f.func.dims.iter().find(|d| d.name == name) {
                Some(d) => (true, d.is_static),
                // Emuera's LOCAL/ARG family is per-function storage that
                // persists across calls (reset by RESETDATA / load)
                None => (false, matches!(name, "LOCAL" | "LOCALS" | "ARG" | "ARGS")),
            },
            None => (false, false),
        };
        if is_static {
            let func = self.frames.last().unwrap().func.clone();
            if !self.statics.get(&func.name).is_some_and(|m| m.contains_key(name)) {
                let a = match self.frame_private(name).cloned() {
                    Some(d) => self.make_dim(&d)?,
                    None if matches!(name, "LOCAL" | "ARG") => Arr::new(false, &[1000]),
                    None => Arr::new(true, &[100]),
                };
                self.statics.entry(func.name.clone()).or_default().insert(name.to_string(), a);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let flat = self.statics[&func.name][name].flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Static(func, v.name.clone()), flat });
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
        if let Some((is_chara, nd)) = self.user_dims.get(name).map(|d| (d.is_chara, d.dims.len())) {
            if is_chara {
                return self.locate_chara(v, nd, table);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let a = self.globals.get(name).ok_or_else(|| Ctrl::Error(format!("{} not allocated", name), false))?;
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Global(v.name.clone()), flat });
        }
        // 3a. CSV name tables (ABLNAME, BASENAME, …) as real string arrays,
        // so FINDELEMENT / VARSIZE / array builtins can take them
        if name.ends_with("NAME") && name != "NAME" {
            if let Some(t) = table_for_var(name) {
                if !self.globals.contains_key(name) {
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
                let flat = self.globals[name].flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
                return Ok(Place { loc: Loc::Global(v.name.clone()), flat });
            }
        }
        // 3. pseudo variables
        if pseudo_var(name) {
            let val = self.pseudo_value(v, table)?;
            return Ok(Place { loc: Loc::Ro(val), flat: 0 });
        }
        // 4. system variables
        if let Some(def) = system_var(name) {
            if def.chara {
                return self.locate_chara(v, def.dims.len(), table);
            }
            let idx = self.eval_indices(v, 0, table)?;
            let a = self.global_arr(name);
            let flat = a.flat(&idx).map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
            return Ok(Place { loc: Loc::Global(v.name.clone()), flat });
        }
        err(format!("undefined variable '{}'", name))
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
            Loc::Global(n) => Some(self.global_arr(n)),
            Loc::Frame(n) => self.frames.last_mut().and_then(|f| f.vars.get_mut(&**n)),
            Loc::FrameAt(d, n) => self.frames.get_mut(*d).and_then(|f| f.vars.get_mut(&**n)),
            Loc::Static(f, n) => self.statics.get_mut(&f.name).and_then(|m| m.get_mut(&**n)),
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
                    self.globals.entry("SAVEDATA_TEXT".into()).or_insert_with(|| Arr::new(true, &[1])).set(0, Value::Str(s));
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
            "MONEYLABEL" => Value::Str(self.csv.gamebase.get("お金の単位").cloned().unwrap_or_else(|| "$".into())),
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
        let mut out = String::new();
        for p in &f.parts {
            match p {
                FormPart::Lit(s) => out.push_str(s),
                FormPart::Str(e, w, a) | FormPart::Int(e, w, a) => {
                    let s = self.eval(e)?.into_string();
                    match w {
                        Some(w) => {
                            let w = self.eval_int(w)?.max(0) as usize;
                            out.push_str(&pad(&s, w, *a));
                        }
                        None => out.push_str(&s),
                    }
                }
                FormPart::Cond(cab) => {
                    let (c, a, b) = &**cab;
                    let s = if self.eval(c)?.truthy() { self.eval_form(a)? } else { self.eval_form(b)? };
                    out.push_str(&s);
                }
            }
        }
        Ok(out)
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
        let Some(fs) = self.prog.funcs.get(name).cloned() else { return Ok(false) };
        if fs.len() == 1 {
            self.call_func(fs[0].clone(), args)?;
            return Ok(true);
        }
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
        for (i, a) in args.iter().enumerate() {
            let p = f.params.get(i).map(|p| p.target.name.clone());
            let is_ref = p.as_ref().is_some_and(|n| f.dims.iter().any(|d| d.is_ref && *d.name == **n));
            if let (true, Expr::Var(v)) = (is_ref, a) {
                let top = self.frames.len().saturating_sub(1);
                let loc = match self.locate(&VarRef { name: v.name.clone(), idx: vec![] })?.loc {
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
    pub fn call_func(&mut self, f: Rc<Function>, args: Vec<Option<Value>>) -> VmResult<Option<Value>> {
        let refs: HashMap<String, Loc> = std::mem::take(&mut self.pending_refs).into_iter().collect();
        // debugging aid: EMUERA_TRACE=<substring of function name>
        if let Some(t) = &self.trace {
            if f.name.contains(t.as_str()) {
                eprintln!("trace: {}@{} args={:?} CHARANUM={}", "  ".repeat(self.frames.len()), f.name, args, self.charas.len());
            }
        }
        if self.frames.len() >= MAX_DEPTH {
            return err("call stack overflow");
        }
        // allocate private (non-static) dims in the caller context
        let mut vars = HashMap::default();
        for d in &f.dims {
            if !d.is_static {
                let a = self.make_dim(d)?;
                vars.insert(d.name.clone(), a);
            }
        }
        self.frames.push(Frame { func: f.clone(), pc: 0, vars, slots: vec![Slot::None; f.n_slots], refs });
        // bind parameters
        for (i, p) in f.params.iter().enumerate() {
            let v = match args.get(i).cloned().flatten() {
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
                        let is_param_var = matches!(n, "ARG" | "ARGS") || f.dims.iter().any(|d| d.name == n && !d.is_ref);
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
        if let (Some(t), Ok(v)) = (&self.trace, &r) {
            if f.name.contains(t.as_str()) {
                eprintln!("trace: {}<- @{} = {:?}", "  ".repeat(self.frames.len()), f.name, v);
            }
        }
        r
    }

    /// Prefix an error with "file:line: @FUNC:" the first time it passes
    /// through a frame, so messages point at the failing ERB line.
    fn located(&self, e: Ctrl, f: &Function, pc: usize) -> Ctrl {
        match e {
            Ctrl::Error(m, false) => {
                let (file, line) = f.lines.get(pc).copied().unwrap_or((f.file, f.line));
                let path = self.prog.files.get(file as usize).cloned().unwrap_or_default();
                Ctrl::Error(format!("{}:{}: @{}: {}", path, line, f.name, m), true)
            }
            e => e,
        }
    }

    /// The instruction loop of the innermost frame.
    fn exec(&mut self) -> VmResult<Option<Value>> {
        loop {
            let (f, pc) = {
                let fr = self.frames.last().unwrap();
                (fr.func.clone(), fr.pc)
            };
            if pc >= f.code.len() {
                return Ok(None);
            }
            self.frames.last_mut().unwrap().pc = pc + 1;
            self.ticks = self.ticks.wrapping_add(1);
            if self.ticks & 0xFFF == 0 {
                self.tick()?;
            }
            match self.step(&f, pc) {
                Ok(None) => {}
                Ok(Some(ret)) => return Ok(ret),
                Err(e) => return Err(self.located(e, &f, pc)),
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
    fn step(&mut self, f: &Rc<Function>, pc: usize) -> VmResult<Option<Option<Value>>> {
        match &f.code[pc] {
            Instr::BadLine(msg, fatal) => {
                if *fatal || self.strict {
                    return err(format!("line failed to parse: {}", msg));
                }
                self.warn(&format!("skipped unparsed line: {}", msg));
            }
            Instr::Print(content, fl) => {
                let s = match content {
                    PrintContent::Raw(s) => s.clone(),
                    PrintContent::Form(fm) => self.eval_form(fm)?,
                    PrintContent::Expr(e) => self.eval_str(e)?,
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
                        text.push_str(&self.eval_form(l)?);
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
            Instr::SelectEval(e, slot) => {
                let v = self.eval(e)?;
                self.frames.last_mut().unwrap().slots[*slot] = Slot::Sel(v);
            }
            Instr::CaseTest(slot, conds, next) => {
                let v = match &self.frames.last().unwrap().slots[*slot] {
                    Slot::Sel(v) => v.clone(),
                    _ => Value::Int(0),
                };
                let mut hit = false;
                for c in conds {
                    if self.case_match(&v, c)? {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    self.set_pc(*next);
                }
            }
            Instr::Call(ci) => {
                let CallInstr { target, args, kind, try_, catch } = &**ci;
                let name = self.resolve_target(target)?;
                let (vals, refs) = match self.prog.funcs.get(&name).map(|fs| fs[0].clone()) {
                    Some(f) => self.call_args(&f, args)?,
                    None => (Vec::new(), Vec::new()),
                };
                if !self.has_func(&name) {
                    if *try_ {
                        if let Some(c) = catch {
                            self.set_pc(*c);
                        }
                        return Ok(None);
                    }
                    return err(format!("function @{} not found", name));
                }
                self.pending_refs = refs;
                self.call_name(&name, vals)?;
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
                match f.labels.get(&label) {
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
                    if let Some(&p) = f.labels.get(&label) {
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
                let x = self.eval(e)?;
                if v.is_str() || x.is_str() { v.as_string() == x.as_string() } else { v.as_int() == x.as_int() }
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
        let text = match fl.column {
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
            self.con.print_ex(&text, !fl.plain);
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
            InputResult::Timeout => Ok(None),
            InputResult::Quit => Err(Ctrl::Quit),
        }
    }

    /// WAIT: show everything, continue on any key / click.
    pub fn wait(&mut self) -> VmResult<()> {
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
        self.raw_input(&InputReq { kind: InputKind::Wait, one: true, timeout_ms: None, mouse: false })?;
        Ok(())
    }

    /// INPUT-style integer input: re-asks on non-numbers, empty input gives
    /// the default; full-width digits are accepted. Callers store RESULT.
    pub fn input_int(&mut self, default: Option<i64>, timeout: Option<u64>, one: bool) -> VmResult<i64> {
        loop {
            let r = self.raw_input(&InputReq { kind: InputKind::Int, one, timeout_ms: timeout, mouse: false })?;
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
        let r = self.raw_input(&InputReq { kind: InputKind::Str, one, timeout_ms: timeout, mouse: false })?;
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
        if self.con.dirty && self.con.redraw {
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
