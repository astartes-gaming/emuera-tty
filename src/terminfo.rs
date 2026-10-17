//! A small terminfo reader for the retro frontend: loads a compiled entry
//! (`/usr/share/terminfo/v/vt52` …, both the 16- and 32-bit number
//! formats), and expands parameterised strings (`tparm`) and `$<n>` delays,
//! so any terminal the system describes — VT52, ADM-3A, Wyse, VT100, a
//! Teletype — can be driven without ncurses.

// capability indices are listed whether or not the frontend uses them yet
#![allow(dead_code)]

use std::path::PathBuf;

// capability indices, from <term.h>
pub const B_AM: usize = 1; // auto_right_margin
pub const B_XENL: usize = 4; // eat_newline_glitch
pub const B_GN: usize = 6; // generic_type
pub const B_HC: usize = 7; // hard_copy
pub const B_OS: usize = 15; // over_strike
pub const B_NPC: usize = 25; // no_pad_char
pub const N_COLS: usize = 0;
pub const N_LINES: usize = 2;
pub const N_XMC: usize = 4; // magic_cookie_glitch
pub const N_PB: usize = 5; // padding_baud_rate
pub const N_COLORS: usize = 13;
pub const S_BEL: usize = 1;
pub const S_CR: usize = 2;
pub const S_CLEAR: usize = 5;
pub const S_EL: usize = 6;
pub const S_ED: usize = 7;
pub const S_CUP: usize = 10;
pub const S_CUD1: usize = 11;
pub const S_HOME: usize = 12;
pub const S_CIVIS: usize = 13;
pub const S_CUB1: usize = 14;
pub const S_CNORM: usize = 16;
pub const S_CUF1: usize = 17;
pub const S_CUU1: usize = 19;
pub const S_SMACS: usize = 25;
pub const S_BOLD: usize = 27;
pub const S_SMCUP: usize = 28;
pub const S_DIM: usize = 30;
pub const S_REV: usize = 34;
pub const S_SMSO: usize = 35;
pub const S_SMUL: usize = 36;
pub const S_RMACS: usize = 38;
pub const S_SGR0: usize = 39;
pub const S_RMCUP: usize = 40;
pub const S_RMSO: usize = 43;
pub const S_RMUL: usize = 44;
pub const S_IS2: usize = 49;
pub const S_KBS: usize = 55;
pub const S_KDCH1: usize = 59;
pub const S_KCUD1: usize = 61;
pub const S_KHOME: usize = 76;
pub const S_KCUB1: usize = 79;
pub const S_KNP: usize = 81;
pub const S_KPP: usize = 82;
pub const S_KCUF1: usize = 83;
pub const S_KCUU1: usize = 87;
pub const S_RMKX: usize = 88;
pub const S_SMKX: usize = 89;
pub const S_NEL: usize = 103;
pub const S_PAD: usize = 104;
pub const S_ACSC: usize = 146;
pub const S_ENACS: usize = 155;
pub const S_KEND: usize = 164;
pub const S_KENT: usize = 165;
pub const S_OP: usize = 297;
pub const S_SETF: usize = 302;
pub const S_SETAF: usize = 359;

#[derive(Clone, Debug, Default)]
pub struct TermInfo {
    pub names: String,
    bools: Vec<bool>,
    nums: Vec<i32>,
    strs: Vec<Option<Vec<u8>>>,
}

impl TermInfo {
    /// Find and load the entry for `name` the way ncurses does:
    /// `$TERMINFO`, `~/.terminfo`, `$TERMINFO_DIRS`, then the system dirs.
    pub fn load(name: &str) -> Result<TermInfo, String> {
        if name.is_empty() || name.contains('/') {
            return Err(format!("bad terminal name {:?}", name));
        }
        let first = name.chars().next().unwrap();
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Ok(d) = std::env::var("TERMINFO") {
            dirs.push(d.into());
        }
        if let Ok(h) = std::env::var("HOME") {
            dirs.push(PathBuf::from(h).join(".terminfo"));
        }
        if let Ok(ds) = std::env::var("TERMINFO_DIRS") {
            dirs.extend(ds.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
        }
        for d in ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo", "/usr/lib/terminfo", "/usr/share/lib/terminfo"] {
            dirs.push(d.into());
        }
        for d in dirs {
            // the usual first-letter directory, or its hex code (macOS)
            for sub in [first.to_string(), format!("{:02x}", first as u32)] {
                let p = d.join(sub).join(name);
                if let Ok(b) = std::fs::read(&p) {
                    return TermInfo::parse(&b).map_err(|e| format!("{}: {}", p.display(), e));
                }
            }
        }
        Err(format!("no terminfo entry for {:?} (try `infocmp {}`; the ncurses-term package has most old terminals)", name, name))
    }

    /// Decode a compiled entry (term(5)).
    pub fn parse(b: &[u8]) -> Result<TermInfo, String> {
        let short = |i: usize| -> Result<i32, String> {
            b.get(i..i + 2).map(|s| i16::from_le_bytes([s[0], s[1]]) as i32).ok_or_else(|| "truncated".to_string())
        };
        let magic = short(0)?;
        let wide = match magic {
            0o432 => false,
            0o1036 => true,
            _ => return Err("not a compiled terminfo entry".into()),
        };
        let (name_sz, n_bools, n_nums, n_strs, tab_sz) = (short(2)? as usize, short(4)? as usize, short(6)? as usize, short(8)? as usize, short(10)? as usize);
        let mut p = 12;
        let names = String::from_utf8_lossy(b.get(p..p + name_sz).ok_or("truncated")?).trim_end_matches('\0').to_string();
        p += name_sz;
        let bools = b.get(p..p + n_bools).ok_or("truncated")?.iter().map(|&x| x == 1).collect();
        p += n_bools;
        if p % 2 == 1 {
            p += 1;
        }
        let mut nums = Vec::with_capacity(n_nums);
        for _ in 0..n_nums {
            if wide {
                let s = b.get(p..p + 4).ok_or("truncated")?;
                nums.push(i32::from_le_bytes([s[0], s[1], s[2], s[3]]));
                p += 4;
            } else {
                nums.push(short(p)?);
                p += 2;
            }
        }
        let offs_at = p;
        let table = offs_at + n_strs * 2;
        let tab = b.get(table..table + tab_sz).ok_or("truncated")?;
        let mut strs = Vec::with_capacity(n_strs);
        for k in 0..n_strs {
            let off = short(offs_at + k * 2)?;
            if off < 0 || off as usize >= tab.len() {
                strs.push(None);
                continue;
            }
            let s = &tab[off as usize..];
            let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            strs.push(Some(s[..end].to_vec()));
        }
        Ok(TermInfo { names, bools, nums, strs })
    }

    pub fn flag(&self, i: usize) -> bool {
        self.bools.get(i).copied().unwrap_or(false)
    }

    /// A number, if present (absent / cancelled are negative in the file).
    pub fn num(&self, i: usize) -> Option<i32> {
        self.nums.get(i).copied().filter(|&n| n >= 0)
    }

    pub fn str(&self, i: usize) -> Option<&[u8]> {
        self.strs.get(i).and_then(|s| s.as_deref()).filter(|s| !s.is_empty())
    }

    /// The first name (`vt52`, `adm3a`…).
    pub fn name(&self) -> &str {
        self.names.split('|').next().unwrap_or("")
    }
}

/// Expand a parameterised string (terminfo's `%` language: `%p1 %d %c
/// %+ %i %{n} %'c' %? %t %e %;` …). `$<n>` delays are left in place for
/// [`pad`].
pub fn tparm(s: &[u8], params: &[i32]) -> Vec<u8> {
    #[derive(Clone)]
    enum V {
        I(i32),
        S(Vec<u8>),
    }
    let as_int = |v: Option<V>| match v {
        Some(V::I(i)) => i,
        Some(V::S(s)) => s.len() as i32,
        None => 0,
    };
    let mut p: [i32; 9] = [0; 9];
    for (k, v) in params.iter().take(9).enumerate() {
        p[k] = *v;
    }
    let mut out = Vec::new();
    let mut stack: Vec<V> = Vec::new();
    let mut dyn_vars = [0i32; 26];
    let mut static_vars = [0i32; 26];
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let Some(&op) = s.get(i) else { break };
        i += 1;
        match op {
            b'%' => out.push(b'%'),
            b'c' => {
                let v = as_int(stack.pop());
                // a NUL can't be sent through most drivers: ncurses sends 0x80
                out.push(if v == 0 { 0x80 } else { v as u8 });
            }
            b's' => match stack.pop() {
                Some(V::S(t)) => out.extend(t),
                Some(V::I(n)) => out.extend(n.to_string().bytes()),
                None => {}
            },
            b'p' => {
                let k = s.get(i).map(|d| d.wrapping_sub(b'1')).unwrap_or(0) as usize;
                i += 1;
                stack.push(V::I(p.get(k).copied().unwrap_or(0)));
            }
            b'P' | b'g' => {
                let v = s.get(i).copied().unwrap_or(b'a');
                i += 1;
                let slot = |v: u8| -> Option<(bool, usize)> {
                    if v.is_ascii_lowercase() {
                        Some((true, (v - b'a') as usize))
                    } else if v.is_ascii_uppercase() {
                        Some((false, (v - b'A') as usize))
                    } else {
                        None
                    }
                };
                if let Some((d, k)) = slot(v) {
                    if op == b'P' {
                        let x = as_int(stack.pop());
                        if d {
                            dyn_vars[k] = x;
                        } else {
                            static_vars[k] = x;
                        }
                    } else {
                        stack.push(V::I(if d { dyn_vars[k] } else { static_vars[k] }));
                    }
                }
            }
            b'\'' => {
                let ch = s.get(i).copied().unwrap_or(0);
                stack.push(V::I(ch as i32));
                i += 2; // the char and the closing quote
            }
            b'{' => {
                let mut n: i32 = 0;
                while let Some(&d) = s.get(i) {
                    i += 1;
                    if d == b'}' {
                        break;
                    }
                    if d.is_ascii_digit() {
                        n = n * 10 + (d - b'0') as i32;
                    }
                }
                stack.push(V::I(n));
            }
            b'l' => {
                let n = match stack.pop() {
                    Some(V::S(t)) => t.len() as i32,
                    v => as_int(v).to_string().len() as i32,
                };
                stack.push(V::I(n));
            }
            b'+' | b'-' | b'*' | b'/' | b'm' | b'&' | b'|' | b'^' | b'=' | b'<' | b'>' | b'A' | b'O' => {
                let b2 = as_int(stack.pop());
                let a2 = as_int(stack.pop());
                let r = match op {
                    b'+' => a2.wrapping_add(b2),
                    b'-' => a2.wrapping_sub(b2),
                    b'*' => a2.wrapping_mul(b2),
                    b'/' => if b2 == 0 { 0 } else { a2 / b2 },
                    b'm' => if b2 == 0 { 0 } else { a2 % b2 },
                    b'&' => a2 & b2,
                    b'|' => a2 | b2,
                    b'^' => a2 ^ b2,
                    b'=' => (a2 == b2) as i32,
                    b'<' => (a2 < b2) as i32,
                    b'>' => (a2 > b2) as i32,
                    b'A' => (a2 != 0 && b2 != 0) as i32,
                    _ => (a2 != 0 || b2 != 0) as i32,
                };
                stack.push(V::I(r));
            }
            b'!' => {
                let v = as_int(stack.pop());
                stack.push(V::I((v == 0) as i32));
            }
            b'~' => {
                let v = as_int(stack.pop());
                stack.push(V::I(!v));
            }
            b'i' => {
                p[0] += 1;
                p[1] += 1;
            }
            b'?' | b';' => {}
            b't' => {
                // false: skip to the matching %e or %;
                if as_int(stack.pop()) == 0 {
                    i = skip_cond(s, i, true);
                }
            }
            b'e' => {
                // reached the end of a taken branch: skip the else part
                i = skip_cond(s, i, false);
            }
            _ => {
                // %[flags][width][.prec](d|o|x|X|s)
                let mut j = i - 1;
                let mut spec = Vec::new();
                while let Some(&d) = s.get(j) {
                    if b"doxXs".contains(&d) {
                        break;
                    }
                    spec.push(d);
                    j += 1;
                }
                let conv = s.get(j).copied().unwrap_or(b'd');
                i = j + 1;
                let spec = String::from_utf8_lossy(&spec).trim_start_matches(':').to_string();
                let (zero, left) = (spec.starts_with('0'), spec.contains('-'));
                let digits: String = spec.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
                let (w, prec) = match digits.split_once('.') {
                    Some((w, pr)) => (w.parse().unwrap_or(0), pr.parse::<usize>().ok()),
                    None => (digits.parse().unwrap_or(0), None),
                };
                let body = if conv == b's' {
                    match stack.pop() {
                        Some(V::S(t)) => String::from_utf8_lossy(&t).to_string(),
                        v => as_int(v).to_string(),
                    }
                } else {
                    let v = as_int(stack.pop());
                    let mut t = match conv {
                        b'o' => format!("{:o}", v),
                        b'x' => format!("{:x}", v),
                        b'X' => format!("{:X}", v),
                        _ => v.to_string(),
                    };
                    if let Some(pr) = prec {
                        while t.len() < pr {
                            t.insert(0, '0');
                        }
                    }
                    t
                };
                let padded = if body.len() >= w {
                    body
                } else if left {
                    format!("{:<w$}", body, w = w)
                } else if zero {
                    format!("{:0>w$}", body, w = w)
                } else {
                    format!("{:>w$}", body, w = w)
                };
                out.extend(padded.bytes());
            }
        }
    }
    out
}

/// Skip a conditional branch starting at `i`: to just past the matching
/// `%e` (when `to_else`) or `%;`, minding nested `%?`.
fn skip_cond(s: &[u8], mut i: usize, to_else: bool) -> usize {
    let mut depth = 0;
    while i + 1 < s.len() {
        if s[i] == b'%' {
            match s[i + 1] {
                b'?' => depth += 1,
                b';' if depth == 0 => return i + 2,
                b';' => depth -= 1,
                b'e' if depth == 0 && to_else => return i + 2,
                _ => {}
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    s.len()
}

/// Replace `$<n>` / `$<n*>` / `$<n/>` delays with pad characters for a
/// line running at `baud` (`affected` lines for `*`). Below the entry's
/// padding_baud_rate, or with no_pad_char, delays are dropped.
pub fn pad(s: &[u8], baud: u32, ti: &TermInfo, affected: u32) -> Vec<u8> {
    let use_pad = !ti.flag(B_NPC) && ti.num(N_PB).map_or(true, |pb| baud >= pb as u32);
    let pad_char = ti.str(S_PAD).and_then(|p| p.first().copied()).unwrap_or(0);
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'$' && s.get(i + 1) == Some(&b'<') {
            if let Some(end) = s[i..].iter().position(|&c| c == b'>') {
                let spec = String::from_utf8_lossy(&s[i + 2..i + end]).to_string();
                let per_line = spec.contains('*');
                let ms: f64 = spec.trim_end_matches(['*', '/']).trim_end_matches('*').parse().unwrap_or(0.0);
                if use_pad && ms > 0.0 {
                    let ms = if per_line { ms * affected.max(1) as f64 } else { ms };
                    // 10 bits per character on the wire
                    let n = (ms * baud as f64 / 10_000.0).ceil() as usize;
                    out.extend(std::iter::repeat(pad_char).take(n));
                }
                i += end + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_addressing_strings() {
        // VT100 / ANSI
        assert_eq!(tparm(b"\x1b[%i%p1%d;%p2%dH", &[4, 9]), b"\x1b[5;10H");
        // VT52 and ADM-3A: offset characters
        assert_eq!(tparm(b"\x1bY%p1%' '%+%c%p2%' '%+%c", &[0, 2]), b"\x1bY \"");
        assert_eq!(tparm(b"\x1b=%p1%' '%+%c%p2%' '%+%c", &[1, 1]), b"\x1b=!!");
        // conditionals (xterm-style colour)
        let setaf = b"\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m";
        assert_eq!(tparm(setaf, &[1]), b"\x1b[31m");
        assert_eq!(tparm(setaf, &[9]), b"\x1b[91m");
        assert_eq!(tparm(setaf, &[100]), b"\x1b[38;5;100m");
        // printf widths
        assert_eq!(tparm(b"%p1%02d|%p1%3d|%p1%x", &[7]), b"07|  7|7");
    }

    #[test]
    fn system_entries_load() {
        // skipped where the database isn't installed
        let Ok(vt) = TermInfo::load("vt100") else { return };
        assert_eq!(vt.num(N_COLS), Some(80));
        assert!(vt.flag(B_AM));
        assert_eq!(tparm(vt.str(S_CUP).unwrap(), &[0, 0]).starts_with(b"\x1b[1;1H"), true);
        if let Ok(adm) = TermInfo::load("adm3a") {
            assert!(adm.str(S_EL).is_none());
            assert_eq!(tparm(adm.str(S_CUP).unwrap(), &[2, 3]), b"\x1b=\"#");
        }
    }

    #[test]
    fn delays_become_padding() {
        let ti = TermInfo::default();
        // 5 ms at 9600 baud: 4.8 characters, rounded up
        assert_eq!(pad(b"\x1b[H$<5>", 9600, &ti, 1), b"\x1b[H\0\0\0\0\0");
        assert_eq!(pad(b"x$<2*>", 9600, &ti, 3), b"x\0\0\0\0\0\0");
    }
}
