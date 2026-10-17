//! Frontend abstraction: rendering the console and collecting input.
//!
//! The VM never talks to the terminal directly: when it needs input it
//! calls [`Frontend::input`], which shows the console and returns what was
//! typed or clicked. [`Headless`] is the plain stdin/stdout frontend used by
//! `--headless`, tests and scripts; the full-screen one is `tty::Tty`.

use crate::console::{Console, Line};
use crate::resources::Resources;
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
/// What the script is waiting for.
pub enum InputKind {
    /// INPUT / TINPUT / ONEINPUT
    Int,
    /// INPUTS and string variants
    Str,
    /// WAIT / WAITANYKEY: any key or click continues
    Wait,
}

#[derive(Debug, Clone)]
pub struct InputReq {
    pub kind: InputKind,
    /// ONEINPUT: a single key is enough
    pub one: bool,
    /// TINPUT / TWAIT: give up after this long
    pub timeout_ms: Option<u64>,
    /// INPUTMOUSEKEY: report clicks/keys. Set by the VM; frontends treat
    /// it like an ordinary input for now.
    #[allow(dead_code)]
    pub mouse: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputResult {
    Text(String),
    Timeout,
    Quit,
}

pub trait Frontend {
    /// Show whatever is new in the console without waiting for input.
    fn flush(&mut self, con: &mut Console, res: &mut Resources);
    /// Show the console and block until the user answers `req`.
    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult;
    /// Console width in half-width columns.
    fn cols(&self) -> usize;
    /// Console height in lines (CLIENTHEIGHT); headless has no real height.
    fn rows(&self) -> usize {
        40
    }
    /// Does this frontend own the whole screen (so stray stderr output
    /// would corrupt it)?
    fn is_fullscreen(&self) -> bool {
        false
    }
    /// Override the width, for frontends with no real terminal size.
    fn set_cols(&mut self, _cols: usize) {}
    fn shutdown(&mut self) {}
}

/// Line-oriented frontend for tests / pipes: prints plain text, reads stdin.
/// Line-oriented frontend: prints finished lines to stdout, reads answers
/// line by line from stdin; WAITs continue immediately. End of input quits.
pub struct Headless {
    printed: usize,
    gen_seen: usize,
    dropped_seen: usize,
    pub echo_input: bool,
    lines_in: Box<dyn BufRead>,
    width: usize,
}

impl Headless {
    pub fn new(echo_input: bool) -> Headless {
        Headless {
            printed: 0,
            gen_seen: 0,
            dropped_seen: 0,
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
        // Lines can be removed (CLEARLINE, REUSELASTLINE, history limit)
        // and new ones added since the last flush: rewind past removals.
        self.printed = self.printed.saturating_sub(con.dropped_front - self.dropped_seen);
        self.dropped_seen = con.dropped_front;
        self.printed = self.printed.min(con.low_water).min(con.lines.len());
        con.low_water = usize::MAX;
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

    fn set_cols(&mut self, cols: usize) {
        self.width = cols;
    }
}
