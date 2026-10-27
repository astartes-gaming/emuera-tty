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
    /// FORCEWAIT: a wait message skipping stops at
    pub stop_skip: bool,
}

/// What INPUTMOUSEKEY reports (Emuera's RESULT:0 = 1 click, 2 wheel,
/// 3 key, 4 timeout). Positions are cells (0-based column, row); the VM
/// turns them into Emuera's pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum RawInput {
    /// `btn`: 0 left, 1 middle, 2 right; `button`: the printed button
    /// clicked, if any
    Click {
        btn: u8,
        cell: (usize, usize),
        button: Option<crate::vm::value::Value>,
    },
    /// +1 up, -1 down
    Wheel {
        up: bool,
        cell: (usize, usize),
    },
    /// Windows key code (Keys enum) and with modifiers (KeyData)
    Key {
        code: i64,
        data: i64,
    },
    Timeout,
    Quit,
    /// an Emuera-Anchor hotkey for the VM
    Hotkey(Hotkey),
}

/// A typed character as a Windows key code (letters upper-case, Shift
/// added for capitals), for INPUTMOUSEKEY.
pub fn char_keycode(c: char) -> (i64, i64) {
    match c {
        'a'..='z' => (c.to_ascii_uppercase() as i64, c.to_ascii_uppercase() as i64),
        'A'..='Z' => (c as i64, c as i64 | 0x10000),
        '0'..='9' | ' ' => (c as i64, c as i64),
        '\r' | '\n' => (13, 13),
        _ => (c as i64, c as i64),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputResult {
    Text(String),
    /// a line typed and entered: the VM expands its macro syntax (`\n`,
    /// `\e`, `(…)*n`, see `macros.rs`) into the inputs it stands for
    Line(String),
    /// the input, given with a right click (or Tab): from here on the WAIT
    /// family passes by itself until an input needs a value (Emuera's
    /// message skip)
    Skip(String),
    Timeout,
    Quit,
    /// the player pressed an Emuera-Anchor hotkey the VM acts on
    Hotkey(Hotkey),
}

/// Hotkeys handled by the VM (restarting is the frontend's own business).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hotkey {
    /// Ctrl-T: back to the title screen
    Title,
    /// F5 / Ctrl-E: read the ERB files again, keeping the game's state
    Reload,
    /// the game's settings (emuera.config) were changed: read them again
    Config,
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
    /// Called now and then while a script runs: has the user asked to quit
    /// (Ctrl+C) or gone away (terminal closed)? Other typed keys stay queued.
    fn poll_interrupt(&mut self) -> bool {
        false
    }
    /// Between the inputs of a typed macro: has the player pressed Esc to
    /// stop it (Emuera's `KillMacro`)?
    fn macro_stop(&mut self) -> bool {
        false
    }
    /// Show output while a script computes (every 150 ms)? Headless runs
    /// don't, so what they print doesn't depend on how fast the machine is.
    fn live_updates(&self) -> bool {
        true
    }

    /// The script has been computing for a while: show that it is busy
    /// (`con.running` names the function).
    fn busy(&mut self, _con: &Console) {}
    /// Show the console now even with REDRAW off (AWAIT).
    fn refresh(&mut self, con: &mut Console, res: &mut Resources) {
        self.flush(con, res);
    }
    /// INPUTMOUSEKEY: wait for a click, a wheel step or a key, or the
    /// timeout. Without a mouse: a typed number counts as clicking that
    /// button, Enter / other text as key presses.
    fn input_raw(&mut self, con: &mut Console, res: &mut Resources, timeout_ms: Option<u64>) -> RawInput {
        let req = InputReq { kind: InputKind::Str, one: false, timeout_ms, stop_skip: false };
        match self.input(con, res, &req) {
            InputResult::Quit => RawInput::Quit,
            InputResult::Hotkey(h) => RawInput::Hotkey(h),
            InputResult::Timeout => RawInput::Timeout,
            InputResult::Text(t) | InputResult::Line(t) | InputResult::Skip(t) => match t.trim().parse::<i64>() {
                Ok(v) => RawInput::Click { btn: 0, cell: (0, 0), button: Some(crate::vm::value::Value::Int(v)) },
                Err(_) => {
                    let (code, data) = t.chars().next().map(char_keycode).unwrap_or((13, 13));
                    RawInput::Key { code, data }
                }
            },
        }
    }
    /// Where the mouse is (0-based column, row), if the frontend knows.
    fn mouse_cell(&self) -> Option<(usize, usize)> {
        None
    }
    /// AWAIT: show the console and, for `ms`, take what the player does
    /// without waiting for an answer, as Emuera's window keeps doing:
    /// typing goes into the input box, the mouse moves the hover, keys and
    /// clicks are noted for GETKEY. True when they asked to quit.
    fn await_input(&mut self, con: &mut Console, res: &mut Resources, ms: u64) -> bool {
        self.refresh(con, res);
        if self.is_fullscreen() && ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        false
    }
    /// GETTEXTBOX: what is typed in the input box.
    fn textbox(&self) -> String {
        String::new()
    }
    /// SETTEXTBOX / CLEARTEXTBOX.
    fn set_textbox(&mut self, _s: &str) {}
    /// MOUSEB: the value of the button under the pointer ("" for none).
    fn hover_value(&self) -> String {
        String::new()
    }
    /// GETKEY (`triggered` false: held now) / GETKEYTRIGGERED (pressed
    /// since asked last), by Windows key code (1 left button, 2 right…).
    fn key_state(&mut self, _code: i64, _triggered: bool) -> bool {
        false
    }
    /// Override the width, for frontends with no real terminal size.
    fn set_cols(&mut self, _cols: usize) {}
    /// The game folder and where its saves are (for the save manager).
    fn set_game(&mut self, _game: &std::path::Path, _save_dir: &std::path::Path) {}
    /// Show a Yes/No confirmation dialog (for infinite loop warning, etc.).
    /// Returns true for Yes, false for No. Default: false (safe default).
    fn confirm(&mut self, _con: &mut Console, _res: &mut Resources, _msg: &str) -> bool {
        false
    }
    /// Show a second confirmation dialog with custom button labels.
    /// Returns true for first button (e.g., "Exit anyway"), false for second (e.g., "Wait...").
    fn confirm2(&mut self, _con: &mut Console, _res: &mut Resources, _msg: &str) -> bool {
        false
    }
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
    /// EMUERA_LOG_STYLE=emuera: only the lines the game printed, as
    /// Emuera's log has them (no prompt or blank line at each input), so
    /// the test suite's expected output compares with emuera.log
    log_style: bool,
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
            log_style: std::env::var("EMUERA_LOG_STYLE").as_deref() == Ok("emuera"),
        }
    }

    fn line_out(l: &Line) -> String {
        Console::line_text(l)
    }
}

impl Frontend for Headless {
    fn live_updates(&self) -> bool {
        false
    }
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
        if self.log_style {
            self.flush(con, res);
        } else if !con.cur.is_empty() {
            // show partial line as a prompt
            let partial =
                Line { segs: con.cur.clone(), align: con.align, gen: con.gen, temporary: false, cols: con.width + 1, kojo: false, src: 0, wait: con.waits };
            self.flush(con, res);
            print!("{}", Self::line_out(&partial));
        } else {
            self.flush(con, res);
        }
        if req.kind == InputKind::Wait {
            if !self.log_style {
                println!();
            }
            return InputResult::Text(String::new());
        }
        // EMUERA_LIST_BUTTONS=1: the values the buttons on screen send, even
        // those that don't show a number (map cells), for scripted play
        if std::env::var_os("EMUERA_LIST_BUTTONS").is_some() {
            let vals: Vec<String> = con.button_values().iter().map(|v| v.to_string()).collect();
            if !vals.is_empty() {
                println!("[buttons: {}]", vals.join(", "));
            }
        }
        let mut s = String::new();
        match self.lines_in.read_line(&mut s) {
            Ok(0) | Err(_) => InputResult::Quit,
            Ok(_) => {
                let s = s.trim_end_matches(['\r', '\n']).to_string();
                if self.echo_input {
                    println!("> {}", s);
                } else if !self.log_style {
                    println!();
                }
                InputResult::Line(s)
            }
        }
    }

    fn cols(&self) -> usize {
        self.width
    }

    fn set_cols(&mut self, cols: usize) {
        self.width = cols;
    }

    fn confirm(&mut self, _con: &mut Console, _res: &mut Resources, _msg: &str) -> bool {
        true
    }

    fn confirm2(&mut self, _con: &mut Console, _res: &mut Resources, _msg: &str) -> bool {
        true
    }
}
