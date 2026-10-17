//! Frontend abstraction: rendering the console and collecting input.

use crate::console::{Console, Line};
use crate::resources::Resources;
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputKind {
    Int,
    Str,
    /// WAIT / WAITANYKEY: any key or click continues
    Wait,
}

#[derive(Debug, Clone)]
pub struct InputReq {
    pub kind: InputKind,
    pub one: bool,
    pub timeout_ms: Option<u64>,
    /// INPUTMOUSEKEY: report clicks/keys
    pub mouse: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputResult {
    Text(String),
    Timeout,
    Quit,
}

pub trait Frontend {
    fn flush(&mut self, con: &mut Console, res: &mut Resources);
    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult;
    /// Console width in half-width columns.
    fn cols(&self) -> usize;
    fn shutdown(&mut self) {}
}

/// Line-oriented frontend for tests / pipes: prints plain text, reads stdin.
pub struct Headless {
    printed: usize,
    gen_seen: usize,
    pub echo_input: bool,
    lines_in: Box<dyn BufRead>,
    width: usize,
}

impl Headless {
    pub fn new(echo_input: bool) -> Headless {
        Headless {
            printed: 0,
            gen_seen: 0,
            echo_input,
            lines_in: Box::new(std::io::BufReader::new(std::io::stdin())),
            width: 80,
        }
    }

    fn line_out(l: &Line) -> String {
        Console::line_text(l)
    }
}

impl Frontend for Headless {
    fn flush(&mut self, con: &mut Console, _res: &mut Resources) {
        // Lines can be removed (CLEARLINE); clamp.
        if self.printed > con.lines.len() {
            self.printed = con.lines.len();
        }
        let mut out = std::io::stdout().lock();
        for l in &con.lines[self.printed..] {
            let _ = writeln!(out, "{}", Self::line_out(l));
        }
        self.printed = con.lines.len();
        let _ = out.flush();
        con.dirty = false;
        self.gen_seen = con.gen as usize;
    }

    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        if !con.cur.is_empty() {
            // show partial line as a prompt
            let partial = Line { segs: con.cur.clone(), align: con.align, gen: con.gen, temporary: false };
            self.flush(con, res);
            print!("{}", Self::line_out(&partial));
        } else {
            self.flush(con, res);
        }
        if req.kind == InputKind::Wait {
            println!();
            return InputResult::Text(String::new());
        }
        let mut s = String::new();
        match self.lines_in.read_line(&mut s) {
            Ok(0) | Err(_) => InputResult::Quit,
            Ok(_) => {
                let s = s.trim_end_matches(['\r', '\n']).to_string();
                if self.echo_input {
                    println!("> {}", s);
                } else {
                    println!();
                }
                InputResult::Text(s)
            }
        }
    }

    fn cols(&self) -> usize {
        self.width
    }
}
