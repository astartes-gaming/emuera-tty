//! emuera-tty's own interface text in the player's language.
//!
//! Every message the program itself shows (settings, launcher, save
//! manager, prompts, its own errors; not game text) goes through
//! [`t!`] / [`tc!`], with the English text as the key. Translations are
//! gettext `.po` catalogues in `i18n/`, compiled into the binary; only the
//! selected language is parsed, on first use. A message a catalogue lacks
//! (or leaves empty) shows in English.
//!
//! `.po` is the long-standing format for program messages: every
//! translation tool (Poedit, Weblate, Crowdin) reads it. emuera-tty needs
//! only `msgctxt` / `msgid` / `msgstr` (no plural forms: Chinese, Japanese
//! and Korean have none, and English plurals are two messages), so the
//! parser is here rather than a dependency.
//!
//! `i18n/emuera-tty.pot`, the template translators start from, is the list
//! of `t!` / `tc!` literals in `src/`; a test compares it with the source
//! and `UPDATE_POT=1 cargo test` rewrites it.

use std::collections::HashMap;
use std::sync::RwLock;

/// The languages emuera-tty has catalogues for: code, name in itself, the
/// catalogue (`include_str!`).
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", ""),
    ("ja", "日本語", include_str!("../i18n/ja.po")),
    ("zh-Hans", "简体中文", include_str!("../i18n/zh-Hans.po")),
    ("zh-Hant", "繁體中文", include_str!("../i18n/zh-Hant.po")),
    ("ko", "한국어", include_str!("../i18n/ko.po")),
];

/// Translations keyed by (context, English text).
type Catalog = HashMap<(String, String), String>;

/// The selected language and its parsed catalogue (parsed on first lookup).
struct Current {
    lang: String,
    cat: Option<Catalog>,
}

#[cfg(not(test))]
static CURRENT: RwLock<Current> = RwLock::new(Current { lang: String::new(), cat: None });

// (tests run in parallel threads: each picks its own language)
#[cfg(test)]
thread_local! {
    static CURRENT_T: &'static RwLock<Current> = Box::leak(Box::new(RwLock::new(Current { lang: String::new(), cat: None })));
}

/// The selection in force (per thread in tests).
fn current() -> &'static RwLock<Current> {
    #[cfg(not(test))]
    return &CURRENT;
    #[cfg(test)]
    return CURRENT_T.with(|c| *c);
}

/// Use `lang` (a code from [`LANGUAGES`]) from now on; anything else is
/// English. Its catalogue is parsed at the next lookup.
pub fn set_language(lang: &str) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    if c.lang != lang {
        c.lang = lang.to_string();
        c.cat = None;
    }
}

/// The language in use (a code from [`LANGUAGES`], or "" for English).
#[allow(dead_code)]
pub fn language() -> String {
    current().read().unwrap_or_else(|e| e.into_inner()).lang.clone()
}

/// Use `lang` with the catalogue `po` instead of the compiled-in one (tests).
#[cfg(test)]
pub fn install(lang: &str, po: &str) {
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    c.lang = lang.to_string();
    c.cat = Some(parse_po(po));
}

/// Choose the language at start-up or when the setting changes:
/// `EMUERA_LANG` if set, else the setting (`auto` = the system's locale).
pub fn select(setting: &str) {
    let env = std::env::var("EMUERA_LANG").unwrap_or_default();
    set_language(&choose(&env, setting, &system_locale()));
}

/// The language code for `EMUERA_LANG` = `env`, the setting and the
/// system locale, in that order of precedence.
fn choose(env: &str, setting: &str, locale: &str) -> String {
    // (the pseudo-languages only from the variable: they test layout)
    if PSEUDO.contains(&env) {
        return env.to_string();
    }
    for want in [env, if setting == "auto" { "" } else { setting }, locale] {
        if let Some(code) = from_locale(want) {
            return code.to_string();
        }
    }
    "en".to_string()
}

/// A language code from [`LANGUAGES`] for a locale or language name
/// (`ja_JP.UTF-8`, `zh_TW`, `zh-Hant`, `ko`), or None (`C`, `POSIX`,
/// empty, or a language without a catalogue). Chinese goes by script:
/// Taiwan, Hong Kong and Macau, or an explicit `Hant`, are Traditional.
fn from_locale(name: &str) -> Option<&'static str> {
    // `ja_JP.UTF-8@euro` → `ja_JP`; `zh-Hant-TW` and `zh_TW` alike
    let base = name.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let mut parts = base.split('-');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let rest: Vec<String> = parts.map(|p| p.to_ascii_lowercase()).collect();
    let code = match lang.as_str() {
        "en" => "en",
        "ja" => "ja",
        "ko" => "ko",
        "zh" => {
            if rest.iter().any(|p| matches!(p.as_str(), "hant" | "tw" | "hk" | "mo")) {
                "zh-Hant"
            } else {
                "zh-Hans"
            }
        }
        _ => return None,
    };
    Some(code)
}

/// The system's language: `LC_ALL`, `LC_MESSAGES`, then `LANG` (the
/// POSIX order) on Unix and Termux, the user's locale on Windows.
fn system_locale() -> String {
    for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        match std::env::var(k) {
            Ok(v) if !v.is_empty() => return v,
            _ => {}
        }
    }
    windows_locale().unwrap_or_default()
}

#[cfg(windows)]
fn windows_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH (85) wide characters
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
}

#[cfg(not(windows))]
fn windows_locale() -> Option<String> {
    None
}

/// Pseudo-languages for checking layout without translations
/// (`EMUERA_LANG=x-cjk` …): every message rewritten in another script and
/// made about 40% longer, as real translations tend to be.
pub const PSEUDO: &[&str] = &["x-cjk", "x-arab", "x-cyrl", "x-deva", "x-avst"];

/// `text` in pseudo-language `lang`: letters replaced by the script's (CJK
/// ideographs two columns wide; Arabic and Old Avestan right to left;
/// Devanagari syllables with combining vowel signs and virama; Old Avestan
/// beyond the BMP), two more letters after every five, `{placeholders}`
/// and everything that isn't a letter kept.
pub fn pseudo(lang: &str, text: &str) -> String {
    let letter = |k: u32, n: u32| -> String {
        match lang {
            "x-cjk" => char::from_u32(0x4E00 + (k * 37 + n * 101) % 0x5000).map(String::from).unwrap_or_default(),
            "x-arab" => {
                // ا to ي, skipping the unassigned U+063B..U+0640
                const AR: &[u32] = &[
                    0x627, 0x628, 0x62A, 0x62B, 0x62C, 0x62D, 0x62E, 0x62F, 0x630, 0x631, 0x632, 0x633, 0x634, 0x635, 0x636, 0x637, 0x638, 0x639, 0x63A, 0x641,
                    0x642, 0x643, 0x644, 0x645, 0x646, 0x647, 0x648, 0x64A,
                ];
                char::from_u32(AR[((k + n) % AR.len() as u32) as usize]).map(String::from).unwrap_or_default()
            }
            "x-cyrl" => {
                const CY: &str = "абцдефгхијклмнопљрстуввхызабвгдеж";
                CY.chars().nth(((k + n) % 26) as usize).map(String::from).unwrap_or_default()
            }
            "x-deva" => {
                // a consonant, then a combining vowel sign or the virama
                const MARKS: &[u32] = &[0x941, 0x942, 0x947, 0x948, 0x94D, 0x902, 0x901];
                let base = char::from_u32(0x915 + (k + n) % 0x24).unwrap_or('क');
                let mark = char::from_u32(MARKS[((k * 3 + n) % MARKS.len() as u32) as usize]).unwrap_or('्');
                format!("{}{}", base, mark)
            }
            "x-avst" => char::from_u32(0x10B00 + (k + n) % 0x36).map(String::from).unwrap_or_default(),
            _ => String::new(),
        }
    };
    let mut out = String::new();
    let (mut run, mut n) = (0u32, 0u32);
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        // a placeholder, kept whole
        if c == '{' {
            if let Some(end) = rest.find('}') {
                out.push_str(&rest[..=end]);
                rest = &rest[end + 1..];
                continue;
            }
        }
        rest = &rest[c.len_utf8()..];
        if !c.is_ascii_alphabetic() {
            out.push(c);
            continue;
        }
        let k = c.to_ascii_lowercase() as u32 - 'a' as u32;
        out.push_str(&letter(k, n));
        n += 1;
        run += 1;
        if run == 5 {
            out.push_str(&letter(k, n + 7));
            out.push_str(&letter(k, n + 13));
            run = 0;
        }
    }
    out
}

/// `msgid` (in context `ctx`, "" for none) in the selected language.
pub fn lookup(ctx: &str, msgid: &str) -> String {
    {
        let c = current().read().unwrap_or_else(|e| e.into_inner());
        if PSEUDO.contains(&c.lang.as_str()) {
            return pseudo(&c.lang, msgid);
        }
        if let Some(cat) = &c.cat {
            return cat.get(&(ctx.to_string(), msgid.to_string())).cloned().unwrap_or_else(|| msgid.to_string());
        }
        if c.lang.is_empty() || c.lang == "en" {
            return msgid.to_string();
        }
    }
    let mut c = current().write().unwrap_or_else(|e| e.into_inner());
    if c.cat.is_none() {
        let src = LANGUAGES.iter().find(|l| l.0 == c.lang).map_or("", |l| l.2);
        c.cat = Some(parse_po(src));
    }
    c.cat.as_ref().and_then(|cat| cat.get(&(ctx.to_string(), msgid.to_string())).cloned()).unwrap_or_else(|| msgid.to_string())
}

/// `text` with each `{name}` replaced by its value (`{{` / `}}` for a
/// literal brace). A placeholder without a value stays as written, so a
/// translation's typo shows instead of vanishing.
pub fn fill(text: &str, args: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push(tail.as_bytes()[0] as char);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('{') {
            if let Some(end) = tail.find('}') {
                let name = &tail[1..end];
                if let Some((_, v)) = args.iter().find(|a| a.0 == name) {
                    out.push_str(v);
                    rest = &tail[end + 1..];
                    continue;
                }
            }
        }
        out.push(tail.as_bytes()[0] as char);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// An interface message in the player's language:
/// `t!("Saved to {file}", file = path.display())`. The first argument must
/// be a string literal (the template is read from the source).
#[macro_export]
macro_rules! t {
    ($msg:literal) => { $crate::i18n::lookup("", $msg) };
    ($msg:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::lookup("", $msg), &[$((stringify!($name), ($val).to_string())),+])
    };
}

/// Marks a literal as an interface message without translating it, for
/// tables that must stay constant (setting labels, help lines): the
/// template picks it up, and the code that shows it calls [`tr`].
#[macro_export]
macro_rules! n {
    ($msg:literal) => {
        $msg
    };
}

/// A message marked with [`n!`], in the selected language.
pub fn tr(msgid: &str) -> String {
    lookup("", msgid)
}

/// [`t!`] with a context, for an English text used in two senses:
/// `tc!("menu", "Open")`.
#[macro_export]
macro_rules! tc {
    ($ctx:literal, $msg:literal) => { $crate::i18n::lookup($ctx, $msg) };
    ($ctx:literal, $msg:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::i18n::fill(&$crate::i18n::lookup($ctx, $msg), &[$((stringify!($name), ($val).to_string())),+])
    };
}

/// How many terminal columns `s` takes: two for wide (CJK) characters,
/// none for combining marks, which share their base character's cell.
/// Control characters count as one (they are shown as spaces).
pub fn width(s: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    s.chars().map(|c| if c.is_control() { 1 } else { c.width().unwrap_or(0) }).sum()
}

/// `s` cut to at most `w` columns, and the columns it takes. A character
/// that doesn't fit is left out with the marks that follow it, so a
/// cluster (base + marks) is never split; controls become spaces.
pub fn cut(s: &str, w: usize) -> (String, usize) {
    use unicode_width::UnicodeWidthChar;
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars().map(|c| if c.is_control() { ' ' } else { c }) {
        let cw = c.width().unwrap_or(0);
        if used + cw > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    (out, used)
}

/// `s` cut or padded with spaces to exactly `w` columns.
pub fn fit(s: &str, w: usize) -> String {
    let (mut out, used) = cut(s, w);
    out.push_str(&" ".repeat(w - used));
    out
}

/// `s` padded with spaces to at least `w` columns (`format!("{:<w$}")`
/// counts characters, which misaligns wide ones); never cut.
pub fn pad(s: &str, w: usize) -> String {
    format!("{}{}", s, " ".repeat(w.saturating_sub(width(s))))
}

/// A `.po` file's translations. Entries with an empty `msgstr`, the header
/// (`msgid ""`) and fuzzy entries (not yet checked by a translator) are
/// left out, so those messages show in English.
pub fn parse_po(src: &str) -> Catalog {
    #[derive(Default)]
    struct Entry {
        ctx: String,
        id: String,
        tr: String,
        fuzzy: bool,
    }
    let mut cat = Catalog::new();
    let mut e = Entry::default();
    // which field a continuation line ("…" alone) adds to
    let mut field = 0u8;
    let mut seen_id = false;
    let finish = |e: &mut Entry, cat: &mut Catalog, seen: &mut bool| {
        if *seen && !e.id.is_empty() && !e.tr.is_empty() && !e.fuzzy {
            cat.insert((std::mem::take(&mut e.ctx), std::mem::take(&mut e.id)), std::mem::take(&mut e.tr));
        }
        *e = Entry::default();
        *seen = false;
    };
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() {
            finish(&mut e, &mut cat, &mut seen_id);
            field = 0;
            continue;
        }
        if let Some(flags) = line.strip_prefix("#,") {
            e.fuzzy |= flags.split(',').any(|f| f.trim() == "fuzzy");
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (key, rest) = match line.split_once(char::is_whitespace) {
            Some((k, r)) if !line.starts_with('"') => (k, r.trim()),
            _ => ("", line),
        };
        let Some(s) = unquote(rest) else { continue };
        match key {
            "msgctxt" => {
                // a new entry starts at its msgctxt
                if seen_id {
                    finish(&mut e, &mut cat, &mut seen_id);
                }
                e.ctx = s;
                field = 1;
            }
            "msgid" => {
                if seen_id {
                    let ctx = std::mem::take(&mut e.ctx);
                    finish(&mut e, &mut cat, &mut seen_id);
                    e.ctx = ctx;
                }
                e.id = s;
                seen_id = true;
                field = 2;
            }
            "msgstr" | "msgstr[0]" => {
                e.tr = s;
                field = 3;
            }
            "" => match field {
                1 => e.ctx.push_str(&s),
                2 => e.id.push_str(&s),
                3 => e.tr.push_str(&s),
                _ => {}
            },
            _ => field = 0,
        }
    }
    finish(&mut e, &mut cat, &mut seen_id);
    cat
}

/// The text of a `.po` string (`"…"` with C escapes).
fn unquote(s: &str) -> Option<String> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            c => {
                out.push('\\');
                out.push(c);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# a translator's comment
msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"

#: src/tty.rs
msgid "Settings"
msgstr "設定"

msgctxt "menu"
msgid "Open"
msgstr "開く"

msgid "Open"
msgstr "オープン"

msgid "Saved {file} in {dir}"
msgstr "{dir} に {file} を保存しました"

msgid ""
"A long message "
"on two lines\n"
msgstr ""
"二行に"
"わたる\"説明\"\n"

msgid "Untranslated"
msgstr ""

#, fuzzy
msgid "Unchecked"
msgstr "未確認"
"#;

    /// Each `.po` construct: header skipped, context, continuation lines,
    /// escapes; empty and fuzzy entries left out (English shows)
    #[test]
    fn po_parsing() {
        let c = parse_po(SAMPLE);
        let get = |ctx: &str, id: &str| c.get(&(ctx.to_string(), id.to_string())).cloned();
        assert_eq!(get("", "Settings").as_deref(), Some("設定"));
        assert_eq!(get("menu", "Open").as_deref(), Some("開く"));
        assert_eq!(get("", "Open").as_deref(), Some("オープン"));
        assert_eq!(get("", "A long message on two lines\n").as_deref(), Some("二行にわたる\"説明\"\n"));
        assert_eq!(get("", "Untranslated"), None);
        assert_eq!(get("", "Unchecked"), None);
        assert_eq!(get("", ""), None);
        assert_eq!(c.len(), 5);
    }

    /// Placeholders by name, in the translation's order; a missing value
    /// leaves the placeholder visible; doubled braces are literal
    #[test]
    fn placeholders() {
        let c = parse_po(SAMPLE);
        let tr = c.get(&(String::new(), "Saved {file} in {dir}".to_string())).unwrap();
        assert_eq!(fill(tr, &[("file", "a.sav".into()), ("dir", "sav".into())]), "sav に a.sav を保存しました");
        assert_eq!(fill("{a} and {b}", &[("a", "1".into())]), "1 and {b}");
        assert_eq!(fill("{{literal}} {x}", &[("x", "y".into())]), "{literal} y");
        assert_eq!(fill("no braces", &[]), "no braces");
        assert_eq!(fill("odd } and {", &[]), "odd } and {");
    }

    /// Each catalogue compiled in parses (the shipped ones may be empty)
    #[test]
    fn shipped_catalogues_parse() {
        for (code, _, src) in LANGUAGES {
            let _ = parse_po(src);
            assert!(*code == "en" || src.contains("Content-Type: text/plain; charset=UTF-8"), "{} has no header", code);
        }
    }

    /// The `t!` / `tc!` literals in `src/` (in order of first appearance)
    /// with the files they appear in.
    fn source_messages() -> Vec<(String, String, Vec<String>)> {
        let mut files = Vec::new();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    out.push(p);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        walk(&root.join("src"), &mut files);
        let mut msgs: Vec<(String, String, Vec<String>)> = Vec::new();
        for f in files {
            let rel = f.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            // (this file's messages are examples; comments hold examples too)
            if rel == "src/i18n.rs" {
                continue;
            }
            // (only the program's own code: test modules left out, each
            // from its `#[cfg(test)]` to the `}` in the first column)
            let all = std::fs::read_to_string(&f).unwrap();
            let mut src = String::new();
            let mut in_test = false;
            let lines: Vec<&str> = all.lines().collect();
            for (k, l) in lines.iter().enumerate() {
                if !in_test && l.trim() == "#[cfg(test)]" {
                    let next = lines[k + 1..].iter().map(|l| l.trim()).find(|l| !l.starts_with("#["));
                    in_test = next.is_some_and(|n| n.starts_with("mod ") || n.contains(" mod "));
                }
                if in_test {
                    in_test = *l != "}";
                    continue;
                }
                if !l.trim_start().starts_with("//") {
                    src.push_str(l);
                    src.push('\n');
                }
            }
            for (ctx, id) in macro_literals(&src) {
                match msgs.iter_mut().find(|m| m.0 == ctx && m.1 == id) {
                    Some(m) if !m.2.contains(&rel) => m.2.push(rel.clone()),
                    Some(_) => {}
                    None => msgs.push((ctx, id, vec![rel.clone()])),
                }
            }
        }
        msgs
    }

    /// The literal arguments of each `t!("…"` and `tc!("…", "…"` in `src`.
    fn macro_literals(src: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() {
            // (bytes: `i` may be inside a multi-byte character)
            let (ctx_macro, skip) = if b[i..].starts_with(b"tc!(") {
                (true, 4)
            } else if b[i..].starts_with(b"t!(") || b[i..].starts_with(b"n!(") {
                (false, 3)
            } else {
                i += 1;
                continue;
            };
            // not part of a longer name (`print!(`, `fmt!(`)
            if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
                i += 1;
                continue;
            }
            let mut j = i + skip;
            let mut lits = Vec::new();
            for _ in 0..if ctx_macro { 2 } else { 1 } {
                while j < b.len() && (b[j] as char).is_whitespace() {
                    j += 1;
                }
                let Some((s, end)) = rust_str(&src[j..]) else { break };
                lits.push(s);
                j += end;
                while j < b.len() && ((b[j] as char).is_whitespace() || b[j] == b',') {
                    j += 1;
                }
            }
            match (ctx_macro, lits.len()) {
                (true, 2) => out.push((lits[0].clone(), lits[1].clone())),
                (false, 1) => out.push((String::new(), lits[0].clone())),
                _ => {}
            }
            i = j.max(i + 1);
        }
        out
    }

    /// A Rust string literal at the start of `s`: its value and length.
    fn rust_str(s: &str) -> Option<(String, usize)> {
        let rest = s.strip_prefix('"')?;
        let mut out = String::new();
        let mut it = rest.char_indices();
        while let Some((k, c)) = it.next() {
            match c {
                '"' => return Some((out, k + 2)),
                '\\' => match it.next()?.1 {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    'x' => {
                        let hex: String = [it.next()?.1, it.next()?.1].iter().collect();
                        out.push(u8::from_str_radix(&hex, 16).ok()? as char);
                    }
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    '\\' => out.push('\\'),
                    'u' => {
                        let mut hex = String::new();
                        for (_, h) in it.by_ref() {
                            match h {
                                '{' => {}
                                '}' => break,
                                h => hex.push(h),
                            }
                        }
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    // a line continuation: the newline and the next line's indent go
                    '\n' => {
                        let peek = it.clone();
                        for (_, w) in peek {
                            if !w.is_whitespace() {
                                break;
                            }
                            it.next();
                        }
                    }
                    c => {
                        out.push('\\');
                        out.push(c);
                    }
                },
                c => out.push(c),
            }
        }
        None
    }

    /// A string as a `.po` string, split at newlines the way gettext does.
    fn po_quote(s: &str) -> String {
        let esc = |p: &str| p.replace('\\', "\\\\").replace('"', "\\\"").replace('\t', "\\t").replace('\n', "\\n");
        if !s.contains('\n') || s.trim_end_matches('\n').find('\n').is_none() && s.ends_with('\n') && s.len() < 70 {
            return format!("\"{}\"", esc(s));
        }
        let mut out = String::from("\"\"");
        for part in s.split_inclusive('\n') {
            out.push_str(&format!("\n\"{}\"", esc(part)));
        }
        out
    }

    /// The template for the messages in `src/`.
    fn template() -> String {
        let mut out = String::from(
            "# emuera-tty's interface messages: the template for translations.\n\
             # Made from the source by `UPDATE_POT=1 cargo test`; don't edit it by hand.\n\
             msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n",
        );
        for (ctx, id, files) in source_messages() {
            out.push('\n');
            out.push_str(&format!("#: {}\n", files.join(" ")));
            if id.contains('{') {
                out.push_str("#, no-c-format\n");
            }
            if !ctx.is_empty() {
                out.push_str(&format!("msgctxt {}\n", po_quote(&ctx)));
            }
            out.push_str(&format!("msgid {}\nmsgstr \"\"\n", po_quote(&id)));
        }
        out
    }

    /// `i18n/emuera-tty.pot` lists exactly the messages in the source;
    /// `UPDATE_POT=1 cargo test` rewrites it
    #[test]
    fn template_matches_source() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n/emuera-tty.pot");
        let want = template();
        if std::env::var_os("UPDATE_POT").is_some() {
            std::fs::write(&path, &want).unwrap();
            return;
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
        if have != want {
            let (h, w) = (parse_ids(&have), parse_ids(&want));
            let missing: Vec<_> = w.iter().filter(|m| !h.contains(m)).collect();
            let stale: Vec<_> = h.iter().filter(|m| !w.contains(m)).collect();
            panic!(
                "i18n/emuera-tty.pot is out of date (run `UPDATE_POT=1 cargo test`)\n  not in the template: {:?}\n  no longer in the source: {:?}",
                missing, stale
            );
        }
    }

    /// The (context, msgid) pairs of a template, translated or not.
    fn parse_ids(pot: &str) -> Vec<(String, String)> {
        // (give every entry a translation so the parser keeps it)
        let filled: String = pot.lines().map(|l| if l == "msgstr \"\"" { "msgstr \"x\"\n".to_string() } else { format!("{}\n", l) }).collect();
        let mut ids: Vec<_> = parse_po(&filled).into_keys().collect();
        ids.sort();
        ids
    }

    /// The template reader finds literals as written, contexts included,
    /// and not inside other macro names
    #[test]
    fn literal_extraction() {
        let src = "const L: &str = n!(\"Marked\");\nlet a = t!(\"Plain\");\nlet b = t!(\"With {n}\", n = 3);\nlet c = tc!(\"menu\", \"Open\");\nprint!(\"not this\");\nlet d = t!(\"Esc\\\"aped\\n\");\nlet e = t!(\"a\\x20b\\\n     c\");";
        let got = macro_literals(src);
        assert_eq!(
            got,
            vec![
                (String::new(), "Marked".to_string()),
                (String::new(), "Plain".to_string()),
                (String::new(), "With {n}".to_string()),
                ("menu".to_string(), "Open".to_string()),
                (String::new(), "Esc\"aped\n".to_string()),
                (String::new(), "a bc".to_string()),
            ]
        );
    }

    /// Widths and cuts in several scripts: combining marks take no column
    /// and stay with their base, CJK takes two, beyond-BMP letters one
    #[test]
    fn widths_and_cuts() {
        let hindi = "हिन्दी"; // ह ि न ् द ी: ि and ी are spacing, ् a mark
        assert_eq!(width(hindi), 5);
        let e = "cafe\u{301}";
        assert_eq!(width(e), 4);
        assert_eq!(cut(e, 4), (e.to_string(), 4));
        // the accent never ends up without its e
        assert_eq!(cut(e, 3), ("caf".to_string(), 3));
        let avst = "\u{10B00}\u{10B01}\u{10B02}";
        assert_eq!(width(avst), 3);
        assert_eq!(cut(avst, 2).0.chars().count(), 2);
        let mixed = "Save مرحبا 日本";
        assert_eq!(width(mixed), 5 + 5 + 1 + 4);
        assert_eq!(cut("日本語", 3), ("日".to_string(), 2));
        assert_eq!(fit("日本語", 5), "日本 ");
        assert_eq!(fit("na\u{301}ive", 3), "na\u{301}i");
        assert_eq!(pad("言語", 6), "言語  ");
        assert_eq!(pad("Language", 4), "Language");
        // the virama stays with न when cut right after it
        let (h, w) = cut(hindi, 3);
        assert!(h.ends_with('\u{94D}') && w == 3, "{:?}", h);
    }

    /// Locale names to catalogue codes, Chinese by script
    #[test]
    fn locale_mapping() {
        for (name, want) in [
            ("ja_JP.UTF-8", Some("ja")),
            ("ja", Some("ja")),
            ("zh_TW", Some("zh-Hant")),
            ("zh_HK.UTF-8", Some("zh-Hant")),
            ("zh-Hant", Some("zh-Hant")),
            ("zh-Hant-TW", Some("zh-Hant")),
            ("zh_CN.UTF-8", Some("zh-Hans")),
            ("zh-Hans-SG", Some("zh-Hans")),
            ("zh", Some("zh-Hans")),
            ("ko_KR.UTF-8", Some("ko")),
            ("en_US.UTF-8", Some("en")),
            ("de_DE.UTF-8@euro", None),
            ("C", None),
            ("POSIX", None),
            ("", None),
        ] {
            assert_eq!(from_locale(name), want, "{:?}", name);
        }
    }

    /// `EMUERA_LANG` beats the setting, which beats the locale; `auto`
    /// follows the locale; nothing known gives English
    #[test]
    fn language_precedence() {
        assert_eq!(choose("ja", "en", "ko_KR.UTF-8"), "ja");
        assert_eq!(choose("", "zh-Hant", "ja_JP.UTF-8"), "zh-Hant");
        assert_eq!(choose("", "auto", "ko_KR.UTF-8"), "ko");
        assert_eq!(choose("", "auto", "C"), "en");
        assert_eq!(choose("", "auto", "de_DE.UTF-8"), "en");
        assert_eq!(choose("fr", "auto", "zh_CN"), "zh-Hans");
        assert_eq!(choose("x-deva", "ja", "C"), "x-deva");
        // (a pseudo-language isn't a setting value)
        assert_eq!(choose("", "x-cjk", "ko"), "ko");
    }

    /// Each pseudo-language: its own script, placeholders and punctuation
    /// kept, about 40% longer
    #[test]
    fn pseudo_languages() {
        let src = "Save {slot} to the library? (y/n)";
        let letters = src.chars().filter(|c| c.is_ascii_alphabetic()).count() - "slot".len();
        for (lang, lo, hi) in
            [("x-cjk", 0x4E00, 0x9FFF), ("x-arab", 0x600, 0x6FF), ("x-cyrl", 0x400, 0x4FF), ("x-deva", 0x900, 0x97F), ("x-avst", 0x10B00, 0x10B3F)]
        {
            let p = pseudo(lang, src);
            assert!(p.contains("{slot}") && p.contains("? (") && p.contains('/') && p.ends_with(')'), "{}: {}", lang, p);
            let body = p.replace("{slot}", "");
            let script: Vec<char> = body.chars().filter(|c| (lo..=hi).contains(&(*c as u32))).collect();
            assert!(!script.is_empty() && !body.chars().any(|c| c.is_ascii_alphabetic()), "{}: {}", lang, p);
            // letters (for Devanagari: syllables) ~1.4 times as many
            let n = if lang == "x-deva" { script.len() / 2 } else { script.len() };
            assert!(n * 10 >= letters * 13 && n * 10 <= letters * 15, "{}: {} letters for {}", lang, n, letters);
        }
        // Devanagari brings combining marks; Old Avestan is beyond the BMP
        assert!(pseudo("x-deva", "ab").chars().any(|c| matches!(c as u32, 0x901 | 0x902 | 0x941 | 0x942 | 0x947 | 0x948 | 0x94D)));
        assert!(pseudo("x-avst", "ab").chars().all(|c| c as u32 > 0xFFFF));
    }

    /// The lookup follows the selected language and falls back to English;
    /// switching languages drops the old catalogue
    #[test]
    fn language_switch() {
        install("ja", SAMPLE);
        assert_eq!(t!("Settings"), "設定");
        assert_eq!(tc!("menu", "Open"), "開く");
        assert_eq!(t!("Saved {file} in {dir}", file = "a.sav", dir = "sav"), "sav に a.sav を保存しました");
        assert_eq!(t!("Untranslated"), "Untranslated");
        install("zh-Hans", "msgid \"Settings\"\nmsgstr \"设置\"\n");
        assert_eq!(t!("Settings"), "设置");
        assert_eq!(tc!("menu", "Open"), "Open");
        // an empty catalogue (the shipped ones, for now) and English: English
        set_language("ko");
        assert_eq!(t!("Settings"), "Settings");
        set_language("en");
        assert_eq!(language(), "en");
        assert_eq!(t!("{n} saves", n = 3), "3 saves");
    }
}

/// A terminal screen for tests: what emuera-tty writes, laid out the way a
/// terminal would (cursor moves, text by display width, combining marks in
/// the cell of the character before them); other escape sequences ignored.
#[cfg(test)]
pub mod screen {
    use unicode_width::UnicodeWidthChar;

    pub struct Screen {
        pub cols: usize,
        pub rows: usize,
        /// each cell's text: a character and its marks; "" after a wide one
        pub cells: Vec<Vec<String>>,
        /// something was written past the right edge or the last row
        pub overflow: bool,
        x: usize,
        y: usize,
    }

    impl Screen {
        pub fn new(cols: usize, rows: usize) -> Screen {
            Screen { cols, rows, cells: vec![vec![" ".to_string(); cols]; rows], overflow: false, x: 0, y: 0 }
        }

        pub fn feed(&mut self, bytes: &[u8]) {
            let text = String::from_utf8_lossy(bytes);
            let mut it = text.chars().peekable();
            while let Some(c) = it.next() {
                match c {
                    '\x1b' => match it.next() {
                        Some('[') => {
                            let mut params = String::new();
                            for p in it.by_ref() {
                                if ('@'..='~').contains(&p) {
                                    self.csi(&params, p);
                                    break;
                                }
                                params.push(p);
                            }
                        }
                        // OSC, DCS, APC: to the string terminator (or BEL)
                        Some(']' | 'P' | '_') => {
                            while let Some(p) = it.next() {
                                if p == '\x07' || (p == '\x1b' && it.peek() == Some(&'\\')) {
                                    if p == '\x1b' {
                                        it.next();
                                    }
                                    break;
                                }
                            }
                        }
                        _ => {}
                    },
                    '\r' => self.x = 0,
                    '\n' => self.y += 1,
                    c if c.is_control() => {}
                    c => self.put(c),
                }
            }
        }

        fn csi(&mut self, params: &str, f: char) {
            let n: Vec<usize> = params.split(';').map(|p| p.trim_start_matches('?').parse().unwrap_or(0)).collect();
            match f {
                'H' => {
                    self.y = n.first().copied().unwrap_or(1).max(1) - 1;
                    self.x = n.get(1).copied().unwrap_or(1).max(1) - 1;
                }
                'K' if self.y < self.rows => {
                    for x in self.x..self.cols {
                        self.cells[self.y][x] = " ".to_string();
                    }
                }
                _ => {}
            }
        }

        fn put(&mut self, c: char) {
            let w = c.width().unwrap_or(0);
            if w == 0 {
                // a mark: with the character before it
                if self.y < self.rows && self.x > 0 {
                    let mut k = self.x - 1;
                    while k > 0 && self.cells[self.y][k].is_empty() {
                        k -= 1;
                    }
                    self.cells[self.y][k].push(c);
                }
                return;
            }
            if self.y >= self.rows || self.x + w > self.cols {
                self.overflow = true;
                self.x += w;
                return;
            }
            self.cells[self.y][self.x] = c.to_string();
            if w == 2 {
                self.cells[self.y][self.x + 1] = String::new();
            }
            self.x += w;
        }

        /// Row `y`'s text.
        pub fn row(&self, y: usize) -> String {
            self.cells[y].concat()
        }

        /// Everything on the screen.
        pub fn text(&self) -> String {
            (0..self.rows).map(|y| self.row(y) + "\n").collect()
        }

        /// The box drawn with `┌┐└┘│` whose top-left corner is the first on
        /// the screen: Err with what is wrong if a row's side is out of line.
        pub fn check_box(&self) -> Result<(usize, usize), String> {
            let find = |c: &str| (0..self.rows).find_map(|y| self.cells[y].iter().position(|t| t == c).map(|x| (x, y)));
            let (left, top) = find("┌").ok_or("no top-left corner")?;
            let right = self.cells[top].iter().position(|t| t == "┐").ok_or("no top-right corner")?;
            let bottom = (top + 1..self.rows).find(|&y| self.cells[y][left] == "└").ok_or("no bottom-left corner")?;
            if self.cells[bottom][right] != "┘" {
                return Err(format!("bottom-right corner not at column {}: {:?}", right, self.row(bottom)));
            }
            for y in top + 1..bottom {
                // (a divider row: ├──┤)
                if !matches!(self.cells[y][left].as_str(), "│" | "├") || !matches!(self.cells[y][right].as_str(), "│" | "┤") {
                    return Err(format!("row {} out of line: {:?}", y, self.row(y)));
                }
            }
            Ok((left, right))
        }

        /// A Devanagari consonant without the vowel sign or virama the
        /// pseudo-language puts after every one: a dropped mark.
        pub fn dropped_mark(&self) -> Option<String> {
            for y in 0..self.rows {
                for t in &self.cells[y] {
                    let mut ch = t.chars();
                    if let Some(c) = ch.next() {
                        if ('\u{915}'..='\u{939}').contains(&c) && ch.next().is_none() {
                            return Some(self.row(y));
                        }
                    }
                }
            }
            None
        }
    }
}
