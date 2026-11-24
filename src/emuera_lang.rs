//! Emuera's own messages (load warnings, script errors, the debug
//! console's refusals) in the language Emuera would show them in.
//!
//! Emuera-EM/EE keeps each message in Japanese under an ID (`Lang.cs`:
//! `Error.Warning1` = `警告Lv{0}:{1}:{2}行目:{3}`) and reads translations
//! keyed by those IDs from `lang/emuera.*.xml` beside its exe
//! (`<lang><name>English</name><tr id="Error.Warning1">…</tr>`). Its
//! setting `Emueraの表示言語` names the language; empty, the system's
//! language picks one (Lang.SetLanguage).
//!
//! Here a message is written as [`em!`] with Emuera's ID, its Japanese
//! text and English written for emuera-tty, then its arguments. The
//! language is chosen once at start ([`init`]):
//!
//! 1. the game's `Emueraの表示言語`: `日本語` is Japanese; a name a game's
//!    language file has is that file;
//! 2. otherwise emuera-tty's own interface language: Japanese, or a
//!    matching language file of the game's, or emuera-tty's English.
//!
//! The text of a game's file is used as it is; nothing of Emuera's own
//! translations is included here. Until [`init`] runs (unit tests), every
//! message is Japanese, as Emuera's default.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Emuera's name for its default language (Lang.DefaultLanguage).
const JAPANESE_NAME: &str = "日本語";
/// A language file bigger than this isn't read (Emuera's are ~150 KB).
const MAX_LANG_FILE_BYTES: u64 = 4 << 20;

/// The language messages are shown in.
#[derive(Clone, Debug, Default, PartialEq)]
enum Active {
    /// Emuera's own Japanese text (the default)
    #[default]
    Japanese,
    /// a game's language file's text by ID, else emuera-tty's English
    Other { table: HashMap<String, String> },
}

#[cfg(not(test))]
static ACTIVE: RwLock<Active> = RwLock::new(Active::Japanese);

// (tests run in parallel threads: each has its own, as in i18n.rs)
#[cfg(test)]
thread_local! {
    static ACTIVE_T: &'static RwLock<Active> = Box::leak(Box::new(RwLock::new(Active::Japanese)));
}

/// The language in force (per thread in tests).
fn active() -> &'static RwLock<Active> {
    #[cfg(not(test))]
    return &ACTIVE;
    #[cfg(test)]
    return ACTIVE_T.with(|a| *a);
}

/// One `lang/emuera.*.xml`: its language's name, culture and texts.
#[derive(Debug, Default)]
struct LangFile {
    name: String,
    culture: String,
    /// from the file name: `emuera.eng.xml` → `eng`
    suffix: String,
    table: HashMap<String, String>,
}

/// Choose the messages' language for the game in `game_dir` with the
/// settings `cfg` (see the module's notes). Call after the interface
/// language is chosen (`i18n::select`) and before the scripts load.
pub fn init(game_dir: &Path, cfg: &crate::emuconfig::Values) {
    let files = lang_files(game_dir);
    let chosen = choose(cfg.value("Emueraの表示言語").trim(), &crate::i18n::language(), files);
    *active().write().unwrap_or_else(|e| e.into_inner()) = chosen;
}

/// The language from `setting` (Emuera's), else `ui_lang` (emuera-tty's
/// interface language code), and the game's language files.
fn choose(setting: &str, ui_lang: &str, files: Vec<LangFile>) -> Active {
    if setting == JAPANESE_NAME {
        return Active::Japanese;
    }
    let pick = |f: LangFile| Active::Other { table: f.table };
    let mut files = files;
    if !setting.is_empty() {
        if let Some(i) = files.iter().position(|f| f.name == setting) {
            return pick(files.swap_remove(i));
        }
        if setting.eq_ignore_ascii_case("English") {
            return Active::Other { table: HashMap::new() };
        }
        // a language the game has no file for: as when it is unset
    }
    if ui_lang == "ja" {
        return Active::Japanese;
    }
    match files.iter().position(|f| file_matches(f, ui_lang)) {
        Some(i) => pick(files.swap_remove(i)),
        None => Active::Other { table: HashMap::new() },
    }
}

/// Is `f` in the interface language `code` (`en`, `zh-Hans`…)? By its
/// `<culture>`, its `<name>` (as emuera-tty names the language), or the
/// usual file names (`emuera.eng.xml`).
fn file_matches(f: &LangFile, code: &str) -> bool {
    let code_lower = code.to_ascii_lowercase();
    let culture = f.culture.to_ascii_lowercase();
    let by_culture = !culture.is_empty() && (culture == code_lower || culture.starts_with(&format!("{}-", code_lower)) || code_lower.starts_with(&culture));
    let by_name = crate::i18n::LANGUAGES.iter().any(|(c, name, _)| *c == code && *name == f.name);
    let by_suffix = matches!(
        (code, f.suffix.as_str()),
        ("en", "eng" | "en") | ("zh-Hans", "zhs" | "chs") | ("zh-Hant", "zht" | "cht") | ("ko", "kor" | "ko") | ("es", "spa" | "es")
    );
    by_culture || by_name || by_suffix
}

/// The game's language files: `lang/emuera.*.xml` beside the game, or in
/// the folder above (an exe kept there, as eraMegatenP's `Data/`).
fn lang_files(game_dir: &Path) -> Vec<LangFile> {
    let dirs: Vec<PathBuf> = std::iter::once(game_dir).chain(game_dir.parent()).map(|d| d.join("lang")).filter(|d| d.is_dir()).collect();
    let Some(dir) = dirs.first() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().starts_with("emuera.") && n.to_ascii_lowercase().ends_with(".xml"))
        })
        .collect();
    paths.sort();
    paths.iter().filter_map(|p| read_lang_file(p)).collect()
}

/// One language file, or None when it can't be read or names no language
/// (as Emuera skips such a file).
fn read_lang_file(path: &Path) -> Option<LangFile> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > MAX_LANG_FILE_BYTES {
        eprintln!("emuera-tty: {}", t!("{file}: too big for a language file, not read", file = path.display()));
        return None;
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("emuera-tty: {}: {}", path.display(), e);
            return None;
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut f = parse_lang(&text);
    if f.name.is_empty() {
        return None;
    }
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    f.suffix = file_name.trim_start_matches("emuera.").trim_end_matches(".xml").to_string();
    Some(f)
}

/// The parts of a language file used here: `<name>`, `<culture>` and each
/// `<tr id="…">text</tr>` (CDATA or plain text with the five XML entities).
fn parse_lang(text: &str) -> LangFile {
    let mut f =
        LangFile { name: element_text(text, "name").unwrap_or_default(), culture: element_text(text, "culture").unwrap_or_default(), ..LangFile::default() };
    let mut rest = text;
    while let Some(start) = rest.find("<tr ") {
        rest = &rest[start..];
        let Some(open_end) = rest.find('>') else { break };
        let id = attribute(&rest[..open_end], "id");
        let body_start = open_end + 1;
        let Some(close) = rest[body_start..].find("</tr>") else { break };
        let body = &rest[body_start..body_start + close];
        if let Some(id) = id {
            f.table.insert(id, inner_text(body));
        }
        rest = &rest[body_start + close + "</tr>".len()..];
    }
    f
}

/// The trimmed text of the first `<tag>…</tag>`.
fn element_text(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    Some(inner_text(&text[start..end]).trim().to_string())
}

/// A `name="value"` (or `'value'`) attribute in an opening tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{}=", name))? + name.len() + 1;
    let quote = tag[at..].chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value_start = at + 1;
    let value_end = tag[value_start..].find(quote)? + value_start;
    Some(decode_entities(&tag[value_start..value_end]))
}

/// An element's text: a CDATA section as it is, else the text decoded.
fn inner_text(body: &str) -> String {
    let trimmed = body.trim();
    match trimmed.strip_prefix("<![CDATA[").and_then(|s| s.strip_suffix("]]>")) {
        Some(cdata) => cdata.to_string(),
        None => decode_entities(body),
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The template for message `id`: `japanese` (Emuera's text) when
/// messages are Japanese, else the game's translation, else `english`.
pub fn template(id: &str, japanese: &'static str, english: &'static str) -> String {
    match &*active().read().unwrap_or_else(|e| e.into_inner()) {
        Active::Japanese => japanese.to_string(),
        Active::Other { table } => table.get(id).cloned().unwrap_or_else(|| english.to_string()),
    }
}

/// .NET's `string.Format` as Emuera's messages use it: `{N}` is the N-th
/// argument; `{{` / `}}` are braces; a number past the arguments (a
/// game's translation may have one) stays as written.
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        let placeholder = tail.strip_prefix('{').and_then(|t| t.find('}').map(|end| &t[..end]));
        match placeholder.and_then(|p| p.trim().parse::<usize>().ok().map(|n| (n, p.len()))) {
            Some((n, len)) if n < args.len() => {
                out.push_str(&args[n]);
                rest = &tail[len + 2..];
            }
            _ => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}


/// Set the language directly (tests).
#[cfg(test)]
fn set_active(chosen: Active) {
    *active().write().unwrap_or_else(|e| e.into_inner()) = chosen;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, culture: &str, suffix: &str, pairs: &[(&str, &str)]) -> LangFile {
        LangFile {
            name: name.into(),
            culture: culture.into(),
            suffix: suffix.into(),
            table: pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    #[test]
    fn fill_follows_net_string_format() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(fill("警告Lv{0}:{1}:{2}行目:{3}", &a(&["1", "A.ERB", "6", "x"])), "警告Lv1:A.ERB:6行目:x");
        assert_eq!(fill("{1} before {0}", &a(&["a", "b"])), "b before a");
        assert_eq!(fill("{{0}} {0}", &a(&["x"])), "{0} x");
        // a number past the arguments, or not a number: left as it is
        assert_eq!(fill("{5} {x} {", &a(&["x"])), "{5} {x} {");
    }

    #[test]
    fn parse_lang_reads_name_culture_and_texts() {
        let xml = "\u{feff}<?xml version='1.0'?>\n<lang>\n <name>English</name>\n <culture>en</culture>\n \
                   <tr id=\"Error.A\"><![CDATA[Line {0} & <b>]]></tr>\n <tr id='Error.B'>x &amp; y</tr>\n</lang>";
        let f = parse_lang(xml);
        assert_eq!((f.name.as_str(), f.culture.as_str()), ("English", "en"));
        assert_eq!(f.table.get("Error.A").map(String::as_str), Some("Line {0} & <b>"));
        assert_eq!(f.table.get("Error.B").map(String::as_str), Some("x & y"));
    }

    #[test]
    fn the_language_follows_emueras_setting_then_emuera_ttys() {
        let eng = || vec![file("English", "", "eng", &[("Error.X", "game's English {0}")])];
        // Emuera's setting first
        assert_eq!(choose("日本語", "en", eng()), Active::Japanese);
        assert!(matches!(choose("English", "ja", eng()), Active::Other { table } if table.contains_key("Error.X")));
        assert!(matches!(choose("English", "ja", vec![]), Active::Other { table } if table.is_empty()));
        // unset: emuera-tty's language, with the game's file for it
        assert_eq!(choose("", "ja", eng()), Active::Japanese);
        assert!(matches!(choose("", "en", eng()), Active::Other { table } if table.contains_key("Error.X")));
        // no file for the language: emuera-tty's English
        assert!(matches!(choose("", "ko", eng()), Active::Other { table } if table.is_empty()));
        assert!(matches!(choose("", "zh-Hans", vec![file("简体中文", "", "zhs", &[("Error.X", "中")])]), Active::Other { table } if table.len() == 1));
    }

    #[test]
    fn em_uses_the_active_language() {
        set_active(Active::Japanese);
        assert_eq!(em!("Error.T", "行{0}", "line {0}", 3), "行3");
        set_active(Active::Other { table: HashMap::new() });
        assert_eq!(em!("Error.T", "行{0}", "line {0}", 3), "line 3");
        set_active(Active::Other { table: [("Error.T".to_string(), "row {0}!".to_string())].into() });
        assert_eq!(em!("Error.T", "行{0}", "line {0}", 3), "row 3!");
        set_active(Active::Japanese);
    }
}
