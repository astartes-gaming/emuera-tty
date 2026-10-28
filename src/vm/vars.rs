//! Variable storage: arrays of ints or strings with 0..3 dimensions, plus
//! the table of Emuera's built-in variables and their default sizes.

use super::value::Value;

/// The element storage of an [`Arr`], row-major. It holds the elements up
/// to the last one written: those past its end read as 0 / "", so a big
/// array nothing wrote to costs nothing (eraAL-K's characters have
/// CSTR:100000 and CFLAG:100000 each).
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
        Arr { data: if is_str { Data::S(Vec::new()) } else { Data::I(Vec::new()) }, dims: dims.to_vec() }
    }

    pub fn is_str(&self) -> bool {
        matches!(self.data, Data::S(_))
    }

    /// Heap bytes held (strings included).
    pub fn bytes(&self) -> usize {
        match &self.data {
            Data::I(v) => v.capacity() * 8,
            Data::S(v) => v.capacity() * 24 + v.iter().map(|s| s.capacity()).sum::<usize>(),
        }
    }

    /// The number of elements (written or not).
    pub fn len(&self) -> usize {
        match self.dims.len() {
            0 => self.stored().max(1),
            1 => self.dims[0],
            _ => self.dims.iter().product(),
        }
    }

    /// The number of elements held (see [`Data`]).
    fn stored(&self) -> usize {
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

    /// Make a 1-D array (or a scalar) long enough for element `f` (capped
    /// at 50M elements to survive runaway indices); false if `f` stays out
    /// of range.
    fn extend(&mut self, f: usize) -> bool {
        let n = self.len();
        if f < n {
            return true;
        }
        if f > 50_000_000 || self.dims.len() > 1 {
            return false;
        }
        if let [d] = &mut self.dims[..] {
            *d = f + 1;
        }
        true
    }

    /// Make room for element `f`, holding the elements up to it: a small
    /// array is held whole at once.
    fn grow(&mut self, f: usize) {
        if f < self.stored() || !self.extend(f) {
            return;
        }
        let n = self.len();
        let to = if n <= 1024 { n.max(f + 1) } else { f + 1 };
        match &mut self.data {
            Data::I(v) => v.resize(to, 0),
            Data::S(v) => v.resize(to, String::new()),
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

    /// Set elements `range` to `val` (VARSET), growing a 1-D array to reach
    /// the end; strings reuse their buffers.
    pub fn fill(&mut self, range: std::ops::Range<usize>, val: &Value) {
        if range.is_empty() {
            return;
        }
        // 0 / "" needn't reach past the elements held
        let unset = match &self.data {
            Data::I(_) => val.as_int() == 0,
            Data::S(_) => val.as_string().is_empty(),
        };
        if unset {
            self.extend(range.end - 1);
        } else {
            self.grow(range.end - 1);
        }
        let end = range.end.min(self.stored());
        let start = range.start.min(end);
        match &mut self.data {
            Data::I(v) => v[start..end].fill(val.as_int()),
            Data::S(v) => {
                let s = val.as_string();
                for x in &mut v[start..end] {
                    x.clear();
                    x.push_str(&s);
                }
            }
        }
    }

    /// Move the elements of `range` `k` places (toward its end when
    /// positive) in place: those pushed past either end are dropped and the
    /// places left free get `free` (ARRAYSHIFT, ARRAYREMOVE). Nothing is
    /// copied, which matters for the 1000-string arrays games shift.
    pub fn shift(&mut self, range: std::ops::Range<usize>, k: i64, free: &Value) {
        if range.is_empty() || k == 0 {
            return;
        }
        self.grow(range.end - 1);
        let end = range.end.min(self.stored());
        let start = range.start.min(end);
        let m = (k.unsigned_abs() as usize).min(end - start);
        if m == 0 {
            return;
        }
        fn rot<T>(s: &mut [T], m: usize, right: bool) {
            if right {
                s.rotate_right(m)
            } else {
                s.rotate_left(m)
            }
        }
        match &mut self.data {
            Data::I(v) => rot(&mut v[start..end], m, k > 0),
            Data::S(v) => rot(&mut v[start..end], m, k > 0),
        }
        self.fill(if k > 0 { start..start + m } else { end - m..end }, free);
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
            Data::I(v) => v.clear(),
            Data::S(v) => v.clear(),
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

/// The global variables: one slot per name, kept for the program's life
/// (clearing empties the slots, it doesn't renumber them), so a variable
/// reference can remember its slot and skip the lookup by name.
#[derive(Default)]
pub struct Globals {
    slots: crate::fxhash::HashMap<String, usize>,
    arrs: Vec<Option<Arr>>,
}

impl Globals {
    /// The slot for `name` (made if new).
    pub fn slot_of(&mut self, name: &str) -> usize {
        if let Some(&s) = self.slots.get(name) {
            return s;
        }
        let s = self.arrs.len();
        self.arrs.push(None);
        self.slots.insert(name.to_string(), s);
        s
    }
    pub fn at(&mut self, slot: usize) -> Option<&mut Arr> {
        self.arrs.get_mut(slot).and_then(|a| a.as_mut())
    }
    pub fn at_ref(&self, slot: usize) -> Option<&Arr> {
        self.arrs.get(slot).and_then(|a| a.as_ref())
    }
    pub fn get(&self, name: &str) -> Option<&Arr> {
        self.slots.get(name).and_then(|&s| self.arrs[s].as_ref())
    }
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Arr> {
        match self.slots.get(name) {
            Some(&s) => self.arrs[s].as_mut(),
            None => None,
        }
    }
    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
    pub fn insert(&mut self, name: String, a: Arr) -> Option<Arr> {
        let s = self.slot_of(&name);
        self.arrs[s].replace(a)
    }
    pub fn remove(&mut self, name: &str) -> Option<Arr> {
        let s = *self.slots.get(name)?;
        self.arrs[s].take()
    }
    pub fn clear(&mut self) {
        self.arrs.fill(None);
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Arr)> {
        self.slots.iter().filter_map(|(k, &s)| self.arrs[s].as_ref().map(|a| (k, a)))
    }
    pub fn get_or_insert_with(&mut self, name: &str, f: impl FnOnce() -> Arr) -> &mut Arr {
        let s = self.slot_of(name);
        self.arrs[s].get_or_insert_with(f)
    }
}

impl std::ops::Index<&str> for Globals {
    type Output = Arr;
    fn index(&self, name: &str) -> &Arr {
        self.get(name).expect("no such global")
    }
}

#[cfg(test)]
mod tests {
    use super::{Arr, Value};

    /// ARRAYSHIFT / ARRAYREMOVE as they were written before `shift`: copy
    /// the values out and back.
    fn shifted(vals: &[i64], range: std::ops::Range<usize>, k: i64, free: i64) -> Vec<i64> {
        let mut out = vals.to_vec();
        let n = range.len() as i64;
        for i in 0..n {
            let src = i - k;
            out[range.start + i as usize] = if (0..n).contains(&src) { vals[range.start + src as usize] } else { free };
        }
        out
    }

    #[test]
    fn shift_matches_copying() {
        let vals: Vec<i64> = (1..=9).collect();
        for (r, k) in [(0..9, 2), (0..9, -3), (2..7, 1), (2..7, -1), (3..5, 9), (3..5, -9), (0..9, 0), (4..4, 2)] {
            let mut a = Arr::new(false, &[9]);
            for (i, &v) in vals.iter().enumerate() {
                a.set(i, Value::Int(v));
            }
            a.shift(r.clone(), k, &Value::Int(-1));
            let got: Vec<i64> = (0..9).map(|i| a.get_i(i)).collect();
            assert_eq!(got, shifted(&vals, r.clone(), k, -1), "{:?} by {}", r, k);
        }
        // strings move without copies; the freed places get the fill value
        let mut s = Arr::new(true, &[4]);
        for (i, t) in ["a", "b", "c", "d"].iter().enumerate() {
            s.set(i, Value::Str(t.to_string()));
        }
        s.shift(1..4, -1, &Value::Str(String::new()));
        assert_eq!((0..4).map(|i| s.get(i).into_string()).collect::<Vec<_>>(), ["a", "c", "d", ""]);
    }

    #[test]
    fn big_arrays_hold_only_what_was_written() {
        let mut a = Arr::new(true, &[100_000]);
        assert_eq!((a.len(), a.bytes()), (100_000, 0));
        a.set(10, Value::Str("x".into()));
        assert_eq!((a.len(), a.get(10).into_string(), a.get(99_999).into_string()), (100_000, "x".to_string(), String::new()));
        assert!(a.bytes() < 1000);
        // clearing past the held part holds nothing more; a value does
        a.fill(0..100_000, &Value::Str(String::new()));
        assert!(a.bytes() < 1000 && a.get(10).into_string().is_empty());
        a.fill(50..60, &Value::Str("y".into()));
        assert_eq!((a.get(59).into_string(), a.get(60).into_string()), ("y".to_string(), String::new()));
        // 1-D arrays still grow past their size; 2-D ones don't
        a.set(100_005, Value::Str("z".into()));
        assert_eq!(a.len(), 100_006);
        let mut m = Arr::new(false, &[3, 4]);
        assert_eq!(m.len(), 12);
        m.set(11, Value::Int(5));
        m.set(12, Value::Int(6));
        assert_eq!((m.get_i(11), m.get_i(12), m.len()), (5, 0, 12));
        // a scalar
        let mut sc = Arr::new(false, &[]);
        assert_eq!(sc.len(), 1);
        sc.set(0, Value::Int(3));
        assert_eq!(sc.get_i(0), 3);
    }
}
