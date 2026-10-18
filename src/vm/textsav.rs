//! Emuera's text save format (`EraDataReader`), written by games whose
//! config turns off "セーブデータをバイナリ形式で保存する": one value per
//! line. First an eramaker-compatible part with the variables in a fixed
//! order (arrays end with `__FINISHED`), then, after an
//! `__EMUERA_xxxx_STRAT__` line, Emuera's own variables by name in
//! sections ended by `__EMU_SEPARATOR__`. Only reading is needed: Emuera
//! loads binary saves whatever its config says, so we always write those.

/// The values of one saved variable: flat, with its dimensions.
#[derive(Debug, Clone, PartialEq)]
pub enum Vals {
    I(Vec<usize>, Vec<i64>),
    S(Vec<usize>, Vec<String>),
}

/// A parsed save: header, characters' and other variables (uppercase names).
#[derive(Debug, Default)]
#[allow(dead_code)] // the header is checked by `header`
pub struct TextSave {
    pub code: i64,
    pub version: i64,
    pub text: String,
    pub charas: Vec<Vec<(String, Vals)>>,
    pub vars: Vec<(String, Vals)>,
}

/// eramaker's character part: strings, integers, then integer arrays.
const CHARA_STR: &[&str] = &["NAME", "CALLNAME"];
const CHARA_INT: &[&str] = &["ISASSI", "NO"];
const CHARA_INT_ARR: &[&str] = &[
    "BASE", "MAXBASE", "ABL", "TALENT", "EXP", "MARK", "PALAM", "SOURCE", "EX", "CFLAG", "JUEL", "RELATION", "EQUIP", "TEQUIP", "STAIN", "GOTJUEL", "NOWEX",
];
/// eramaker's global part: 60 integer arrays (codes 0x00-0x3B), SAVESTR.
const GLOBAL_INT_ARR: &[&str] = &[
    "DAY", "MONEY", "ITEM", "FLAG", "TFLAG", "UP", "PALAMLV", "EXPLV", "EJAC", "DOWN", "RESULT", "COUNT", "TARGET", "ASSI", "MASTER", "NOITEM",
    "LOSEBASE", "SELECTCOM", "ASSIPLAY", "PREVCOM", "", "", "TIME", "ITEMSALES", "PLAYER", "NEXTCOM", "PBAND", "BOUGHT", "", "", "A", "B", "C",
    "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "", "", "", "",
];

const FINISHER: &str = "__FINISHED";
const SEPARATOR: &str = "__EMU_SEPARATOR__";

struct Lines<'a> {
    it: std::str::Lines<'a>,
}

impl<'a> Lines<'a> {
    fn next(&mut self) -> Result<&'a str, String> {
        self.it.next().map(|l| l.strip_suffix('\r').unwrap_or(l)).ok_or_else(|| "the save data ends too early".to_string())
    }
    fn int(&mut self) -> Result<i64, String> {
        let l = self.next()?;
        l.trim().parse().map_err(|_| format!("not a number: {:?}", l))
    }
    /// Lines up to `__FINISHED`.
    fn list(&mut self) -> Result<Vec<&'a str>, String> {
        let mut v = Vec::new();
        loop {
            let l = self.next()?;
            if l == FINISHER {
                return Ok(v);
            }
            if l == SEPARATOR {
                return Err("broken save data (separator inside an array)".into());
            }
            v.push(l);
        }
    }
    fn ints(&mut self) -> Result<Vec<i64>, String> {
        self.list()?.iter().map(|l| l.trim().parse().map_err(|_| format!("not a number: {:?}", l))).collect()
    }
    /// The Emuera marker, if any, and its version.
    fn seek_emu(&mut self) -> Option<u32> {
        for l in self.it.by_ref() {
            let l = l.trim_end_matches('\r');
            for (m, v) in [("__EMUERA_STRAT__", 1700), ("__EMUERA_1708_STRAT__", 1708), ("__EMUERA_1729_STRAT__", 1729), ("__EMUERA_1803_STRAT__", 1803), ("__EMUERA_1808_STRAT__", 1808)] {
                if l == m {
                    return Some(v);
                }
            }
        }
        None
    }
}

/// One `key:value` section (scalars), up to the separator.
fn scalars(l: &mut Lines, out: &mut Vec<(String, Vals)>, int: bool) -> Result<(), String> {
    loop {
        let s = l.next()?;
        if s == SEPARATOR {
            return Ok(());
        }
        let (k, v) = s.split_once(':').ok_or("broken save data (no ':')")?;
        let vals = if int {
            Vals::I(vec![1], vec![v.trim().parse().map_err(|_| format!("not a number: {:?}", v))?])
        } else {
            Vals::S(vec![1], vec![v.to_string()])
        };
        out.push((k.to_uppercase(), vals));
    }
}

/// A section of named 1-D arrays.
fn arrays(l: &mut Lines, out: &mut Vec<(String, Vals)>, int: bool) -> Result<(), String> {
    loop {
        let k = l.next()?;
        if k == SEPARATOR {
            return Ok(());
        }
        let vals = if int {
            let v = l.ints()?;
            Vals::I(vec![v.len()], v)
        } else {
            let v: Vec<String> = l.list()?.into_iter().map(String::from).collect();
            Vals::S(vec![v.len()], v)
        };
        out.push((k.to_uppercase(), vals));
    }
}

/// Comma-separated numbers ("" is an empty row).
fn row(s: &str) -> Result<Vec<i64>, String> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    s.split(',').map(|t| t.trim().parse().map_err(|_| format!("not a number: {:?}", t))).collect()
}

/// Rows of different lengths → a flat, rectangular array.
fn rect(rows: &[Vec<i64>]) -> (usize, Vec<i64>) {
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
    let mut flat = vec![0; rows.len() * w];
    for (y, r) in rows.iter().enumerate() {
        flat[y * w..y * w + r.len()].copy_from_slice(r);
    }
    (w, flat)
}

/// A section of named 2-D integer arrays (one row per line).
fn arrays2d(l: &mut Lines, out: &mut Vec<(String, Vals)>) -> Result<(), String> {
    loop {
        let k = l.next()?;
        if k == SEPARATOR {
            return Ok(());
        }
        let rows: Vec<Vec<i64>> = l.list()?.into_iter().map(row).collect::<Result<_, _>>()?;
        let (w, flat) = rect(&rows);
        out.push((k.to_uppercase(), Vals::I(vec![rows.len(), w], flat)));
    }
}

/// A section of named 3-D integer arrays (`x{` … `}` blocks of rows).
fn arrays3d(l: &mut Lines, out: &mut Vec<(String, Vals)>) -> Result<(), String> {
    loop {
        let k = l.next()?;
        if k == SEPARATOR {
            return Ok(());
        }
        let mut planes: Vec<Vec<Vec<i64>>> = Vec::new();
        loop {
            let s = l.next()?;
            if s == FINISHER {
                break;
            }
            if s.contains('{') {
                let mut p = Vec::new();
                loop {
                    let r = l.next()?;
                    if r == "}" {
                        break;
                    }
                    p.push(row(r)?);
                }
                planes.push(p);
            }
        }
        let ny = planes.iter().map(|p| p.len()).max().unwrap_or(0).max(1);
        let nz = planes.iter().flatten().map(|r| r.len()).max().unwrap_or(0).max(1);
        let mut flat = vec![0; planes.len() * ny * nz];
        for (x, p) in planes.iter().enumerate() {
            for (y, r) in p.iter().enumerate() {
                let o = (x * ny + y) * nz;
                flat[o..o + r.len()].copy_from_slice(r);
            }
        }
        out.push((k.to_uppercase(), Vals::I(vec![planes.len(), ny, nz], flat)));
    }
}

/// A section that must be empty in this version (string 2-D / 3-D arrays,
/// which Emuera never writes).
fn skip_section(l: &mut Lines) -> Result<(), String> {
    loop {
        if l.next()? == SEPARATOR {
            return Ok(());
        }
    }
}

/// Is this a text save? (The first line is the game's code, a number.)
pub fn looks_like(bytes: &[u8]) -> bool {
    let s = crate::loader::decode(&bytes[..bytes.len().min(64)]);
    s.lines().next().is_some_and(|l| !l.trim().is_empty() && l.trim().trim_start_matches('-').chars().all(|c| c.is_ascii_digit()))
}

/// The header only: (game code, version, comment).
pub fn header(bytes: &[u8]) -> Result<(i64, i64, String), String> {
    let text = crate::loader::decode(bytes);
    let mut l = Lines { it: text.lines() };
    Ok((l.int()?, l.int()?, l.next()?.to_string()))
}

/// Parse a whole normal (slot) save.
pub fn parse(bytes: &[u8]) -> Result<TextSave, String> {
    let text = crate::loader::decode(bytes);
    let mut l = Lines { it: text.lines() };
    let mut s = TextSave { code: l.int()?, version: l.int()?, text: l.next()?.to_string(), ..TextSave::default() };
    let n = l.int()?;
    if !(0..=100_000).contains(&n) {
        return Err("bad character count".into());
    }
    for _ in 0..n {
        let mut c = Vec::new();
        for k in CHARA_STR {
            c.push((k.to_string(), Vals::S(vec![1], vec![l.next()?.to_string()])));
        }
        for k in CHARA_INT {
            c.push((k.to_string(), Vals::I(vec![1], vec![l.int()?])));
        }
        for k in CHARA_INT_ARR {
            let v = l.ints()?;
            c.push((k.to_string(), Vals::I(vec![v.len()], v)));
        }
        s.charas.push(c);
    }
    for k in GLOBAL_INT_ARR {
        let v = l.ints()?;
        if !k.is_empty() {
            s.vars.push((k.to_string(), Vals::I(vec![v.len()], v)));
        }
    }
    let v: Vec<String> = l.list()?.into_iter().map(String::from).collect();
    s.vars.push(("SAVESTR".into(), Vals::S(vec![v.len()], v)));
    let Some(emu) = l.seek_emu() else { return Ok(s) };
    for c in s.charas.iter_mut() {
        scalars(&mut l, c, false)?;
        scalars(&mut l, c, true)?;
        arrays(&mut l, c, false)?;
        arrays(&mut l, c, true)?;
        if emu >= 1803 {
            skip_section(&mut l)?;
            arrays2d(&mut l, c)?;
        }
    }
    scalars(&mut l, &mut s.vars, false)?;
    scalars(&mut l, &mut s.vars, true)?;
    arrays(&mut l, &mut s.vars, false)?;
    arrays(&mut l, &mut s.vars, true)?;
    if emu >= 1708 {
        skip_section(&mut l)?;
        arrays2d(&mut l, &mut s.vars)?;
    }
    if emu >= 1729 {
        skip_section(&mut l)?;
        arrays3d(&mut l, &mut s.vars)?;
    }
    if emu >= 1808 {
        // the game's own SAVEDATA variables (#DIM SAVEDATA)
        arrays(&mut l, &mut s.vars, false)?;
        arrays(&mut l, &mut s.vars, true)?;
        skip_section(&mut l)?;
        arrays2d(&mut l, &mut s.vars)?;
        skip_section(&mut l)?;
        arrays3d(&mut l, &mut s.vars)?;
    }
    Ok(s)
}

/// Parse a text global.sav: GLOBAL and GLOBALS, then the game's GLOBAL
/// SAVEDATA variables.
pub fn parse_global(bytes: &[u8]) -> Result<TextSave, String> {
    let text = crate::loader::decode(bytes);
    let mut l = Lines { it: text.lines() };
    let mut s = TextSave { code: l.int()?, version: l.int()?, ..TextSave::default() };
    let g = l.ints()?;
    s.vars.push(("GLOBAL".into(), Vals::I(vec![g.len()], g)));
    let gs: Vec<String> = l.list()?.into_iter().map(String::from).collect();
    s.vars.push(("GLOBALS".into(), Vals::S(vec![gs.len()], gs)));
    if l.seek_emu().is_some() {
        arrays(&mut l, &mut s.vars, false)?;
        arrays(&mut l, &mut s.vars, true)?;
        skip_section(&mut l)?;
        arrays2d(&mut l, &mut s.vars)?;
        skip_section(&mut l)?;
        arrays3d(&mut l, &mut s.vars)?;
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let mut t = vec!["123".to_string(), "1000".into(), "a comment".into(), "1".into()];
        // one character
        t.extend(["Reimu", "Reimu-san", "0", "1"].map(String::from));
        for k in 0..CHARA_INT_ARR.len() {
            if k == 0 {
                t.extend(["100".into(), "200".into()]);
            }
            t.push(FINISHER.into());
        }
        for k in 0..GLOBAL_INT_ARR.len() {
            if k == 0 {
                t.push("7".into());
            }
            t.push(FINISHER.into());
        }
        t.extend(["saved".into(), FINISHER.into()]);
        t.push("__EMUERA_1808_STRAT__".into());
        // character: str, int, str arrays, int arrays, str 2D, int 2D
        t.extend(["NICKNAME:Miko", SEPARATOR, SEPARATOR, "CSTR", "x", FINISHER, SEPARATOR, "TCVAR", "5", FINISHER, SEPARATOR, SEPARATOR, "CDFLAG", "1,2", "", "3", FINISHER, SEPARATOR].map(String::from));
        // globals: str, int, str arr, int arr, str2D, int2D, str3D, int3D
        t.extend([SEPARATOR, SEPARATOR, "TSTR", "t0", FINISHER, SEPARATOR, SEPARATOR, SEPARATOR, "DA", "4,5", FINISHER, SEPARATOR, SEPARATOR, "TA", "0{", "1,2", "}", "1{", "}", FINISHER, SEPARATOR].map(String::from));
        // user SAVEDATA: str, int, str2D, int2D, str3D, int3D
        t.extend([SEPARATOR, "MYVAR", "9", "8", FINISHER, SEPARATOR, SEPARATOR, SEPARATOR, SEPARATOR, SEPARATOR].map(String::from));
        t.join("\r\n")
    }

    #[test]
    fn reads_eramaker_and_emuera_parts() {
        let b = sample();
        assert!(looks_like(b.as_bytes()));
        assert_eq!(header(b.as_bytes()).unwrap(), (123, 1000, "a comment".into()));
        let s = parse(b.as_bytes()).unwrap();
        let c = &s.charas[0];
        let get = |v: &[(String, Vals)], k: &str| v.iter().find(|x| x.0 == k).map(|x| x.1.clone());
        assert_eq!(get(c, "NAME"), Some(Vals::S(vec![1], vec!["Reimu".into()])));
        assert_eq!(get(c, "BASE"), Some(Vals::I(vec![2], vec![100, 200])));
        assert_eq!(get(c, "NICKNAME"), Some(Vals::S(vec![1], vec!["Miko".into()])));
        assert_eq!(get(c, "TCVAR"), Some(Vals::I(vec![1], vec![5])));
        assert_eq!(get(c, "CDFLAG"), Some(Vals::I(vec![3, 2], vec![1, 2, 0, 0, 3, 0])));
        assert_eq!(get(&s.vars, "DAY"), Some(Vals::I(vec![1], vec![7])));
        assert_eq!(get(&s.vars, "SAVESTR"), Some(Vals::S(vec![1], vec!["saved".into()])));
        assert_eq!(get(&s.vars, "TSTR"), Some(Vals::S(vec![1], vec!["t0".into()])));
        assert_eq!(get(&s.vars, "DA"), Some(Vals::I(vec![1, 2], vec![4, 5])));
        assert_eq!(get(&s.vars, "TA"), Some(Vals::I(vec![2, 1, 2], vec![1, 2, 0, 0])));
        assert_eq!(get(&s.vars, "MYVAR"), Some(Vals::I(vec![2], vec![9, 8])));
        assert!(!looks_like(b"\x00\x01binary"));
    }
}
