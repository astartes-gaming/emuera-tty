//! Live translation of character dialog. By default with a translator
//! running on this machine (nothing is sent anywhere else): Ollama,
//! LibreTranslate, translateLocally (Bergamot), argos-translate, or any
//! command given in `EMUERA_TRANSLATE_CMD` (text on its stdin, translation
//! on its stdout). On request, online services through their command-line
//! tools: translate-shell (`trans`) and DeepL (`deepl`).
//!
//! Lines are translated one at a time on a thread of their own, so the
//! game never waits: a line shows as printed until its translation is in.
//! Results are kept, so a line seen again is translated at once.

use crate::fxhash::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// A local translator.
#[derive(Clone, Debug)]
pub enum Engine {
    /// an Ollama model at localhost:11434
    Ollama { model: String },
    /// a LibreTranslate server (`url` up to the port)
    Libre { url: String },
    /// `translateLocally -m model`
    Bergamot { model: String },
    /// Argos Translate: one Python process kept running (see [`Argos`])
    Argos,
    /// a shell command
    Command(String),
    /// translate-shell (online: Google by default, `EMUERA_TRANS_ENGINE`)
    Trans,
    /// DeepL's own `deepl` tool (online; key in DEEPL_AUTH_KEY)
    Deepl,
}

impl Engine {
    /// The translator chosen in the settings (`local`, `translate-shell`,
    /// `deepl`), or why it can't be used.
    pub fn choose(which_one: &str) -> Result<Engine, String> {
        match which_one {
            "translate-shell" if which("trans") => Ok(Engine::Trans),
            "translate-shell" => Err("translate-shell (trans) isn't installed: get it from your package manager or translate-shell's site".into()),
            "deepl" if !which("deepl") => Err("DeepL's tool isn't installed: pip install deepl".into()),
            "deepl" if std::env::var_os("DEEPL_AUTH_KEY").is_none() => Err("DeepL needs your API key in DEEPL_AUTH_KEY (a free key works)".into()),
            "deepl" => Ok(Engine::Deepl),
            "argos" if which("argos-translate") || Argos::python().is_some() => Ok(Engine::Argos),
            "argos" => Err("Argos Translate isn't installed: pipx install argostranslate, then argospm install translate-ja_en".into()),
            _ => Engine::find().ok_or_else(|| {
                "no translator on this computer: install Ollama, LibreTranslate, translateLocally or argos-translate (or set EMUERA_TRANSLATE_CMD), or choose translate-shell / DeepL".into()
            }),
        }
    }

    /// The local translator to use: `EMUERA_TRANSLATE_CMD` if set, else the
    /// first of Ollama, LibreTranslate, translateLocally and argos-translate.
    pub fn find() -> Option<Engine> {
        if let Ok(c) = std::env::var("EMUERA_TRANSLATE_CMD") {
            if !c.trim().is_empty() {
                return Some(Engine::Command(c));
            }
        }
        let model = std::env::var("EMUERA_TRANSLATE_MODEL").ok().filter(|m| !m.is_empty());
        if let Some(tags) = http("127.0.0.1:11434", "GET", "/api/tags", "", Duration::from_millis(500)) {
            let name = model.clone().or_else(|| json_field(&tags, "name"));
            if let Some(model) = name {
                return Some(Engine::Ollama { model });
            }
        }
        let libre = std::env::var("EMUERA_LIBRETRANSLATE").unwrap_or_else(|_| "127.0.0.1:5000".into());
        let libre = libre.trim_start_matches("http://").trim_end_matches('/').to_string();
        if http(&libre, "GET", "/languages", "", Duration::from_millis(500)).is_some_and(|b| b.contains("\"ja\"")) {
            return Some(Engine::Libre { url: libre });
        }
        if which("translateLocally") {
            return Some(Engine::Bergamot { model: model.unwrap_or_default() });
        }
        if which("argos-translate") || Argos::python().is_some() {
            return Some(Engine::Argos);
        }
        None
    }

    pub fn describe(&self) -> String {
        match self {
            Engine::Ollama { model } => format!("Ollama ({})", model),
            Engine::Libre { url } => format!("LibreTranslate ({})", url),
            Engine::Bergamot { model } if model.is_empty() => "translateLocally".into(),
            Engine::Bergamot { model } => format!("translateLocally ({})", model),
            Engine::Argos => "argos-translate".into(),
            Engine::Command(c) => format!("`{}`", c),
            Engine::Trans => "translate-shell (online)".into(),
            Engine::Deepl => "DeepL (online)".into(),
        }
    }

    /// Translate one line from `from` (ja, en) into `to` (`argos`: the
    /// worker's Argos process, started on first use).
    fn translate(&self, text: &str, from: &str, to: &str, argos: &mut Option<Argos>) -> Result<String, String> {
        let out = match self {
            Engine::Ollama { model } => {
                let prompt = format!(
                    "Translate this line of {} text from a game into natural {}. Keep names, numbers and symbols as they are. Reply with the translation only, no notes.\n\n{}",
                    language_name(from),
                    language_name(to),
                    text
                );
                let body = format!("{{\"model\":{},\"prompt\":{},\"stream\":false,\"options\":{{\"temperature\":0.2}}}}", json_str(model), json_str(&prompt));
                let r = http("127.0.0.1:11434", "POST", "/api/generate", &body, Duration::from_secs(120)).ok_or("Ollama didn't answer")?;
                json_field(&r, "response").ok_or("Ollama gave no translation")?
            }
            Engine::Libre { url } => {
                let body = format!("{{\"q\":{},\"source\":{},\"target\":{},\"format\":\"text\"}}", json_str(text), json_str(from), json_str(to));
                let r = http(url, "POST", "/translate", &body, Duration::from_secs(60)).ok_or("LibreTranslate didn't answer")?;
                json_field(&r, "translatedText").ok_or("LibreTranslate gave no translation")?
            }
            Engine::Bergamot { model } => {
                // (no model given: Bergamot's names, `ja-en-tiny`)
                let model = if model.is_empty() { format!("{}-{}-tiny", from, to) } else { model.clone() };
                run(Command::new("translateLocally").args(["-m", &model]), text)?
            }
            Engine::Argos => {
                if argos.is_none() {
                    *argos = Argos::start(to);
                }
                match argos.as_mut().map(|a| a.translate(text, from)) {
                    Some(Ok(t)) => t,
                    // no module to import: the command, a process a line
                    _ => {
                        *argos = None;
                        run(Command::new("argos-translate").args(["--from-lang", from, "--to-lang", to]).arg(text), "")?
                    }
                }
            }
            Engine::Command(c) => {
                let mut cmd = if cfg!(windows) { Command::new("cmd") } else { Command::new("sh") };
                cmd.arg(if cfg!(windows) { "/C" } else { "-c" }).arg(c).env("EMUERA_TRANSLATE_TO", to).env("EMUERA_TRANSLATE_FROM", from);
                run(&mut cmd, text)?
            }
            // (the text on stdin: a line starting with - isn't an option)
            Engine::Trans => {
                let mut cmd = Command::new("trans");
                cmd.args(["-b", "-no-ansi", "-no-autocorrect", "-s", from, "-t", to]);
                if let Ok(e) = std::env::var("EMUERA_TRANS_ENGINE") {
                    cmd.args(["-e", &e]);
                }
                run(&mut cmd, text)?
            }
            Engine::Deepl => run(Command::new("deepl").args(["text", "--from", &from.to_uppercase(), "--to", &deepl_lang(to), text]), "")?,
        };
        // one line in, one line out
        let out = out.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        if out.is_empty() {
            Err("empty translation".into())
        } else {
            Ok(out)
        }
    }
}

/// DeepL's name for a target language (it wants the variant of some).
fn deepl_lang(code: &str) -> String {
    match code {
        "en" => "EN-US".into(),
        "pt" => "PT-BR".into(),
        c => c.to_uppercase(),
    }
}

fn language_name(code: &str) -> &str {
    match code {
        "en" => "English",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "fr" => "French",
        "de" => "German",
        "ru" => "Russian",
        "zh" => "Chinese",
        "ko" => "Korean",
        "ja" => "Japanese",
        "it" => "Italian",
        c => c,
    }
}

fn which(name: &str) -> bool {
    let exe = if cfg!(windows) { format!("{}.exe", name) } else { name.to_string() };
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(&exe).is_file()))
}

/// Run `cmd` with `input` on stdin; its stdout.
fn run(cmd: &mut Command, input: &str) -> Result<String, String> {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(input.as_bytes());
    }
    let o = child.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("translator failed ({})", o.status));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

/// A plain HTTP/1.0 request to a server on this machine; the body.
fn http(host: &str, method: &str, path: &str, body: &str, timeout: Duration) -> Option<String> {
    use std::net::ToSocketAddrs;
    let addr = host.to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(timeout)).ok()?;
    let req = format!(
        "{} {} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        method,
        path,
        host,
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).ok()?;
    let resp = String::from_utf8_lossy(&resp);
    let (head, body) = resp.split_once("\r\n\r\n")?;
    head.split_whitespace().nth(1).filter(|c| c.starts_with('2'))?;
    Some(body.to_string())
}

/// `s` as a JSON string literal.
fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            '\n' => o += "\\n",
            '\r' => o += "\\r",
            '\t' => o += "\\t",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The first string value of `"key"` in a JSON text.
fn json_field(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\"", key);
    let at = json.find(&pat)? + pat.len();
    let rest = json[at..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut it = rest.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' | 'f' => {}
                'u' => {
                    let hex: String = it.by_ref().take(4).collect();
                    let mut cp = u32::from_str_radix(&hex, 16).ok()?;
                    // a surrogate pair
                    if (0xD800..0xDC00).contains(&cp) {
                        let low: String = it.by_ref().skip(2).take(4).collect();
                        let lo = u32::from_str_radix(&low, 16).ok()?;
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.saturating_sub(0xDC00));
                    }
                    out.push(char::from_u32(cp).unwrap_or('?'));
                }
                c => out.push(c),
            },
            c => out.push(c),
        }
    }
    None
}

/// Argos Translate kept running: loading Python and the model takes
/// seconds, so one process translates every line (a line in, a line out).
pub struct Argos {
    child: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
}

const ARGOS_SCRIPT: &str = r#"
import sys
from argostranslate import translate
dst = sys.argv[1]
for line in sys.stdin:
    src, _, text = line.rstrip("\n").partition("\t")
    try:
        out = translate.translate(text, src, dst)
    except Exception as e:
        out = ""
    sys.stdout.write(" ".join(out.split("\n")) + "\n")
    sys.stdout.flush()
"#;

impl Argos {
    /// The Python that has argostranslate: the one `argos-translate` was
    /// installed with (pipx and virtual environments have their own),
    /// else python3 if it can import the module.
    fn python() -> Option<String> {
        let exe = if cfg!(windows) { "argos-translate.exe" } else { "argos-translate" };
        let cli = std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(exe)).find(|p| p.is_file()));
        if let Some(cli) = cli {
            if let Ok(text) = std::fs::read(&cli) {
                let first = String::from_utf8_lossy(&text[..text.len().min(300)]).lines().next().unwrap_or("").to_string();
                if let Some(line) = first.strip_prefix("#!") {
                    // `#!/usr/bin/python3 -I`, `#!/usr/bin/env -S python3`
                    let mut words = line.split_whitespace();
                    let py = match words.next() {
                        Some(e) if e.ends_with("/env") => words.find(|w| !w.starts_with('-')),
                        w => w,
                    };
                    if let Some(py) = py {
                        return Some(py.to_string());
                    }
                }
            }
        }
        let ok = Command::new("python3").args(["-c", "import argostranslate"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        ok.is_ok_and(|s| s.success()).then(|| "python3".to_string())
    }

    fn start(to: &str) -> Option<Argos> {
        let py = Argos::python()?;
        let mut child = Command::new(py).args(["-c", ARGOS_SCRIPT, to]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let input = child.stdin.take()?;
        let output = std::io::BufReader::new(child.stdout.take()?);
        Some(Argos { child, input, output })
    }

    fn translate(&mut self, text: &str, from: &str) -> Result<String, String> {
        use std::io::BufRead;
        let line = text.replace(['\r', '\n', '\t'], " ");
        writeln!(self.input, "{}\t{}", from, line).map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        let mut out = String::new();
        if self.output.read_line(&mut out).map_err(|e| e.to_string())? == 0 {
            return Err("Argos stopped (is argostranslate installed for that Python?)".into());
        }
        Ok(out.trim_end().to_string())
    }
}

impl Drop for Argos {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What became of a line.
enum State {
    Waiting,
    Done(String),
    Failed,
}

/// The translator thread and what it has translated.
pub struct Mt {
    to: Sender<(String, &'static str)>,
    from: Receiver<(String, Result<String, String>)>,
    seen: HashMap<String, State>,
    /// the last failure, for the settings page
    pub last_error: Option<String>,
}

impl Mt {
    pub fn start(engine: Engine, lang: &str) -> Mt {
        let (to, rx) = channel::<(String, &'static str)>();
        let (tx, from) = channel();
        let (e, lang) = (engine, lang.to_string());
        let _ = std::thread::Builder::new().name("translate".into()).spawn(move || {
            let mut argos = None;
            for (text, from) in rx {
                let r = e.translate(&text, from, &lang, &mut argos);
                if tx.send((text, r)).is_err() {
                    break;
                }
            }
        });
        Mt { to, from, seen: HashMap::default(), last_error: None }
    }

    /// The translation of `text` (in language `from`) if it is in; asks
    /// for it if never asked.
    pub fn get(&mut self, text: &str, from: &'static str) -> Option<&str> {
        if !self.seen.contains_key(text) {
            // (bounded: a long session forgets the oldest… all of them)
            if self.seen.len() > 20_000 {
                self.seen.clear();
            }
            self.seen.insert(text.to_string(), State::Waiting);
            let _ = self.to.send((text.to_string(), from));
            return None;
        }
        match self.seen.get(text) {
            Some(State::Done(t)) => Some(t),
            _ => None,
        }
    }

    /// Take the translations that came in; true if any did.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((text, r)) = self.from.try_recv() {
            any = true;
            match r {
                Ok(t) => {
                    self.seen.insert(text, State::Done(t));
                }
                Err(e) => {
                    self.last_error = Some(e);
                    self.seen.insert(text, State::Failed);
                }
            }
        }
        any
    }

    /// Are lines still being translated?
    pub fn busy(&self) -> bool {
        self.seen.values().any(|s| matches!(s, State::Waiting))
    }
}

/// A map or a box drawing (lines, blocks, Braille dots): never translated.
pub fn is_drawing(text: &str) -> bool {
    let drawing = text.chars().filter(|&c| matches!(c as u32, 0x2500..=0x25FF | 0x2800..=0x28FF)).count();
    drawing * 10 > text.chars().count()
}

fn is_japanese(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// The language of `text` to translate from into `to` — Japanese or
/// English, as `from` (the Translate from setting: auto, ja, en) allows —
/// or None when there is nothing to translate: no words, a map or box
/// drawing, or already the target language.
pub fn source_of(text: &str, from: &str, to: &str) -> Option<&'static str> {
    if is_drawing(text) {
        return None;
    }
    let ja = text.chars().filter(|&c| is_japanese(c)).count() >= 2;
    // English: a word of three letters at least
    let en = !ja && text.split(|c: char| !c.is_ascii_alphabetic()).any(|w| w.len() >= 3);
    let src = match (from, ja, en) {
        ("ja" | "auto", true, _) => "ja",
        ("en" | "auto", _, true) => "en",
        _ => return None,
    };
    (src != to).then_some(src)
}

/// Is a printed line character dialog? Printed by a character's dialogue
/// (kojo) code, or in quotes (「」 『』 for Japanese, " “” for English).
pub fn is_dialog(text: &str, kojo: bool, src: &str) -> bool {
    kojo || if src == "ja" { text.contains('「') || text.contains('『') } else { text.contains('"') || text.contains('“') }
}

/// The part of `s` worth translating: (what goes before, the words, what
/// goes after). Numbers, brackets and symbols around the words (`[100] - `,
/// `[641]`, `(1600/1600)`) stay as they are.
pub fn split_words(s: &str) -> (&str, &str, &str) {
    let word = |c: char| c.is_alphabetic();
    let Some(a) = s.find(word) else { return (s, "", "") };
    let b = s.char_indices().rfind(|&(_, c)| word(c)).map_or(s.len(), |(i, c)| i + c.len_utf8());
    // a sentence keeps its closing punctuation
    let mut b2 = b;
    for (i, c) in s[b..].char_indices() {
        if matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '」' | '』' | '"' | '”' | '…' | '、' | ',') {
            b2 = b + i + c.len_utf8();
        } else {
            break;
        }
    }
    (&s[..a], &s[a..b2], &s[b2..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let s = "「やあ」\n\"quoted\" \\ tab\t😀";
        let doc = format!("{{\"model\":\"x\",\"response\":{},\"done\":true}}", json_str(s));
        assert_eq!(json_field(&doc, "response").as_deref(), Some(s));
        assert_eq!(json_field(r#"{"translatedText": "Hi é 😀"}"#, "translatedText").as_deref(), Some("Hi é 😀"));
    }

    #[test]
    fn what_gets_translated() {
        assert_eq!(source_of("「おはよう、ご主人様」", "auto", "en"), Some("ja"));
        assert_eq!(source_of("Good morning, Master.", "auto", "en"), None);
        assert_eq!(source_of("Good morning, Master.", "auto", "es"), Some("en"));
        assert_eq!(source_of("Good morning, Master.", "ja", "es"), None);
        assert_eq!(source_of("「おはよう」", "en", "es"), None);
        assert_eq!(source_of("┃●━Valley┛┃　┏Valley━ForWes┗┓　┏┛森", "auto", "es"), None);
        assert_eq!(source_of("( 1600/ 1600) [12]", "auto", "es"), None);
        assert!(is_dialog("「おはよう、ご主人様」", false, "ja"));
        assert!(is_dialog("咲夜は微笑んだ。", true, "ja"));
        assert!(!is_dialog("咲夜は微笑んだ。", false, "ja"));
        assert!(is_dialog("\"Good morning,\" said Sakuya.", false, "en"));
        assert!(!is_dialog("You go to the kitchen.", false, "en"));
        assert_eq!(split_words("[100] - Wake Up"), ("[100] - ", "Wake Up", ""));
        assert_eq!(split_words("Ability Cards[641]"), ("", "Ability Cards", "[641]"));
        assert_eq!(split_words(" STA ( 1600/ 1600)"), (" ", "STA", " ( 1600/ 1600)"));
        assert_eq!(split_words("「おはよう」と言った。 "), ("「", "おはよう」と言った。", " "));
        assert_eq!(split_words("[42]"), ("[42]", "", ""));
    }

    /// translate-shell and DeepL get the arguments their tools expect
    /// (stand-ins that print what they were given).
    #[test]
    fn online_engines_command_lines() {
        if cfg!(windows) {
            return;
        }
        let bin = std::env::temp_dir().join(format!("emuera-tty-mt-{}", std::process::id()));
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["trans", "deepl"] {
            let p = bin.join(name);
            std::fs::write(&p, "#!/bin/sh\nprintf '%s|' \"$@\"; cat\n").unwrap();
            crate::sys::set_executable(&p).unwrap();
        }
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin.clone()];
        paths.extend(std::env::split_paths(&old));
        // (tests run on threads: only this one changes PATH and the key)
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
        std::env::set_var("DEEPL_AUTH_KEY", "test");
        let t = Engine::choose("translate-shell").unwrap().translate("「やあ」", "ja", "en", &mut None).unwrap();
        assert_eq!(t, "-b|-no-ansi|-no-autocorrect|-s|ja|-t|en|「やあ」");
        let d = Engine::choose("deepl").unwrap().translate("「やあ」", "ja", "pt", &mut None).unwrap();
        assert_eq!(d, "text|--from|JA|--to|PT-BR|「やあ」|");
        std::env::set_var("PATH", old);
        let _ = std::fs::remove_dir_all(&bin);
    }

    /// The kept-running Argos process, with a stand-in argostranslate
    /// module (the real one needs gigabytes of PyTorch).
    #[test]
    fn argos_process() {
        if cfg!(windows) || !which("python3") {
            return;
        }
        let dir = std::env::temp_dir().join(format!("emuera-tty-argos-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("argostranslate")).unwrap();
        std::fs::write(dir.join("argostranslate/__init__.py"), "").unwrap();
        std::fs::write(dir.join("argostranslate/translate.py"), "def translate(t, s, d):\n    return d + ':' + t[::-1]\n").unwrap();
        let mut a = {
            let mut c = Command::new("python3");
            c.args(["-c", ARGOS_SCRIPT, "en"]).env("PYTHONPATH", &dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = c.spawn().unwrap();
            let input = child.stdin.take().unwrap();
            let output = std::io::BufReader::new(child.stdout.take().unwrap());
            Argos { child, input, output }
        };
        assert_eq!(a.translate("あいう", "ja").unwrap(), "en:ういあ");
        assert_eq!(a.translate("Hello", "en").unwrap(), "en:olleH");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_engine() {
        if cfg!(windows) {
            return;
        }
        let e = Engine::Command("tr a-z A-Z".into());
        assert_eq!(e.translate("hello there\n", "ja", "en", &mut None).unwrap(), "HELLO THERE");
        let mut mt = Mt::start(e, "en");
        assert_eq!(mt.get("abc", "ja"), None);
        for _ in 0..100 {
            if mt.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(mt.get("abc", "ja"), Some("ABC"));
        assert!(!mt.busy());
    }
}
