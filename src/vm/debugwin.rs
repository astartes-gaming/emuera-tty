//! Emuera's debug window (`UI/Framework/Forms/DebugDialog.cs`), the VM's
//! side: the watched expressions' values, the stack trace, the debug
//! console (DEBUGPRINT's output and debug commands), and the two files
//! Emuera keeps them in, `debug/watchlist.csv` and `debug/console.log` in
//! the game's folder, so they move between Emuera and emuera-tty. The
//! frontend shows them (`tty/debugpanel.rs`) and asks for updates with
//! [`crate::frontend::DebugAction`]. Only in debug mode (`--game-debug`,
//! Emuera's `-Debug`).

use super::Vm;
use crate::frontend::DebugAction;
use std::path::PathBuf;

/// Statements a debug command can't be (Emuera's DebugCommand: flow
/// control, waiting for input, what a #FUNCTION can't do, and these).
const NOT_IN_DEBUG: &[&str] = &["PUTFORM", "UPCHECK", "CUPCHECK", "SAVEDATA"];

impl Vm {
    fn debug_dir(&self) -> PathBuf {
        self.game_dir.join("debug")
    }

    /// Debug mode on: read the watch list Emuera or a previous session
    /// left (`DebugDialog.loadWatchList`).
    pub fn debug_start(&mut self) {
        self.con.debug.enabled = true;
        if let Ok(raw) = std::fs::read(self.debug_dir().join("watchlist.csv")) {
            self.con.debug.watches = crate::loader::decode(&raw).lines().map(str::trim_end).filter(|l| !l.is_empty()).map(String::from).collect();
        }
    }

    /// What the debug panel asked for.
    pub fn debug_action(&mut self, a: DebugAction) {
        if !self.con.debug.enabled {
            return;
        }
        match a {
            DebugAction::Refresh => {}
            DebugAction::Command(c) => self.debug_command(&c),
            DebugAction::Close => {
                self.con.debug.open = false;
                self.debug_save();
                return;
            }
        }
        self.debug_refresh();
    }

    /// The watches' values and the stack trace, as of now.
    pub fn debug_refresh(&mut self) {
        let watches = self.con.debug.watches.clone();
        self.con.debug.values = watches.iter().map(|w| self.debug_value(w)).collect();
        self.con.debug.trace = self.debug_trace();
        self.debug_save_watches();
    }

    /// An expression's value as the watch list shows it, or why there is
    /// none (DebugDialog.getValueString).
    fn debug_value(&mut self, src: &str) -> String {
        let e = match crate::expr::parse_expr(src.trim(), &self.prog.defines) {
            Ok(e) => crate::parser::upper_expr(e),
            Err(e) => return e,
        };
        let depth = self.frames.len();
        let r = self.eval(&e);
        self.frames.truncate(depth);
        match r {
            Ok(v) => v.into_string(),
            Err(super::Ctrl::Error(m, _)) => m,
            Err(_) => String::new(),
        }
    }

    /// Emuera's GetDebugTraceLog: the line running and the calls that led
    /// to it, newest first.
    fn debug_trace(&self) -> String {
        let path = |file: u32| self.prog.files.get(file as usize).cloned().unwrap_or_default();
        let mut s = String::from("*実行中の行\n");
        match self.frames.last() {
            Some(fr) => {
                let (file, line) = fr.body.lines.get(fr.pc.saturating_sub(1)).copied().unwrap_or((fr.func.file, fr.func.line));
                s += &format!("ファイル名:{}\n行番号:{} 関数名:{}\n", path(file), line, fr.func.name);
            }
            None => s += "ファイル名:なし\n行番号:なし 関数名:なし\n",
        }
        s += "\n*スタックトレース\n";
        for fr in self.frames.iter().rev() {
            s += &format!("CALL :@{}:{}:{}行目\n", fr.func.name, path(fr.func.file), fr.func.line);
        }
        s
    }

    /// DEBUGPRINT and friends: into the debug console, in debug mode only
    /// (Emuera's EmueraConsole.DebugPrint).
    pub fn debug_print(&mut self, s: &str, newline: bool) {
        if !self.con.debug.enabled {
            return;
        }
        self.con.debug.log += s;
        if newline {
            self.con.debug.log.push('\n');
        }
    }

    /// A line typed in the debug console (EmueraConsole.DebugCommand): a
    /// statement is run, anything that doesn't parse as one is an
    /// expression whose value is printed; the line and any error go to the
    /// console. The script's own state (its call stack) is left as it was.
    pub fn debug_command(&mut self, com: &str) {
        let com = com.trim();
        if com.is_empty() {
            return;
        }
        self.con.debug.history.push(com.to_string());
        self.debug_print(com, true);
        if let Err(e) = self.debug_run(com, false) {
            self.debug_print(&e, true);
        }
    }

    /// Emuera's doSystemCommand: a line typed at the game's input that
    /// starts with `@`. The line is echoed; QUIT / EXIT quit, DEBUG opens
    /// the debug panel, REBOOT reloads ERB files (like F5), CONFIG reloads
    /// config (headless) or opens the settings page (TUI), OUTPUTLOG writes
    /// the console to a file; anything else is a debug command when
    /// デバッグコマンドを使用する is on (its output in the game's console,
    /// and MASTER renamed イカサマ unless
    /// デバッグコマンドを使用した時にMASTERの名前を変更する is off).
    pub fn system_command(&mut self, line: &str) -> super::VmResult<()> {
        self.con.print(line);
        self.con.newline();
        let com = line[1..].trim();
        match com.to_uppercase().as_str() {
            "QUIT" | "EXIT" => return Err(super::Ctrl::Quit),
            "DEBUG" if self.con.debug.enabled => {
                self.con.debug.open = true;
                self.debug_refresh();
            }
            "DEBUG" => self.print_error("デバッグウインドウは-Debug引数付きで起動したときのみ使えます"),
            "REBOOT" => self.reload_erb(),
            "CONFIG" => {
                self.reload_config();
                self.con.print("Config reloaded (use F3 in the TUI to open the settings page)");
                self.con.newline();
            }
            "OUTPUT" | "OUTPUTLOG" => {
                let path = self.game_dir.join("emuera.log");
                if self.output_log(&path) {
                    self.con.print(&format!("Log written to {}", path.display()));
                    self.con.newline();
                } else {
                    self.print_error("Failed to write log");
                }
            }
            _ if !self.cfg.bool("デバッグコマンドを使用する") => self.print_error("デバッグコマンドを使用できない設定になっています"),
            _ => match self.debug_run(com, true) {
                Ok(()) => {
                    if self.cfg.bool("デバッグコマンドを使用した時にMASTERの名前を変更する") {
                        self.munchkin();
                    }
                }
                Err(e) => self.print_error(&e),
            },
        }
        Ok(())
    }

    /// Emuera's OutputSystemLog: write the console scrollback to a file.
    fn output_log(&mut self, path: &std::path::Path) -> bool {
        use std::fs::File;
        use std::io::{BufWriter, Write};
        let file = match File::create(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let mut w = BufWriter::new(file);
        // Write UTF-8 BOM like Emuera does
        let _ = w.write_all(&[0xEF, 0xBB, 0xBF]);
        for line in &self.con.lines {
            let text = crate::console::Console::line_text(line);
            let _ = writeln!(w, "{}", text.replace('\n', "\r\n"));
        }
        // Also write the current line being built
        if !self.con.cur.is_empty() {
            let line = crate::console::Line {
                segs: self.con.cur.clone(),
                align: crate::ast::Align::Left,
                gen: 0,
                temporary: false,
                cols: self.con.width,
                kojo: false,
                src: 0,
                wait: 0,
            };
            let text = crate::console::Console::line_text(&line);
            let _ = write!(w, "{}", text.replace('\n', "\r\n"));
        }
        w.flush().is_ok()
    }

    /// The load warnings, in the game's console before it starts, as
    /// Emuera's PrintWarning: the warning in red, then the line it is about.
    pub fn show_load_warnings(&mut self) {
        let shown: Vec<(String, String, u32)> = crate::loadcheck::load_warnings(&self.prog, &self.cfg)
            .into_iter()
            .map(|w| (crate::loadcheck::format(&self.prog, w), self.prog.files.get(w.file as usize).cloned().unwrap_or_default(), w.line))
            .collect();
        for (msg, file, line) in shown {
            self.print_error(&msg);
            if let Some(src) = self.source_line(&file, line) {
                self.con.print(src.trim());
                self.con.newline();
            }
        }
    }

    /// A line in the game's console in red, as Emuera's PrintError.
    fn print_error(&mut self, msg: &str) {
        let fg = self.con.style.fg;
        self.con.style.fg = crate::console::Rgb(255, 0, 0);
        self.con.print(msg);
        self.con.newline();
        self.con.style.fg = fg;
    }

    /// Emuera's IamaMunchkin: a debug command was used, and MASTER's names
    /// say so.
    fn munchkin(&mut self) {
        let master = self.get_global_i("MASTER", 0);
        if master < 0 || master as usize >= self.charas.len() {
            return;
        }
        let m = master as usize;
        for n in ["NAME", "CALLNAME", "NICKNAME"] {
            // (set now: saved, as Emuera saves a name set to anything)
            self.charas[m].names_unset &= !super::name_bit(n);
            self.chara_arr(m, n).set(0, super::Value::Str("イカサマ".into()));
        }
    }

    /// Run a debug command; `main`: as typed at the game's input (an
    /// expression's value printed there), else in the debug console.
    fn debug_run(&mut self, com: &str, main: bool) -> Result<(), String> {
        // (`@…`, a string and `\…` can only be expressions)
        let statement = !com.starts_with(['@', '"', '\\']);
        if statement {
            let word: String = com.chars().take_while(|&c| crate::expr::is_ident_char(c) && c != '　').collect::<String>().to_uppercase();
            if crate::parser::FLOW_WORDS.contains(&word.as_str()) {
                return Err("フロー制御命令は使用できません".into());
            }
            let wait = crate::parser::parse_print_name(&word).is_some_and(|(_, f)| f.wait);
            if wait || NOT_IN_DEBUG.contains(&word.as_str()) || super::builtins::NOT_IN_FUNCTION.binary_search(&word.as_str()).is_ok() {
                return Err(format!("{}命令は使用できません", word));
            }
            let compiled = match &self.prog.snippets {
                Some(p) => p.lock().map_err(|_| "解釈できない行です".to_string())?.compile_snippet("__DEBUGCOMMAND", com),
                None => Err("解釈できない行です".into()),
            };
            if let Ok(f) = compiled {
                let depth = self.frames.len();
                let r = self.call_func(f, Vec::new());
                self.frames.truncate(depth);
                return match r {
                    Ok(_) => Ok(()),
                    Err(super::Ctrl::Error(m, _)) => Err(bare(&m)),
                    Err(_) => Err("フロー制御命令は使用できません".into()),
                };
            }
        }
        // an expression: its value (Emuera prints it with DEBUGPRINTFORML)
        let e = crate::expr::parse_expr(com, &self.prog.defines).map_err(|_| "解釈できない行です".to_string())?;
        let e = crate::parser::upper_expr(e);
        let depth = self.frames.len();
        let r = self.eval(&e);
        self.frames.truncate(depth);
        match r {
            Ok(v) if main => {
                self.con.print(&v.into_string());
                self.con.newline();
                Ok(())
            }
            Ok(v) => {
                self.debug_print(&v.into_string(), true);
                Ok(())
            }
            Err(super::Ctrl::Error(m, _)) => Err(m),
            Err(_) => Err("解釈できない行です".into()),
        }
    }

    /// `debug/watchlist.csv`: one expression a line, UTF-8 with a BOM as
    /// Emuera writes it (Config.Encode).
    fn debug_save_watches(&self) {
        let dir = self.debug_dir();
        let path = dir.join("watchlist.csv");
        let text: String = self.con.debug.watches.iter().filter(|w| !w.is_empty()).map(|w| format!("{}\r\n", w)).collect();
        if text.is_empty() && !path.exists() {
            return;
        }
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(path, [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat());
    }

    /// The watch list and `debug/console.log` (DebugDialog.saveData, when
    /// the window closes).
    pub fn debug_save(&self) {
        self.debug_save_watches();
        if self.con.debug.log.is_empty() {
            return;
        }
        let dir = self.debug_dir();
        let _ = std::fs::create_dir_all(&dir);
        let log = self.con.debug.log.replace('\n', "\r\n");
        let _ = std::fs::write(dir.join("console.log"), [&[0xEF, 0xBB, 0xBF][..], log.as_bytes()].concat());
    }
}

/// An error raised inside the command's function, without the location
/// and call stack the VM adds ("file:line: @__DEBUGCOMMAND: message").
fn bare(m: &str) -> String {
    let first = m.lines().next().unwrap_or(m);
    first.split_once("@__DEBUGCOMMAND: ").map_or(first, |x| x.1).to_string()
}

#[cfg(test)]
mod tests {
    use crate::frontend::DebugAction;
    use std::sync::{Arc, Mutex};

    fn vm(dir: &std::path::Path) -> super::Vm {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let cfg = Arc::new(crate::emuconfig::Values::emuera());
        let mut p = crate::parser::Parser::new(&[], false, cfg.clone());
        let src = crate::parser::SourceFile { path: "T.ERB".into(), text: "@F(A)\n#FUNCTION\nRETURNF A * 3\n".into() };
        p.prog.files.push(src.path.clone());
        p.parse_erb(0, &src);
        p.prog.snippets = Some(Arc::new(Mutex::new(p.snippet_parser())));
        let csv = crate::csvdata::GameCsv::load(&root.join("sample/CSV"), None, &cfg);
        let fe = Box::new(crate::frontend::Headless::new(false));
        super::Vm::new(p.prog, csv, crate::resources::Resources::default(), fe, dir.to_path_buf(), cfg)
    }

    /// The watch list, the console's commands and their errors, the stack
    /// trace, and the files Emuera keeps them in.
    #[test]
    fn debug_window_round_trip() {
        let dir = std::env::temp_dir().join(format!("emuera-tty-debugwin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("debug")).unwrap();
        std::fs::write(dir.join("debug/watchlist.csv"), "\u{feff}FLAG:3 * 2\r\nF(FLAG:3)\r\n").unwrap();
        let mut vm = vm(&dir);
        vm.debug_start();
        assert_eq!(vm.con.debug.watches, ["FLAG:3 * 2", "F(FLAG:3)"]);
        vm.debug_action(DebugAction::Command("FLAG:3 = 21".into()));
        vm.debug_action(DebugAction::Command("FLAG:3 + 1".into()));
        vm.debug_action(DebugAction::Command("WAIT".into()));
        vm.debug_action(DebugAction::Command("IF 1".into()));
        vm.debug_action(DebugAction::Command("FLAG:4 = 1 / FLAG:5".into()));
        assert_eq!(vm.con.debug.values, ["42", "63"]);
        let log = vm.con.debug.log.clone();
        assert!(log.starts_with("FLAG:3 = 21\nFLAG:3 + 1\n22\nWAIT\nWAIT命令は使用できません\nIF 1\nフロー制御命令は使用できません\n"), "{log}");
        // (an error inside the command: its message, not the VM's location)
        assert!(log.lines().nth(8).is_some_and(|l| !l.contains("__DEBUGCOMMAND") && !l.is_empty()), "{log:?}");
        vm.debug_print("from DEBUGPRINT", true);
        vm.con.debug.watches.push("NOPE(".into());
        vm.debug_action(DebugAction::Refresh);
        assert!(!vm.con.debug.values[2].is_empty());
        assert!(vm.con.debug.trace.starts_with("*実行中の行\nファイル名:なし"), "{}", vm.con.debug.trace);
        vm.debug_action(DebugAction::Close);
        let watch = std::fs::read(dir.join("debug/watchlist.csv")).unwrap();
        assert_eq!(watch, "\u{feff}FLAG:3 * 2\r\nF(FLAG:3)\r\nNOPE(\r\n".as_bytes());
        let console = String::from_utf8(std::fs::read(dir.join("debug/console.log")).unwrap()).unwrap();
        assert!(console.starts_with('\u{feff}') && console.contains("22\r\n") && console.ends_with("from DEBUGPRINT\r\n"), "{console}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
