//! `--mcp`: the game as a Model Context Protocol server on stdin/stdout,
//! for AI agents (Claude Code, Codex, OpenCode…) doing QA or playing.
//!
//! It is a frontend like `Headless`: the game runs as usual, and whenever
//! it waits for input the frontend answers the client's JSON-RPC requests
//! (one JSON object a line, MCP's stdio transport) until one of them is
//! the input. Requests that need the VM (`eval`, `run`, `state`) go through
//! the debug panel's channel ([`DebugAction`]): the VM runs them and asks
//! for the input again, and the answer is what they left in the debug
//! console. Debug mode is on, as with `--game-debug`.
//!
//! Nothing but protocol messages may reach stdout; the game's text goes
//! into the answers.

use crate::console::{Console, Line};
use crate::frontend::{char_keycode, DebugAction, Frontend, InputKind, InputReq, InputResult, RawInput};
use crate::json::{quote, Json};
use crate::resources::Resources;
use crate::vm::value::Value;
use std::io::{self, BufRead, Write};

/// How many WAITs an `input` passes by itself before answering anyway (a
/// game that WAITs in a loop would otherwise never answer).
const MAX_AUTO_WAITS: u32 = 500;
/// The most game text one answer carries (its end is kept).
const MAX_TEXT: usize = 60_000;

/// A request waiting for the game to come back to an input.
enum Pending {
    /// `input`: answered with what the game printed until its next input
    Input(Json),
    /// `eval` / `run` / `state`: answered from the debug console (from
    /// `log` bytes on), the trace, and what the game printed
    Debug { id: Json, log: usize, trace: bool, buf: usize },
}

/// What the client asked the waiting input for.
enum Answer {
    Input(String),
    Timeout,
    Debug(DebugAction),
    Quit,
}

pub struct Mcp {
    lines_in: Box<dyn BufRead>,
    width: usize,
    /// lines already put into `buf` (Headless's `printed`)
    printed: usize,
    dropped_seen: usize,
    /// the game's text since the last answer to `input`
    buf: String,
    pending: Option<Pending>,
    /// pass WAITs by themselves (an `input`'s `stop_at_wait` false)
    auto_wait: bool,
    waits_passed: u32,
    /// what the game waits for now, said in answers
    waiting: String,
    done: bool,
}

impl Mcp {
    pub fn new() -> Mcp {
        Mcp {
            lines_in: Box::new(io::BufReader::new(io::stdin())),
            width: 80,
            printed: 0,
            dropped_seen: 0,
            buf: String::new(),
            pending: None,
            auto_wait: true,
            waits_passed: 0,
            waiting: String::new(),
            done: false,
        }
    }

    fn send(&self, msg: &str) {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "{}", msg);
        let _ = out.flush();
    }

    fn reply(&self, id: &Json, result: &str) {
        self.send(&format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}", id_json(id), result));
    }

    fn error(&self, id: &Json, code: i32, msg: &str) {
        self.send(&format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":{}}}}}", id_json(id), code, quote(msg)));
    }

    /// A tool's result: text, and whether it is an error.
    fn tool_reply(&self, id: &Json, text: &str, is_error: bool) {
        self.reply(id, &format!("{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{}}}", quote(text), is_error));
    }

    /// The game's text gathered for the next answer, its end if too long.
    fn take_buf(&mut self) -> String {
        let s = std::mem::take(&mut self.buf);
        if s.len() <= MAX_TEXT {
            return s;
        }
        let mut cut = s.len() - MAX_TEXT;
        while !s.is_char_boundary(cut) {
            cut += 1;
        }
        format!("[… {} bytes before this left out]\n{}", cut, &s[cut..])
    }

    /// The text of the line being printed (the prompt), if any.
    fn partial(con: &Console) -> String {
        if con.cur.is_empty() {
            return String::new();
        }
        let l = Line { segs: con.cur.clone(), align: con.align, gen: con.gen, temporary: false, cols: con.width + 1, kojo: false, src: 0, wait: con.waits };
        Console::line_text(&l)
    }

    fn buttons_text(con: &Console) -> String {
        let b = con.button_labels();
        if b.is_empty() {
            return "(no buttons)".into();
        }
        b.iter().map(|(v, t)| format!("{} → {}", value_text(v), if t.is_empty() { "(no text)" } else { t })).collect::<Vec<_>>().join("\n")
    }

    /// The status block that ends an answer: what the game waits for and
    /// the buttons that answer it.
    fn status(&self, con: &Console) -> String {
        let mut s = format!("--- waiting for: {}", self.waiting);
        let p = Self::partial(con);
        if !p.trim().is_empty() {
            s += &format!("\n--- prompt: {}", p.trim_end());
        }
        s += "\n--- buttons:\n";
        s += &Self::buttons_text(con);
        s
    }

    /// Answer the request waiting for the game, now that it is back at an
    /// input (or has ended).
    fn settle(&mut self, con: &Console) {
        match self.pending.take() {
            None => {}
            Some(Pending::Input(id)) => {
                let mut text = self.take_buf();
                if self.waits_passed > 0 {
                    text += &format!("\n({} WAIT{} passed)", self.waits_passed, if self.waits_passed == 1 { "" } else { "s" });
                }
                let text = format!("{}\n{}", text.trim_end_matches('\n'), self.status(con));
                self.tool_reply(&id, text.trim_start_matches('\n'), false);
            }
            Some(Pending::Debug { id, log, trace, buf }) => {
                let out = con.debug.log.get(log..).unwrap_or("");
                // (the console echoes the command first)
                let out = out.split_once('\n').map_or("", |x| x.1).trim_end().to_string();
                // (only what the command printed: the rest waits for `input`)
                let printed = self.buf.get(buf..).unwrap_or("").to_string();
                self.buf.truncate(buf.min(self.buf.len()));
                let mut text = if trace { con.debug.trace.trim_end().to_string() } else { out.clone() };
                if trace && !out.is_empty() {
                    text = format!("{}\n{}", out, text);
                }
                if !printed.trim().is_empty() {
                    text += &format!("\n--- printed to the game's console:\n{}", printed.trim_end());
                }
                if trace {
                    if !con.debug.phase.is_empty() {
                        text += &format!("\n--- game phase: {}", con.debug.phase);
                    }
                    text += &format!("\n{}", self.status(con));
                }
                let is_error = !trace && con.debug.failed;
                let text = text.trim_start_matches('\n');
                self.tool_reply(&id, if text.is_empty() { "(done, no output)" } else { text }, is_error);
            }
        }
    }

    /// Serve requests until one of them answers the waiting input.
    fn serve(&mut self, con: &Console) -> Answer {
        self.settle(con);
        loop {
            let mut line = String::new();
            match self.lines_in.read_line(&mut line) {
                Ok(0) | Err(_) => return Answer::Quit,
                Ok(_) => {}
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some(msg) = Json::parse(line) else {
                self.error(&Json::Null, -32700, "parse error");
                continue;
            };
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(Json::str).unwrap_or("");
            let Some(id) = id else {
                // a notification (initialized, cancelled…): nothing to say
                continue;
            };
            let params = msg.get("params").cloned().unwrap_or(Json::Obj(Vec::new()));
            match method {
                "initialize" => {
                    let ver = params.get("protocolVersion").and_then(Json::str).unwrap_or("2025-06-18").to_string();
                    self.reply(
                        &id,
                        &format!(
                            "{{\"protocolVersion\":{},\"capabilities\":{{\"tools\":{{}}}},\"serverInfo\":{{\"name\":\"emuera-tty\",\"version\":{}}},\"instructions\":{}}}",
                            quote(&ver),
                            quote(env!("CARGO_PKG_VERSION")),
                            quote(INSTRUCTIONS)
                        ),
                    );
                }
                "ping" => self.reply(&id, "{}"),
                "tools/list" => self.reply(&id, &format!("{{\"tools\":{}}}", TOOLS.replace('\n', ""))),
                "tools/call" => {
                    let name = params.get("name").and_then(Json::str).unwrap_or("").to_string();
                    let args = params.get("arguments").cloned().unwrap_or(Json::Obj(Vec::new()));
                    let arg = |k: &str| args.get(k).map(|v| v.str().map(String::from).unwrap_or_else(|| json_scalar(v))).unwrap_or_default();
                    match name.as_str() {
                        "screen" => {
                            let n = args.get("lines").and_then(|v| if let Json::Num(n) = v { Some(*n as usize) } else { None }).unwrap_or(40).max(1);
                            let from = con.lines.len().saturating_sub(n);
                            let mut text: String = con.lines[from..].iter().map(|l| Console::line_text(l) + "\n").collect();
                            // (seen now: the next `input` answers with what follows)
                            self.buf.clear();
                            text += &self.status(con);
                            self.tool_reply(&id, &text, false);
                        }
                        "buttons" => self.tool_reply(&id, &Self::buttons_text(con), false),
                        "input" => {
                            self.auto_wait = !matches!(args.get("stop_at_wait"), Some(Json::Bool(true)));
                            self.waits_passed = 0;
                            self.pending = Some(Pending::Input(id));
                            if matches!(args.get("timeout"), Some(Json::Bool(true))) {
                                return Answer::Timeout;
                            }
                            return Answer::Input(arg("value"));
                        }
                        "eval" | "run" => {
                            let src = arg(if name == "eval" { "expression" } else { "statement" });
                            if src.trim().is_empty() {
                                self.tool_reply(&id, "nothing to run", true);
                                continue;
                            }
                            // (a statement that doesn't parse as one would be
                            // taken as an expression: `eval` asks for one)
                            self.pending = Some(Pending::Debug { id, log: con.debug.log.len(), trace: false, buf: self.buf.len() });
                            let line = src.lines().next().unwrap_or("").to_string();
                            return Answer::Debug(if name == "run" { DebugAction::Call(line) } else { DebugAction::Command(line) });
                        }
                        "state" => {
                            self.pending = Some(Pending::Debug { id, log: con.debug.log.len(), trace: true, buf: self.buf.len() });
                            return Answer::Debug(DebugAction::Refresh);
                        }
                        _ => self.tool_reply(&id, &format!("unknown tool: {}", name), true),
                    }
                }
                _ => self.error(&id, -32601, &format!("method not found: {}", method)),
            }
        }
    }
}

impl Frontend for Mcp {
    fn live_updates(&self) -> bool {
        false
    }

    fn flush(&mut self, con: &mut Console, _res: &mut Resources) {
        // (as Headless: rewind past lines removed since)
        self.printed = self.printed.saturating_sub(con.dropped_front - self.dropped_seen);
        self.dropped_seen = con.dropped_front;
        self.printed = self.printed.min(con.low_water).min(con.lines.len());
        con.low_water = usize::MAX;
        for l in &con.lines[self.printed..] {
            self.buf += &Console::line_text(l);
            self.buf.push('\n');
        }
        self.printed = con.lines.len();
        con.dirty = false;
    }

    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        self.flush(con, res);
        if req.kind == InputKind::Wait && self.auto_wait && self.pending.is_some() && self.waits_passed < MAX_AUTO_WAITS {
            self.waits_passed += 1;
            return InputResult::Text(String::new());
        }
        self.waiting = match req.kind {
            InputKind::Int => "a number (INPUT)",
            InputKind::Str => "text (INPUTS)",
            InputKind::Wait => "any input to continue (WAIT)",
        }
        .to_string();
        if req.one {
            self.waiting += ", one key";
        }
        if let Some(ms) = req.timeout_ms {
            self.waiting += &format!(", timed ({} ms; input with timeout=true lets it run out)", ms);
        }
        match self.serve(con) {
            Answer::Input(s) => InputResult::Line(s),
            Answer::Timeout => InputResult::Timeout,
            Answer::Debug(a) => InputResult::Debug(a),
            Answer::Quit => InputResult::Quit,
        }
    }

    fn input_raw(&mut self, con: &mut Console, res: &mut Resources, timeout_ms: Option<u64>) -> RawInput {
        self.flush(con, res);
        self.waiting = "a click or a key (INPUTMOUSEKEY): a button's value clicks it, key:NAME or key:CODE presses a key".to_string();
        if let Some(ms) = timeout_ms {
            self.waiting += &format!(", timed ({} ms; input with timeout=true lets it run out)", ms);
        }
        match self.serve(con) {
            Answer::Quit => RawInput::Quit,
            Answer::Timeout => RawInput::Timeout,
            Answer::Debug(a) => RawInput::Debug(a),
            Answer::Input(s) => raw_answer(con, &s),
        }
    }

    fn cols(&self) -> usize {
        self.width
    }

    fn set_cols(&mut self, cols: usize) {
        self.width = cols;
    }

    /// No one to ask about a long computation: it goes on (as Headless).
    fn confirm(&mut self, _con: &mut Console, _res: &mut Resources, _msg: &str) -> bool {
        false
    }

    /// The game ended: the request waiting for it gets what it printed.
    fn shutdown(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        if let Some(Pending::Input(id) | Pending::Debug { id, .. }) = self.pending.take() {
            let text = format!("{}\n--- the game has ended", self.take_buf().trim_end());
            self.tool_reply(&id, text.trim_start(), false);
        }
    }
}

/// An INPUTMOUSEKEY answer: a button's value clicks it, `key:…` presses a
/// key, anything else presses its first character.
fn raw_answer(con: &Console, s: &str) -> RawInput {
    let click = |v: Value| RawInput::Click { btn: 0, cell: (0, 0), button: Some(v) };
    if let Ok(n) = s.trim().parse::<i64>() {
        return click(Value::Int(n));
    }
    if let Some((v, _)) = con.button_labels().into_iter().find(|(v, _)| matches!(v, Value::Str(t) if t.as_str() == s)) {
        return click(v);
    }
    if let Some(k) = s.strip_prefix("key:") {
        let code = match k.to_ascii_lowercase().as_str() {
            "enter" | "return" => 13,
            "esc" | "escape" => 27,
            "space" => 32,
            "tab" => 9,
            "backspace" => 8,
            "left" => 37,
            "up" => 38,
            "right" => 39,
            "down" => 40,
            "pageup" => 33,
            "pagedown" => 34,
            "home" => 36,
            "end" => 35,
            k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<i64>().is_ok_and(|n| (1..=12).contains(&n)) => 111 + k[1..].parse::<i64>().unwrap_or(1),
            k => match k.parse::<i64>() {
                Ok(n) => n,
                Err(_) => return char_keycode(k.chars().next().unwrap_or('\r')).into_key(),
            },
        };
        return RawInput::Key { code, data: code };
    }
    char_keycode(s.chars().next().unwrap_or('\r')).into_key()
}

trait IntoKey {
    fn into_key(self) -> RawInput;
}

impl IntoKey for (i64, i64) {
    fn into_key(self) -> RawInput {
        RawInput::Key { code: self.0, data: self.1 }
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("{:?}", s.as_str()),
        v => v.clone().into_string(),
    }
}

/// A request's id as JSON (a number or a string).
fn id_json(id: &Json) -> String {
    match id {
        Json::Str(s) => quote(s),
        v => json_scalar(v),
    }
}

/// A number, boolean or null as JSON text (whole numbers without `.0`).
fn json_scalar(v: &Json) -> String {
    match v {
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => format!("{}", *n as i64),
        Json::Num(n) => format!("{}", n),
        Json::Bool(b) => b.to_string(),
        Json::Str(s) => quote(s),
        _ => "null".into(),
    }
}

const INSTRUCTIONS: &str = "An Emuera (eraBasic) game running in emuera-tty. The game is paused at an input whenever a tool answers. \
`input` answers it (a number, text, or a button's value) and returns everything the game printed until its next input, then what it waits for and the buttons on screen. \
WAITs pass by themselves unless stop_at_wait is true. `screen` shows the recent lines, `buttons` the live buttons. \
`eval` evaluates an ERB expression (variables like FLAG:3, CFLAG:MASTER:10, functions and #FUNCTIONs); `run` runs one ERB statement (e.g. FLAG:3 = 1, CALL FUNC, PRINTL text) the way Emuera's debug console does: flow control and inputs are refused. \
`state` gives the line running and the call stack. [IF_DEBUG] blocks and ;#; lines run only when emuera-tty was started with --game-debug as well.";

const TOOLS: &str = r#"[
{"name":"screen","description":"The last lines of the game's console, what the game waits for, and the live buttons.","inputSchema":{"type":"object","properties":{"lines":{"type":"integer","description":"how many lines (default 40)"}}}},
{"name":"buttons","description":"The buttons the current input accepts: their values and the text they show.","inputSchema":{"type":"object","properties":{}}},
{"name":"input","description":"Answer the game's input and run until its next one; returns what was printed meanwhile, what it waits for next, and the buttons. For a number input give the number; a button's value clicks it; at a mouse/key input (INPUTMOUSEKEY) key:Enter, key:Esc, key:Up, key:F1 or key:CODE press keys.","inputSchema":{"type":"object","properties":{"value":{"type":"string","description":"what to type, or a button's value"},"stop_at_wait":{"type":"boolean","description":"stop at each WAIT instead of passing it (default false)"},"timeout":{"type":"boolean","description":"let a timed input run out instead of answering it"}},"required":["value"]}},
{"name":"eval","description":"Evaluate an ERB expression in the game's current state and return its value: variables (FLAG:3, CFLAG:MASTER:10, NAME:TARGET, LOCAL@FUNC), arithmetic, built-in functions and the game's #FUNCTIONs.","inputSchema":{"type":"object","properties":{"expression":{"type":"string"}},"required":["expression"]}},
{"name":"run","description":"Run one ERB statement now, as Emuera's debug console does: assignments (FLAG:3 = 5, STR:0 = text, NAME:1 '= \"Ann\"), PRINTL, ADDCHARA…, and also CALL FUNC(args) / CALLF / TRYCALL (beyond Emuera's console: run a game's setup functions). Other flow control (GOTO, JUMP, RETURN, BEGIN), inputs and WAITs are refused. Returns what it printed.","inputSchema":{"type":"object","properties":{"statement":{"type":"string"}},"required":["statement"]}},
{"name":"state","description":"Where the script is: the line running, the call stack (Emuera's debug window format; none at the engine's own SHOP/TRAIN menus), the game phase (TITLE, FIRST, SHOP, TRAIN…), what the game waits for and the buttons.","inputSchema":{"type":"object","properties":{}}}
]"#;
