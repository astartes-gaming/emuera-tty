//! Emuera's keyboard macros.
//!
//! - Typed input is expanded as Emuera's console does (`EmueraConsole`
//!   input, `parseInput`): `\n` (or a real line break) separates inputs
//!   given one after another, `\e` skips messages after its input (as a
//!   right click), `(…)*n` repeats. `"(0\e\n)*3"` is `0`, `0`, `0`, each
//!   with skipping. Clicks aren't expanded.
//! - With the config's "Use keyboard macros" on, Shift+F1…F12 store the
//!   line being typed under that key and F1…F12 put it back; ten groups
//!   of twelve. They are kept in the game folder's `macro.txt`, written
//!   as Emuera writes it (`KeyMacro.cs`), so the same file works in both.

use std::path::Path;

/// `input` as the inputs it stands for, in order, each with whether it
/// starts message skipping (`\e`).
pub fn expand(input: &str) -> Vec<(String, bool)> {
    let s = if input.contains('(') {
        let ch: Vec<char> = input.chars().collect();
        parse(&ch, &mut 0, false)
    } else {
        input.to_string()
    };
    split(&s)
        .into_iter()
        .map(|p| match p.contains("\\e") {
            true => (p.replace("\\e", ""), true),
            false => (p, false),
        })
        .collect()
}

/// .NET's `Split` on `\n` (the two characters), CRLF, LF and CR, tried in
/// that order at each position.
fn split(s: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let sep = ["\\n", "\r\n", "\n", "\r"].into_iter().find(|p| rest.starts_with(p));
        match sep {
            Some(p) => {
                out.push(String::new());
                rest = &rest[p.len()..];
            }
            None => {
                out.last_mut().unwrap().push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// Emuera's `parseInput`: `(…)*n` repeated, `\n` / `\r` / `\e` and other
/// `\x` escapes resolved (`\e` ends its input: the next `\n` is dropped).
fn parse(ch: &[char], i: &mut usize, nest: bool) -> String {
    let mut sb = String::new();
    let mut has_ret = false;
    while *i < ch.len() && (!nest || ch[*i] != ')') {
        if ch[*i] == '(' {
            *i += 1;
            let t = parse(ch, i, true);
            if *i >= ch.len() {
                sb.push_str(&t);
                break;
            }
            // past the `)`
            *i += 1;
            if ch.get(*i) == Some(&'*') {
                *i += 1;
                let mut num = String::new();
                while *i < ch.len() && ch[*i].is_numeric() {
                    num.push(ch[*i]);
                    *i += 1;
                }
                // (int.TryParse: other digits than ASCII give 0)
                for _ in 0..num.parse::<u32>().unwrap_or(0) {
                    sb.push_str(&t);
                }
            } else {
                sb.push_str(&t);
            }
            continue;
        } else if ch[*i] == '\\' {
            *i += 1;
            match ch.get(*i) {
                Some('n') if has_ret => has_ret = false,
                Some('n') => sb.push('\n'),
                Some('r') => sb.push('\r'),
                Some('e') => {
                    sb.push_str("\\e\n");
                    has_ret = true;
                }
                Some('\n') | None => {}
                Some(&c) => sb.push(c),
            }
        } else {
            sb.push(ch[*i]);
        }
        *i += 1;
    }
    sb
}

/// Groups, and keys per group.
pub const GROUPS: usize = 10;
pub const KEYS: usize = 12;
/// The group lines' label, the same in every language.
const GROUP_ID: &str = "グループ";
/// The keys' labels and the groups' default names in the languages of
/// Emuera's UI (`Lang.KeyMacro`, Properties/lang/emuera-*.xml): Emuera
/// writes and reads them in its UI's language only.
const LANGS: [(&str, &str); 3] = [("マクロキーF", "マクログループ{}に設定"), ("Macro Key F", "Set to macro group {}"), ("宏快捷键F", "设置为按键宏组{}")];

fn key_label(lang: usize, group: usize, key: usize) -> String {
    match group {
        0 => format!("{}{}:", LANGS[lang].0, key + 1),
        g => format!("G{}:{}{}:", g, LANGS[lang].0, key + 1),
    }
}

/// The stored macros (`macro.txt`).
pub struct Macros {
    keys: Vec<String>,
    groups: Vec<String>,
    changed: bool,
    /// the labels' language (`LANGS`): the file's, so the Emuera that
    /// wrote it still reads it
    lang: usize,
    /// the group F1…F12 use now
    pub group: usize,
}

impl Default for Macros {
    fn default() -> Macros {
        Macros {
            keys: vec![String::new(); GROUPS * KEYS],
            groups: (0..GROUPS).map(|g| LANGS[0].1.replace("{}", &g.to_string())).collect(),
            changed: false,
            lang: 0,
            group: 0,
        }
    }
}

impl Macros {
    /// Read `macro.txt` as Emuera's `LoadMacroFile` does: `;` lines are
    /// comments, `グループN:` names group N, `マクロキーFn:` / `Gg:マクロキーFn:`
    /// give a key's macro, here in any of Emuera's UI languages. A missing
    /// file means no macros.
    pub fn load(path: &Path) -> Macros {
        let mut m = Macros::default();
        let Ok(text) = crate::loader::read_text(path) else { return m };
        for line in text.lines() {
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if let Some(rest) = line.strip_prefix(GROUP_ID) {
                let c: Vec<char> = rest.chars().take(2).collect();
                if line.chars().count() >= GROUP_ID.chars().count() + 4 && c.len() == 2 && c[1] == ':' {
                    if let Some(n) = c[0].to_digit(10) {
                        m.groups[n as usize] = rest.chars().skip(2).collect();
                    }
                }
            }
            'langs: for lang in 0..LANGS.len() {
                for (i, slot) in m.keys.iter_mut().enumerate() {
                    if let Some(v) = line.strip_prefix(&key_label(lang, i / KEYS, i % KEYS)) {
                        *slot = v.to_string();
                        m.lang = lang;
                        break 'langs;
                    }
                }
            }
        }
        m
    }

    /// Write `macro.txt` if a macro was stored since it was read (Emuera's
    /// `SaveMacro`: UTF-8 with a BOM, the group names, then every key).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        let mut s = String::from("\u{feff}");
        for (g, name) in self.groups.iter().enumerate() {
            s.push_str(&format!("{}{}:{}\r\n", GROUP_ID, g, name));
        }
        for (i, v) in self.keys.iter().enumerate() {
            s.push_str(&format!("{}{}\r\n", key_label(self.lang, i / KEYS, i % KEYS), v));
        }
        std::fs::write(path, s)
    }

    /// The macro under F`key + 1` in the current group.
    pub fn get(&self, key: usize) -> &str {
        &self.keys[self.group * KEYS + key]
    }

    /// Store `text` under F`key + 1` in the current group.
    pub fn set(&mut self, key: usize, text: &str) {
        self.keys[self.group * KEYS + key] = text.to_string();
        self.changed = true;
    }

    /// The current group's name, shown when it is chosen.
    pub fn group_name(&self) -> &str {
        &self.groups[self.group]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(s: &str) -> Vec<(String, bool)> {
        expand(s)
    }
    fn v(items: &[(&str, bool)]) -> Vec<(String, bool)> {
        items.iter().map(|(s, b)| (s.to_string(), *b)).collect()
    }

    /// The examples in Emuera's manual (usage.md, Macros).
    #[test]
    fn manual_examples() {
        assert_eq!(ex("0\\e\\n1\\e\\n"), v(&[("0", true), ("1", true), ("", false)]));
        assert_eq!(ex("(0\\e\\n)*3"), ex("0\\e\\n0\\e\\n0\\e\\n"));
        assert_eq!(ex("(0\\e\\n)*3"), v(&[("0", true), ("0", true), ("0", true), ("", false)]));
    }

    #[test]
    fn plain_and_edge_cases() {
        assert_eq!(ex("123"), v(&[("123", false)]));
        assert_eq!(ex(""), v(&[("", false)]));
        assert_eq!(ex("1\\n2"), v(&[("1", false), ("2", false)]));
        // without a `(`, `\x` escapes other than \n / \e stay as typed
        assert_eq!(ex("a\\xb"), v(&[("a\\xb", false)]));
        // with one, they are resolved; `(x)` without `*n` is x once
        assert_eq!(ex("(a)\\xb"), v(&[("axb", false)]));
        assert_eq!(ex("(ab)*2\\n(c)*0"), v(&[("abab", false), ("", false)]));
        // an unclosed `(` keeps what it holds
        assert_eq!(ex("(ab"), v(&[("ab", false)]));
    }

    /// macro.txt round trip, in Emuera's format.
    #[test]
    fn macro_file() {
        let d = std::env::temp_dir().join(format!("emuera-macro-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("macro.txt");
        let mut m = Macros::load(&p);
        assert_eq!(m.get(0), "");
        m.set(0, "0\\e\\n");
        m.group = 1;
        m.set(11, "(1\\n)*5");
        m.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("\u{feff}グループ0:マクログループ0に設定\r\n"), "{:?}", &text[..40]);
        assert!(text.contains("\r\nマクロキーF1:0\\e\\n\r\n") && text.contains("\r\nG1:マクロキーF12:(1\\n)*5\r\n"), "{}", text);
        let mut m = Macros::load(&p);
        assert_eq!(m.get(0), "0\\e\\n");
        m.group = 1;
        assert_eq!((m.get(11), m.group_name()), ("(1\\n)*5", "マクログループ1に設定"));
        // one an English Emuera wrote is read, and written back in English
        std::fs::write(&p, "\u{feff}グループ0:Set to macro group 0\r\nMacro Key F1:abc\r\nG9:Macro Key F12:xyz\r\n").unwrap();
        let mut m = Macros::load(&p);
        assert_eq!((m.get(0), m.group_name()), ("abc", "Set to macro group 0"));
        m.group = 9;
        assert_eq!(m.get(11), "xyz");
        m.set(0, "q");
        m.save(&p).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\r\nMacro Key F1:abc\r\n") && text.contains("\r\nG9:Macro Key F1:q\r\n"), "{}", text);
        let _ = std::fs::remove_dir_all(&d);
    }
}
