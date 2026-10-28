//! A small JSON reader, for the launcher's downloader (the eraGames wiki's
//! MediaWiki API answers): objects, arrays, strings, numbers, true, false
//! and null. Nesting deeper than 64 levels is refused.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// members in the document's order
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn parse(s: &str) -> Option<Json> {
        let mut p = Parser { s, i: 0 };
        let v = p.value(0)?;
        p.ws();
        (p.i == s.len()).then_some(v)
    }

    /// An object's member `key`.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.members().iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// An array's items (none for anything else).
    pub fn items(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }

    /// An object's members (none for anything else).
    pub fn members(&self) -> &[(String, Json)] {
        match self {
            Json::Obj(m) => m,
            _ => &[],
        }
    }
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.as_bytes().get(self.i).is_some_and(|b| b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> Option<()> {
        self.s[self.i..].starts_with(lit).then(|| self.i += lit.len())
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        if depth > 64 {
            return None;
        }
        self.ws();
        match *self.s.as_bytes().get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.eat("}").is_some() {
                    return Some(Json::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.eat(":")?;
                    m.push((k, self.value(depth + 1)?));
                    self.ws();
                    if self.eat("}").is_some() {
                        return Some(Json::Obj(m));
                    }
                    self.eat(",")?;
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.eat("]").is_some() {
                    return Some(Json::Arr(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    if self.eat("]").is_some() {
                        return Some(Json::Arr(a));
                    }
                    self.eat(",")?;
                }
            }
            b'"' => self.string().map(Json::Str),
            b't' => self.eat("true").map(|_| Json::Bool(true)),
            b'f' => self.eat("false").map(|_| Json::Bool(false)),
            b'n' => self.eat("null").map(|_| Json::Null),
            _ => {
                let len = self.s[self.i..].find(|c: char| !matches!(c, '0'..='9' | '-' | '+' | '.' | 'e' | 'E')).unwrap_or(self.s.len() - self.i);
                let n = self.s[self.i..self.i + len].parse().ok()?;
                self.i += len;
                Some(Json::Num(n))
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = u32::from_str_radix(self.s.get(self.i..self.i + 4)?, 16).ok()?;
        self.i += 4;
        Some(h)
    }

    fn string(&mut self) -> Option<String> {
        self.eat("\"")?;
        let mut out = String::new();
        loop {
            // up to the next quote or backslash (both ASCII: char boundaries)
            let run = self.s[self.i..].find(['"', '\\'])?;
            out.push_str(&self.s[self.i..self.i + run]);
            self.i += run;
            if self.eat("\"").is_some() {
                return Some(out);
            }
            self.i += 1;
            let e = *self.s.as_bytes().get(self.i)?;
            self.i += 1;
            out.push(match e {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    let hi = self.hex4()?;
                    let c = if (0xd800..0xdc00).contains(&hi) && self.eat("\\u").is_some() {
                        let lo = self.hex4()?;
                        0x10000 + ((hi - 0xd800) << 10) + lo.wrapping_sub(0xdc00)
                    } else {
                        hi
                    };
                    char::from_u32(c).unwrap_or('\u{fffd}')
                }
                _ => return None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_documents() {
        let j = Json::parse(r#" {"a": [1, -2.5e1, true, null], "s": "x\"\\\/\n\u00e9\ud83d\ude00", "o": {}} "#).unwrap();
        assert_eq!(j.get("a").unwrap().items(), &[Json::Num(1.0), Json::Num(-25.0), Json::Bool(true), Json::Null]);
        assert_eq!(j.get("s").and_then(Json::str), Some("x\"\\/\né😀"));
        assert_eq!(j.get("o"), Some(&Json::Obj(Vec::new())));
        assert_eq!(Json::parse("[\"日本語\"]").unwrap().items()[0].str(), Some("日本語"));
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "\"\\x\"", "[1] 2", "tru"] {
            assert_eq!(Json::parse(bad), None, "{}", bad);
        }
        assert_eq!(Json::parse(&"[".repeat(100)), None);
    }
}
