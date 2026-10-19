//! Live translation of character dialog with a translator running on this
//! machine (nothing is sent anywhere else): Ollama, LibreTranslate,
//! translateLocally (Bergamot), argos-translate, or any command given in
//! `EMUERA_TRANSLATE_CMD` (text on its stdin, translation on its stdout).
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
    /// `argos-translate --from-lang ja --to-lang <to>`
    Argos,
    /// a shell command
    Command(String),
}

impl Engine {
    /// The translator to use: `EMUERA_TRANSLATE_CMD` if set, else the first
    /// of Ollama, LibreTranslate, translateLocally and argos-translate found.
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
            return Some(Engine::Bergamot { model: model.unwrap_or_else(|| "ja-en-tiny".into()) });
        }
        if which("argos-translate") {
            return Some(Engine::Argos);
        }
        None
    }

    pub fn describe(&self) -> String {
        match self {
            Engine::Ollama { model } => format!("Ollama ({})", model),
            Engine::Libre { url } => format!("LibreTranslate ({})", url),
            Engine::Bergamot { model } => format!("translateLocally ({})", model),
            Engine::Argos => "argos-translate".into(),
            Engine::Command(c) => format!("`{}`", c),
        }
    }

    /// Translate one line of Japanese into `to`.
    fn translate(&self, text: &str, to: &str) -> Result<String, String> {
        let out = match self {
            Engine::Ollama { model } => {
                let prompt = format!(
                    "Translate this line of Japanese dialogue from a game into natural {}. Keep names as they are. Reply with the translation only, no notes.\n\n{}",
                    language_name(to),
                    text
                );
                let body = format!("{{\"model\":{},\"prompt\":{},\"stream\":false,\"options\":{{\"temperature\":0.2}}}}", json_str(model), json_str(&prompt));
                let r = http("127.0.0.1:11434", "POST", "/api/generate", &body, Duration::from_secs(120)).ok_or("Ollama didn't answer")?;
                json_field(&r, "response").ok_or("Ollama gave no translation")?
            }
            Engine::Libre { url } => {
                let body = format!("{{\"q\":{},\"source\":\"ja\",\"target\":{},\"format\":\"text\"}}", json_str(text), json_str(to));
                let r = http(url, "POST", "/translate", &body, Duration::from_secs(60)).ok_or("LibreTranslate didn't answer")?;
                json_field(&r, "translatedText").ok_or("LibreTranslate gave no translation")?
            }
            Engine::Bergamot { model } => run(Command::new("translateLocally").args(["-m", model]), text)?,
            Engine::Argos => run(Command::new("argos-translate").args(["--from-lang", "ja", "--to-lang", to]).arg(text), "")?,
            Engine::Command(c) => {
                let mut cmd = if cfg!(windows) { Command::new("cmd") } else { Command::new("sh") };
                cmd.arg(if cfg!(windows) { "/C" } else { "-c" }).arg(c).env("EMUERA_TRANSLATE_TO", to);
                run(&mut cmd, text)?
            }
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

/// What became of a line.
enum State {
    Waiting,
    Done(String),
    Failed,
}

/// The translator thread and what it has translated.
pub struct Mt {
    to: Sender<String>,
    from: Receiver<(String, Result<String, String>)>,
    seen: HashMap<String, State>,
    /// the last failure, for the settings page
    pub last_error: Option<String>,
}

impl Mt {
    pub fn start(engine: Engine, lang: &str) -> Mt {
        let (to, rx) = channel::<String>();
        let (tx, from) = channel();
        let (e, lang) = (engine, lang.to_string());
        let _ = std::thread::Builder::new().name("translate".into()).spawn(move || {
            for text in rx {
                let r = e.translate(&text, &lang);
                if tx.send((text, r)).is_err() {
                    break;
                }
            }
        });
        Mt { to, from, seen: HashMap::default(), last_error: None }
    }

    /// The translation of `text` if it is in; asks for it if never asked.
    pub fn get(&mut self, text: &str) -> Option<&str> {
        if !self.seen.contains_key(text) {
            // (bounded: a long session forgets the oldest… all of them)
            if self.seen.len() > 20_000 {
                self.seen.clear();
            }
            self.seen.insert(text.to_string(), State::Waiting);
            let _ = self.to.send(text.to_string());
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

/// Is a printed line character dialog worth translating? It has Japanese
/// in it, and was printed by a character's dialogue (kojo) code or is in
/// 「」 / 『』 quotes; never a map or a box drawing (lines and blocks), and
/// never a line with buttons (menus).
pub fn is_dialog(text: &str, kojo: bool) -> bool {
    let japanese = text.chars().filter(|&c| matches!(c as u32, 0x3040..=0x30FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF)).count();
    if japanese < 2 {
        return false;
    }
    let drawing = text.chars().filter(|&c| matches!(c as u32, 0x2500..=0x25FF | 0x2800..=0x28FF)).count();
    if drawing * 10 > text.chars().count() {
        return false;
    }
    kojo || text.contains('「') || text.contains('『')
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
    fn only_dialog() {
        assert!(is_dialog("「おはよう、ご主人様」", false));
        assert!(is_dialog("咲夜は微笑んだ。", true));
        assert!(!is_dialog("咲夜は微笑んだ。", false));
        assert!(!is_dialog("Good morning, Master.", true));
        assert!(!is_dialog("┃●━Valley┛┃　┏Valley━ForWes┗┓　┏┛森", true));
    }

    #[test]
    fn command_engine() {
        if cfg!(windows) {
            return;
        }
        let e = Engine::Command("tr a-z A-Z".into());
        assert_eq!(e.translate("hello there\n", "en").unwrap(), "HELLO THERE");
        let mut mt = Mt::start(e, "en");
        assert_eq!(mt.get("abc"), None);
        for _ in 0..100 {
            if mt.poll() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(mt.get("abc"), Some("ABC"));
        assert!(!mt.busy());
    }
}
