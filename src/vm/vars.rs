//! Variable storage: arrays of ints or strings with 0..3 dimensions, plus
//! the table of Emuera's built-in variables and their default sizes.

use super::value::Value;

/// The element storage of an [`Arr`], row-major.
#[derive(Clone, Debug)]
pub enum Data {
    I(Vec<i64>),
    S(Vec<String>),
}

/// One variable: every Emuera variable, scalars included, is an array.
#[derive(Clone, Debug)]
pub struct Arr {
    pub data: Data,
    /// empty => scalar
    pub dims: Vec<usize>,
}

impl Arr {
    pub fn new(is_str: bool, dims: &[usize]) -> Arr {
        let n: usize = dims.iter().product::<usize>().max(1);
        Arr {
            data: if is_str { Data::S(vec![String::new(); n]) } else { Data::I(vec![0; n]) },
            dims: dims.to_vec(),
        }
    }

    pub fn is_str(&self) -> bool {
        matches!(self.data, Data::S(_))
    }

    pub fn len(&self) -> usize {
        match &self.data {
            Data::I(v) => v.len(),
            Data::S(v) => v.len(),
        }
    }

    /// Flatten indices. 1-D (and scalar) arrays accept out-of-range indices
    /// (auto-grow on write, default on read); multi-D arrays are bounds checked.
    pub fn flat(&self, idx: &[i64]) -> Result<usize, String> {
        for &i in idx {
            if i < 0 {
                return Err(format!("negative array index {}", i));
            }
        }
        if self.dims.len() <= 1 {
            if idx.len() > 1 && idx[1..].iter().any(|&x| x != 0) {
                return Err(format!("too many indices ({}) for 1-D array", idx.len()));
            }
            return Ok(idx.first().copied().unwrap_or(0) as usize);
        }
        if idx.len() > self.dims.len() {
            return Err(format!("too many indices ({}) for {}-D array", idx.len(), self.dims.len()));
        }
        let mut f = 0usize;
        for (d, &size) in self.dims.iter().enumerate() {
            let i = idx.get(d).copied().unwrap_or(0) as usize;
            if i >= size {
                return Err(format!("index {} out of range (dim {} size {})", i, d, size));
            }
            f = f * size + i;
        }
        Ok(f)
    }

    /// Element `f`; past the end reads as 0 / "" (1-D arrays grow on write).
    pub fn get(&self, f: usize) -> Value {
        match &self.data {
            Data::I(v) => Value::Int(v.get(f).copied().unwrap_or(0)),
            Data::S(v) => Value::Str(v.get(f).cloned().unwrap_or_default()),
        }
    }

    /// Make room for element `f` (capped at 50M elements to survive
    /// runaway indices).
    fn grow(&mut self, f: usize) {
        if f >= self.len() {
            if f > 50_000_000 {
                return;
            }
            match &mut self.data {
                Data::I(v) => v.resize(f + 1, 0),
                Data::S(v) => v.resize(f + 1, String::new()),
            }
            if self.dims.len() == 1 {
                self.dims[0] = f + 1;
            }
        }
    }

    /// Store `val`, converting to the array's type.
    pub fn set(&mut self, f: usize, val: Value) {
        self.grow(f);
        match &mut self.data {
            Data::I(v) => {
                if let Some(x) = v.get_mut(f) {
                    *x = val.as_int()
                }
            }
            Data::S(v) => {
                if let Some(x) = v.get_mut(f) {
                    *x = val.into_string()
                }
            }
        }
    }

    pub fn get_i(&self, f: usize) -> i64 {
        match &self.data {
            Data::I(v) => v.get(f).copied().unwrap_or(0),
            Data::S(v) => v.get(f).map(|s| s.parse().unwrap_or(0)).unwrap_or(0),
        }
    }

    pub fn set_i(&mut self, f: usize, x: i64) {
        self.set(f, Value::Int(x));
    }

    /// Reset every element to 0 / "".
    pub fn clear(&mut self) {
        match &mut self.data {
            Data::I(v) => v.iter_mut().for_each(|x| *x = 0),
            Data::S(v) => v.iter_mut().for_each(|x| x.clear()),
        }
    }

    /// Size of the last dimension (row length) for multi-assign / VARSET ranges.
    pub fn last_dim(&self) -> usize {
        self.dims.last().copied().unwrap_or(1)
    }
}

/// A built-in variable: string or int, per-character or global, and its
/// default dimensions (empty = scalar).
pub struct VarDef {
    pub name: &'static str,
    pub is_str: bool,
    pub chara: bool,
    pub dims: &'static [usize],
}

macro_rules! v {
    ($n:expr, $s:expr, $c:expr, $d:expr) => {
        VarDef { name: $n, is_str: $s, chara: $c, dims: $d }
    };
}

/// Built-in variables with Emuera's default sizes (`ConstantData`: integer
/// arrays 1000, FLAG 10000, character arrays 100 but TALENT / CFLAG 1000 and
/// JUEL / GOTJUEL 200, CDFLAG 1x1); VariableSize.csv overrides them.
pub static SYSTEM_VARS: &[VarDef] = &[
    v!("DAY", false, false, &[1000]),
    v!("MONEY", false, false, &[1000]),
    v!("NOITEM", false, false, &[1000]),
    v!("TIME", false, false, &[1000]),
    v!("ITEM", false, false, &[1000]),
    v!("ITEMSALES", false, false, &[1000]),
    v!("BOUGHT", false, false, &[1000]),
    v!("PBAND", false, false, &[1000]),
    v!("FLAG", false, false, &[10000]),
    v!("TFLAG", false, false, &[1000]),
    v!("UP", false, false, &[1000]),
    v!("DOWN", false, false, &[1000]),
    v!("LOSEBASE", false, false, &[1000]),
    v!("PALAMLV", false, false, &[1000]),
    v!("EXPLV", false, false, &[1000]),
    v!("EJAC", false, false, &[1000]),
    v!("RESULT", false, false, &[1000]),
    v!("COUNT", false, false, &[1000]),
    v!("TARGET", false, false, &[1000]),
    v!("ASSI", false, false, &[1000]),
    v!("MASTER", false, false, &[1000]),
    v!("PLAYER", false, false, &[1000]),
    v!("ASSIPLAY", false, false, &[1000]),
    v!("SELECTCOM", false, false, &[1000]),
    v!("PREVCOM", false, false, &[1000]),
    v!("NEXTCOM", false, false, &[1000]),
    v!("NOWEX_", false, false, &[1]),
    v!("GLOBAL", false, false, &[1000]),
    v!("RANDDATA", false, false, &[625]),
    v!("ITEMPRICE", false, false, &[1000]),
    v!("DITEMTYPE", false, false, &[100, 100]),
    v!("DA", false, false, &[100, 100]),
    v!("DB", false, false, &[100, 100]),
    v!("DC", false, false, &[100, 100]),
    v!("DD", false, false, &[100, 100]),
    v!("DE", false, false, &[100, 100]),
    v!("TA", false, false, &[100, 100, 100]),
    v!("TB", false, false, &[100, 100, 100]),
    v!("A", false, false, &[1000]),
    v!("B", false, false, &[1000]),
    v!("C", false, false, &[1000]),
    v!("D", false, false, &[1000]),
    v!("E", false, false, &[1000]),
    v!("F", false, false, &[1000]),
    v!("G", false, false, &[1000]),
    v!("H", false, false, &[1000]),
    v!("I", false, false, &[1000]),
    v!("J", false, false, &[1000]),
    v!("K", false, false, &[1000]),
    v!("L", false, false, &[1000]),
    v!("M", false, false, &[1000]),
    v!("N", false, false, &[1000]),
    v!("O", false, false, &[1000]),
    v!("P", false, false, &[1000]),
    v!("Q", false, false, &[1000]),
    v!("R", false, false, &[1000]),
    v!("S", false, false, &[1000]),
    v!("T", false, false, &[1000]),
    v!("U", false, false, &[1000]),
    v!("V", false, false, &[1000]),
    v!("W", false, false, &[1000]),
    v!("X", false, false, &[1000]),
    v!("Y", false, false, &[1000]),
    v!("Z", false, false, &[1000]),
    v!("STR", true, false, &[20000]),
    v!("SAVESTR", true, false, &[100]),
    v!("RESULTS", true, false, &[100]),
    v!("TSTR", true, false, &[100]),
    v!("GLOBALS", true, false, &[100]),
    // character variables
    v!("NO", false, true, &[]),
    v!("ISASSI", false, true, &[]),
    v!("BASE", false, true, &[100]),
    v!("MAXBASE", false, true, &[100]),
    v!("DOWNBASE", false, true, &[100]),
    v!("ABL", false, true, &[100]),
    v!("TALENT", false, true, &[1000]),
    v!("EXP", false, true, &[100]),
    v!("MARK", false, true, &[100]),
    v!("PALAM", false, true, &[100]),
    v!("SOURCE", false, true, &[100]),
    v!("EX", false, true, &[100]),
    v!("NOWEX", false, true, &[100]),
    v!("CFLAG", false, true, &[1000]),
    v!("JUEL", false, true, &[200]),
    v!("GOTJUEL", false, true, &[200]),
    v!("CUP", false, true, &[100]),
    v!("CDOWN", false, true, &[100]),
    v!("RELATION", false, true, &[100]),
    v!("EQUIP", false, true, &[100]),
    v!("TEQUIP", false, true, &[100]),
    v!("STAIN", false, true, &[100]),
    v!("TCVAR", false, true, &[100]),
    v!("CDFLAG", false, true, &[1, 1]),
    v!("NAME", true, true, &[]),
    v!("CALLNAME", true, true, &[]),
    v!("NICKNAME", true, true, &[]),
    v!("MASTERNAME", true, true, &[]),
    v!("CSTR", true, true, &[100]),
];

/// Look up a built-in variable by (upper-case) name. Called on nearly every
/// variable access, so it's a hash lookup rather than a scan of the table.
pub fn system_var(name: &str) -> Option<&'static VarDef> {
    static MAP: std::sync::OnceLock<crate::fxhash::HashMap<&'static str, &'static VarDef>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| SYSTEM_VARS.iter().map(|d| (d.name, d)).collect()).get(name).copied()
}
