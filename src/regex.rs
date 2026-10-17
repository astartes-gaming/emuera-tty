//! Regular expressions with .NET's behaviour, for REPLACE, REGEXPMATCH and
//! FINDELEMENT (Emuera hands their patterns to `System.Text.RegularExpressions`).
//! `fancy_regex` covers what Emuera scripts use beyond the `regex` crate:
//! lookaround, back-references, atomic groups. Compiled patterns are cached
//! like Emuera's `RegexFactory`.

use fancy_regex::{Captures, Regex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<Regex>>> = RefCell::new(HashMap::new());
}

/// Compile (or fetch) a pattern.
pub fn get(pat: &str) -> Result<Rc<Regex>, String> {
    if let Some(r) = CACHE.with(|c| c.borrow().get(pat).cloned()) {
        return Ok(r);
    }
    let r = Rc::new(Regex::new(&dotnet_pattern(pat)).map_err(|e| format!("bad regular expression {:?}: {}", pat, e))?);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(pat.to_string(), r.clone());
    });
    Ok(r)
}

/// .NET syntax the Rust engines spell differently: named groups
/// `(?<name>` / `(?'name'` become `(?P<name>`, and `\k<name>` stays.
fn dotnet_pattern(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    let c: Vec<char> = p.chars().collect();
    let mut i = 0;
    let mut in_class = false;
    while i < c.len() {
        if c[i] == '\\' && i + 1 < c.len() {
            out.push(c[i]);
            out.push(c[i + 1]);
            i += 2;
            continue;
        }
        if in_class {
            if c[i] == ']' {
                in_class = false;
            }
            out.push(c[i]);
            i += 1;
            continue;
        }
        if c[i] == '[' {
            in_class = true;
            out.push('[');
            i += 1;
            // a leading ] (or ^]) is a literal
            if c.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            if c.get(i) == Some(&']') {
                out.push_str("\\]");
                i += 1;
            }
            continue;
        }
        if c[i] == '(' && c.get(i + 1) == Some(&'?') {
            // (?<name>…) but not the lookbehinds (?<= / (?<!
            if c.get(i + 2) == Some(&'<') && !matches!(c.get(i + 3), Some('=') | Some('!')) {
                out.push_str("(?P<");
                i += 3;
                continue;
            }
            if c.get(i + 2) == Some(&'\'') {
                out.push_str("(?P<");
                i += 3;
                while i < c.len() && c[i] != '\'' {
                    out.push(c[i]);
                    i += 1;
                }
                out.push('>');
                i += 1;
                continue;
            }
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Expand a .NET replacement string for one match: `$1`, `${name}`, `$&`
/// (whole match), `` $` `` / `$'` (before / after it), `$+` (last group),
/// `$_` (the whole input), `$$` (a dollar sign).
fn expand(rep: &str, caps: &Captures<'_, str>, input: &str) -> String {
    let mut out = String::new();
    let c: Vec<char> = rep.chars().collect();
    let whole = caps.get(0).unwrap();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '$' || i + 1 >= c.len() {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let n = c[i + 1];
        match n {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(whole.as_str());
                i += 2;
            }
            '`' => {
                out.push_str(&input[..whole.start()]);
                i += 2;
            }
            '\'' => {
                out.push_str(&input[whole.end()..]);
                i += 2;
            }
            '_' => {
                out.push_str(input);
                i += 2;
            }
            '+' => {
                if let Some(m) = (1..caps.len()).rev().find_map(|k| caps.get(k)) {
                    out.push_str(m.as_str());
                }
                i += 2;
            }
            '{' => match c[i + 2..].iter().position(|&x| x == '}') {
                Some(e) => {
                    let name: String = c[i + 2..i + 2 + e].iter().collect();
                    let m = match name.parse::<usize>() {
                        Ok(k) => caps.get(k),
                        Err(_) => caps.name(&name),
                    };
                    if let Some(m) = m {
                        out.push_str(m.as_str());
                    }
                    i += 3 + e;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            },
            d if d.is_ascii_digit() => {
                // the longest group number that exists, like .NET
                let mut j = i + 1;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                let mut k = j;
                loop {
                    let num: String = c[i + 1..k].iter().collect();
                    if let Ok(g) = num.parse::<usize>() {
                        if g < caps.len() {
                            if let Some(m) = caps.get(g) {
                                out.push_str(m.as_str());
                            }
                            i = k;
                            break;
                        }
                    }
                    k -= 1;
                    if k == i + 1 {
                        // no such group: the text stays as written
                        out.push('$');
                        i += 1;
                        break;
                    }
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// `Regex.Replace(input, replacement)`.
pub fn replace(re: &Regex, input: &str, rep: &str) -> String {
    replace_with(re, input, |caps| expand(rep, caps, input))
}

/// Replace every match with what `f` returns for it.
pub fn replace_with(re: &Regex, input: &str, mut f: impl FnMut(&Captures<'_, str>) -> String) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    for caps in re.captures_iter(input) {
        let Ok(caps) = caps else { break };
        let m = caps.get(0).unwrap();
        out.push_str(&input[last..m.start()]);
        out.push_str(&f(&caps));
        last = m.end();
    }
    out.push_str(&input[last..]);
    out
}

pub fn is_match(re: &Regex, input: &str) -> bool {
    re.is_match(input).unwrap_or(false)
}

/// Every match's groups in .NET's `GetGroupNames()` order: the numbered
/// groups (0 = the whole match) and then the named ones. Returns
/// (number of groups, the values of all matches one after another).
pub fn match_groups(re: &Regex, input: &str) -> (usize, Vec<String>) {
    let names: Vec<Option<&str>> = re.capture_names().collect();
    // .NET numbers unnamed groups first, then named ones
    let mut order: Vec<usize> = (0..names.len()).filter(|&k| names[k].is_none()).collect();
    order.extend((0..names.len()).filter(|&k| names[k].is_some()));
    let mut vals = Vec::new();
    for caps in re.captures_iter(input) {
        let Ok(caps) = caps else { break };
        for &k in &order {
            vals.push(caps.get(k).map(|m| m.as_str().to_string()).unwrap_or_default());
        }
    }
    (names.len(), vals)
}

pub fn count(re: &Regex, input: &str) -> usize {
    re.find_iter(input).filter(|m| m.is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotnet_replace() {
        // pops-tw's word wrap: BREAKENG
        let re = get(r"(.{1,10})(?:\s|$)").unwrap();
        assert_eq!(replace(&re, "the quick brown fox jumps", "$1<br>"), "the quick<br>brown fox<br>jumps<br>");
        let re = get(r"(?<w>\w+)@(\d+)").unwrap();
        assert_eq!(replace(&re, "a@1 b@22", "${w}=$2;$$"), "a=1;$ b=22;$");
        assert_eq!(replace(&re, "a@1", "[$&]"), "[a@1]");
        // $12 with only two groups: group 1, then "2"
        let re = get(r"(a)(b)").unwrap();
        assert_eq!(replace(&re, "ab", "$12"), "a2");
        assert_eq!(replace(&re, "ab", "$9"), "$9");
        // lookbehind / lookahead
        let re = get(r"(?<=x)\d+(?=y)").unwrap();
        assert_eq!(replace(&re, "x12y x34z", "N"), "xNy x34z");
    }

    #[test]
    fn groups_and_counts() {
        let re = get(r"(?<k>\w)=(\d)").unwrap();
        assert_eq!(count(&re, "a=1 b=2 c"), 2);
        // .NET order: 0, the unnamed (2nd) group, then the named one
        assert_eq!(match_groups(&re, "a=1 b=2"), (3, vec!["a=1".into(), "1".into(), "a".into(), "b=2".into(), "2".into(), "b".into()]));
        assert!(is_match(&get("^[]a]+$").unwrap(), "a]a"));
    }
}
