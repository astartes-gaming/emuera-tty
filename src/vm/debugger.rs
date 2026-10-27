//! Debugger: breakpoints, stepping, variable inspection, call stack.
//!
//! Integrated into the VM's instruction loop (`exec`) and function calls
//! (`call_func_in`). Controlled via `EMUERA_VM_DEBUG=1` env var or the
//! `--vm-debug` flag.

use crate::ast::Body;
use crate::fxhash::HashSet;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

// Used by Debugger::parse_command
#[allow(unused_imports)]
use std::io::BufRead;

/// Global debug enabled flag (set from EMUERA_VM_DEBUG or --vm-debug).
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Check if debugger is globally enabled.
pub fn debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Enable/disable debugger globally.
pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// A breakpoint location.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Breakpoint {
    /// File:line (file is the relative path as in prog.files)
    FileLine(String, u32),
    /// Function name (uppercase, e.g., "@EVENT_SHOP")
    Function(String),
    /// Instruction PC within a function (function name + pc)
    Instruction(String, usize),
}

/// Step mode for single-stepping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepMode {
    /// Not stepping
    None,
    /// Step into: stop at next instruction (including calls)
    Into,
    /// Step over: stop at next instruction in same or caller frame
    Over,
    /// Step out: stop when current function returns
    Out,
}

/// Reason for stopping.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum StopReason {
    /// Hit a breakpoint
    Breakpoint(Breakpoint),
    /// Step mode condition met
    Step(StepMode),
    /// Hit an error
    Error(String),
    /// Manual stop (e.g., Ctrl+C in debug mode)
    Manual,
}

/// A parsed debug command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugCommand {
    Continue,
    StepInto(Option<usize>),
    StepOver(Option<usize>),
    StepOut(Option<usize>),
    Backtrace,
    Print(String),           // variable or expression
    Watch(String),           // add watch expression
    Unwatch(usize),          // remove watch by index
    Breakpoint(String),      // add breakpoint (file:line, @func, func:pc)
    ClearBreakpoint(String), // clear breakpoint
    ListBreakpoints,
    ListWatch,
    InfoLocals,
    InfoGlobals,
    InfoFrame,
    Help,
    Quit,
}

/// Debugger state and configuration.
pub struct Debugger {
    /// Enabled breakpoints
    breakpoints: HashSet<Breakpoint>,
    /// Current step mode
    step_mode: StepMode,
    /// Number of steps remaining for multi-step commands
    step_count: usize,
    /// Frame depth when step over/out was initiated (for tracking)
    step_frame_depth: usize,
    /// Watch expressions (evaluated at each stop)
    watch_exprs: Vec<String>,
    /// Whether we're currently stopped at a breakpoint
    stopped: bool,
    /// Reason for the current stop
    #[allow(dead_code)]
    stop_reason: Option<StopReason>,
    /// Pending commands from REPL
    #[allow(dead_code)]
    pending_cmd: Option<DebugCommand>,
    /// Whether to echo debug output to stderr
    pub verbose: bool,
}

impl Debugger {
    /// Get the current step mode.
    pub fn step_mode(&self) -> StepMode {
        self.step_mode
    }

    /// Set the current step mode.
    pub fn set_step_mode(&mut self, mode: StepMode) {
        self.step_mode = mode;
    }

    /// Get the step count for multi-step commands.
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    /// Set the step count for multi-step commands.
    #[allow(dead_code)]
    pub fn set_step_count(&mut self, count: usize) {
        self.step_count = count;
    }

    /// Decrement step count and return true if more steps remain.
    pub fn decrement_step_count(&mut self) -> bool {
        if self.step_count > 0 {
            self.step_count -= 1;
            self.step_count > 0
        } else {
            false
        }
    }

    pub fn new() -> Self {
        let mut dbg = Self {
            breakpoints: HashSet::default(),
            step_mode: StepMode::None,
            step_count: 0,
            step_frame_depth: 0,
            watch_exprs: Vec::new(),
            stopped: false,
            stop_reason: None,
            pending_cmd: None,
            verbose: true,
        };
        dbg.load_initial_breakpoints();
        dbg
    }

    /// Load initial breakpoints from EMUERA_BREAKPOINTS env var.
    /// Format: comma-separated specs like "@EVENTFIRST,file:123,func:42"
    fn load_initial_breakpoints(&mut self) {
        if let Ok(val) = std::env::var("EMUERA_BREAKPOINTS") {
            for spec in val.split(',') {
                let spec = spec.trim();
                if !spec.is_empty() {
                    let bp = Self::parse_breakpoint_spec(spec);
                    self.breakpoints.insert(bp.clone());
                    if self.verbose {
                        eprintln!("Initial breakpoint: {:?}", bp);
                    }
                }
            }
        }
    }

    /// Check if debugger should stop before executing instruction at `pc` in `body`.
    /// Returns the stop reason if we should stop.
    pub fn should_stop_at(
        &mut self,
        frame_depth: usize,
        current_func_name: &str,
        _body: &Body,
        pc: usize,
        file_name: Option<&str>,
        line: Option<u32>,
    ) -> Option<StopReason> {
        if !debug_enabled() {
            return None;
        }

        // Check explicit breakpoints
        if let (Some(file), Some(line)) = (file_name, line) {
            let bp = Breakpoint::FileLine(file.to_string(), line);
            if self.breakpoints.contains(&bp) {
                return Some(StopReason::Breakpoint(bp));
            }
        }

        // Function breakpoint
        let func_bp = Breakpoint::Function(current_func_name.to_string());
        if self.breakpoints.contains(&func_bp) {
            return Some(StopReason::Breakpoint(func_bp));
        }

        // Instruction breakpoint
        let inst_bp = Breakpoint::Instruction(current_func_name.to_string(), pc);
        if self.breakpoints.contains(&inst_bp) {
            return Some(StopReason::Breakpoint(inst_bp));
        }

        // Check step mode
        match self.step_mode {
            StepMode::Into => {
                return Some(StopReason::Step(StepMode::Into));
            }
            StepMode::Over => {
                // Stop if we're at or above the frame depth where step over started
                if frame_depth <= self.step_frame_depth {
                    return Some(StopReason::Step(StepMode::Over));
                }
            }
            StepMode::Out => {
                // Stop when we return from the frame where step out started
                if frame_depth < self.step_frame_depth {
                    return Some(StopReason::Step(StepMode::Out));
                }
            }
            StepMode::None => {}
        }

        None
    }

    /// Called when a function is entered.
    pub fn on_function_enter(&mut self, _frame_depth: usize) {
        if debug_enabled() && self.step_mode != StepMode::None {
            // For step over/out: the check in should_stop_at handles this via frame depth comparison
        }
    }

    /// Called when a function returns.
    pub fn on_function_exit(&mut self, _frame_depth: usize) {
        // Step out: stop when we pop back to the original frame
        // Handled in should_stop_at by checking frame depth
    }

    /// Set the frame depth for step over/out tracking.
    pub fn set_step_frame_depth(&mut self, depth: usize) {
        self.step_frame_depth = depth;
    }

    /// Get watch expressions for iteration.
    pub fn watch_exprs(&self) -> &[String] {
        &self.watch_exprs
    }

    /// Parse a debug command line.
    pub fn parse_command(&self, line: &str) -> DebugCommand {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return DebugCommand::Help;
        }

        // Parse optional count argument for step commands
        let count = if parts.len() > 1 {
            parts[1].parse::<usize>().ok()
        } else {
            None
        };

        match parts[0] {
            "c" | "continue" => DebugCommand::Continue,
            "s" | "step" | "si" | "stepi" => DebugCommand::StepInto(count),
            "n" | "next" => DebugCommand::StepOver(count),
            "f" | "finish" => DebugCommand::StepOut(count),
            "bt" | "backtrace" | "where" => DebugCommand::Backtrace,
            "p" | "print" => {
                if parts.len() > 1 {
                    DebugCommand::Print(parts[1..].join(" "))
                } else {
                    DebugCommand::Help
                }
            }
            "w" | "watch" => {
                if parts.len() > 1 {
                    DebugCommand::Watch(parts[1..].join(" "))
                } else {
                    DebugCommand::Help
                }
            }
            "uw" | "unwatch" => {
                if parts.len() > 1 {
                    DebugCommand::Unwatch(parts[1].parse().unwrap_or(0))
                } else {
                    DebugCommand::Help
                }
            }
            "b" | "break" | "breakpoint" => {
                if parts.len() > 1 {
                    DebugCommand::Breakpoint(parts[1..].join(" "))
                } else {
                    DebugCommand::ListBreakpoints
                }
            }
            "cb" | "clear" => {
                if parts.len() > 1 {
                    DebugCommand::ClearBreakpoint(parts[1..].join(" "))
                } else {
                    DebugCommand::Help
                }
            }
            "lb" | "listb" => DebugCommand::ListBreakpoints,
            "lw" | "listw" => DebugCommand::ListWatch,
            "il" | "locals" => DebugCommand::InfoLocals,
            "ig" | "globals" => DebugCommand::InfoGlobals,
            "if" | "frame" => DebugCommand::InfoFrame,
            "h" | "help" | "?" => DebugCommand::Help,
            "q" | "quit" => DebugCommand::Quit,
            _ => DebugCommand::Help,
        }
    }

    /// Print help text.
    pub fn print_help(&self) {
        eprintln!(
            r#"Debugger commands:
  c, continue       Continue execution
  s, step, si [n]   Step into (next n instructions, default 1)
  n, next [n]       Step over (next n instructions in this or caller frame)
  f, finish [n]     Step out (until n functions return, default 1)
  bt, backtrace     Show call stack
  p, print <var>    Print variable value
  w, watch <expr>   Add watch expression
  uw, unwatch <n>   Remove watch expression
  b, break <spec>   Set breakpoint (file:line, @func, func:pc)
  cb, clear <spec>  Clear breakpoint
  lb, listb         List breakpoints
  lw, listw         List watch expressions
  il, locals        Show local variables
  ig, globals       Show global variables
  if, frame         Show current frame info
  h, help, ?        Show this help
  q, quit           Quit the game
"#
        );
    }

    /// Execute a debug command that doesn't require VM access.
    /// Returns true if the command was handled and execution should continue.
    pub fn execute_simple_command(&mut self, cmd: DebugCommand) -> Option<ReplAction> {
        match cmd {
            DebugCommand::Continue => {
                self.step_mode = StepMode::None;
                self.step_count = 0;
                Some(ReplAction::Continue)
            }
            DebugCommand::StepInto(count) => {
                self.step_mode = StepMode::Into;
                self.step_count = count.unwrap_or(1);
                Some(ReplAction::Continue)
            }
            DebugCommand::StepOver(count) => {
                self.step_mode = StepMode::Over;
                self.step_count = count.unwrap_or(1);
                Some(ReplAction::Continue)
            }
            DebugCommand::StepOut(count) => {
                self.step_mode = StepMode::Out;
                self.step_count = count.unwrap_or(1);
                Some(ReplAction::Continue)
            }
            DebugCommand::Watch(expr) => {
                self.watch_exprs.push(expr.clone());
                eprintln!("Watchpoint {}: {}", self.watch_exprs.len() - 1, expr);
                Some(ReplAction::Stay)
            }
            DebugCommand::Unwatch(idx) => {
                if idx < self.watch_exprs.len() {
                    let removed = self.watch_exprs.remove(idx);
                    eprintln!("Removed watch: {}", removed);
                } else {
                    eprintln!("Invalid watch index: {}", idx);
                }
                Some(ReplAction::Stay)
            }
            DebugCommand::Breakpoint(spec) => {
                self.add_breakpoint(&spec);
                Some(ReplAction::Stay)
            }
            DebugCommand::ClearBreakpoint(spec) => {
                self.remove_breakpoint(&spec);
                Some(ReplAction::Stay)
            }
            DebugCommand::ListBreakpoints => {
                self.list_breakpoints();
                Some(ReplAction::Stay)
            }
            DebugCommand::ListWatch => {
                self.list_watches();
                Some(ReplAction::Stay)
            }
            DebugCommand::Help => {
                self.print_help();
                Some(ReplAction::Stay)
            }
            DebugCommand::Quit => Some(ReplAction::Quit),
            // Commands that need VM access
            DebugCommand::Backtrace | DebugCommand::Print(_) | DebugCommand::InfoLocals | DebugCommand::InfoGlobals | DebugCommand::InfoFrame => None,
        }
    }

    /// Add a breakpoint from a spec string.
    fn add_breakpoint(&mut self, spec: &str) {
        let bp = Self::parse_breakpoint_spec(spec);
        self.breakpoints.insert(bp.clone());
        eprintln!("Breakpoint set: {:?}", bp);
    }

    /// Remove a breakpoint from a spec string.
    fn remove_breakpoint(&mut self, spec: &str) {
        let bp = Self::parse_breakpoint_spec(spec);
        if self.breakpoints.remove(&bp) {
            eprintln!("Breakpoint cleared: {:?}", bp);
        } else {
            eprintln!("Breakpoint not found: {:?}", bp);
        }
    }

    /// Parse a breakpoint specification.
    /// Formats: "file:line", "@function", "function:pc"
    fn parse_breakpoint_spec(spec: &str) -> Breakpoint {
        // Check for file:line (contains path separator or dot before colon)
        if spec.contains(':') && !spec.starts_with('@') {
            let parts: Vec<&str> = spec.splitn(2, ':').collect();
            let before_colon = parts[0];
            // Heuristic: file paths contain / or \ or . (for extension)
            let looks_like_file = before_colon.contains('/') || before_colon.contains('\\') || before_colon.contains('.');
            if looks_like_file {
                if let Ok(line) = parts[1].parse::<u32>() {
                    return Breakpoint::FileLine(parts[0].to_string(), line);
                }
            }
        }
        // Check for function:pc (e.g., "EVENT_SHOP:42")
        if spec.contains(':') && !spec.starts_with('@') {
            let parts: Vec<&str> = spec.splitn(2, ':').collect();
            if let Ok(pc) = parts[1].parse::<usize>() {
                return Breakpoint::Instruction(parts[0].to_uppercase(), pc);
            }
        }
        // Function name (with or without @)
        let name = spec.trim_start_matches('@').to_uppercase();
        Breakpoint::Function(name)
    }

    fn list_breakpoints(&self) {
        if self.breakpoints.is_empty() {
            eprintln!("No breakpoints set.");
            return;
        }
        eprintln!("Breakpoints:");
        for bp in &self.breakpoints {
            eprintln!("  {:?}", bp);
        }
    }

    fn list_watches(&self) {
        if self.watch_exprs.is_empty() {
            eprintln!("No watch expressions.");
            return;
        }
        eprintln!("Watch expressions:");
        for (i, w) in self.watch_exprs.iter().enumerate() {
            eprintln!("  [{}] {}", i, w);
        }
    }
}

/// Action after executing a REPL command.
pub enum ReplAction {
    Continue,
    Stay,
    Quit,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Debugger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Debugger")
            .field("breakpoints", &self.breakpoints)
            .field("step_mode", &self.step_mode)
            .field("watch_exprs", &self.watch_exprs)
            .field("stopped", &self.stopped)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_breakpoint_file_line() {
        let bp = Debugger::parse_breakpoint_spec("ERB/TEST.ERB:42");
        assert_eq!(bp, Breakpoint::FileLine("ERB/TEST.ERB".to_string(), 42));
    }

    #[test]
    fn parse_breakpoint_function_with_at() {
        let bp = Debugger::parse_breakpoint_spec("@EVENTFIRST");
        assert_eq!(bp, Breakpoint::Function("EVENTFIRST".to_string()));
    }

    #[test]
    fn parse_breakpoint_function_without_at() {
        let bp = Debugger::parse_breakpoint_spec("EVENTFIRST");
        assert_eq!(bp, Breakpoint::Function("EVENTFIRST".to_string()));
    }

    #[test]
    fn parse_breakpoint_instruction() {
        let bp = Debugger::parse_breakpoint_spec("EVENTFIRST:10");
        assert_eq!(bp, Breakpoint::Instruction("EVENTFIRST".to_string(), 10));
    }

    #[test]
    fn parse_command_continue() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("c"), DebugCommand::Continue);
        assert_eq!(dbg.parse_command("continue"), DebugCommand::Continue);
    }

    #[test]
    fn parse_command_step_into() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("s"), DebugCommand::StepInto(None));
        assert_eq!(dbg.parse_command("step"), DebugCommand::StepInto(None));
        assert_eq!(dbg.parse_command("si"), DebugCommand::StepInto(None));
    }

    #[test]
    fn parse_command_step_over() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("n"), DebugCommand::StepOver(None));
        assert_eq!(dbg.parse_command("next"), DebugCommand::StepOver(None));
    }

    #[test]
    fn parse_command_step_out() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("f"), DebugCommand::StepOut(None));
        assert_eq!(dbg.parse_command("finish"), DebugCommand::StepOut(None));
    }

    #[test]
    fn parse_command_backtrace() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("bt"), DebugCommand::Backtrace);
        assert_eq!(dbg.parse_command("backtrace"), DebugCommand::Backtrace);
        assert_eq!(dbg.parse_command("where"), DebugCommand::Backtrace);
    }

    #[test]
    fn parse_command_print() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("p MONEY"), DebugCommand::Print("MONEY".to_string()));
        assert_eq!(dbg.parse_command("print DAY"), DebugCommand::Print("DAY".to_string()));
    }

    #[test]
    fn parse_command_watch() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("w MONEY"), DebugCommand::Watch("MONEY".to_string()));
        assert_eq!(dbg.parse_command("watch DAY"), DebugCommand::Watch("DAY".to_string()));
    }

    #[test]
    fn parse_command_breakpoint() {
        let dbg = Debugger::new();
        assert_eq!(dbg.parse_command("b @EVENTFIRST"), DebugCommand::Breakpoint("@EVENTFIRST".to_string()));
        assert_eq!(dbg.parse_command("break EVENTFIRST"), DebugCommand::Breakpoint("EVENTFIRST".to_string()));
        assert_eq!(dbg.parse_command("breakpoint ERB/TEST.ERB:10"), DebugCommand::Breakpoint("ERB/TEST.ERB:10".to_string()));
    }

    #[test]
    fn step_mode_tracking() {
        let mut dbg = Debugger::new();
        assert_eq!(dbg.step_mode, StepMode::None);

        dbg.step_mode = StepMode::Into;
        assert_eq!(dbg.step_mode, StepMode::Into);

        dbg.step_mode = StepMode::Over;
        dbg.set_step_frame_depth(3);
        assert_eq!(dbg.step_mode, StepMode::Over);
        assert_eq!(dbg.step_frame_depth, 3);

        dbg.step_mode = StepMode::Out;
        dbg.set_step_frame_depth(2);
        assert_eq!(dbg.step_mode, StepMode::Out);
        assert_eq!(dbg.step_frame_depth, 2);
    }

    #[test]
    fn watch_expressions() {
        let mut dbg = Debugger::new();
        assert!(dbg.watch_exprs().is_empty());

        // Simulate adding a watch
        dbg.watch_exprs.push("MONEY".to_string());
        assert_eq!(dbg.watch_exprs(), &["MONEY"]);

        dbg.watch_exprs.push("DAY".to_string());
        assert_eq!(dbg.watch_exprs(), &["MONEY", "DAY"]);
    }

    #[test]
    fn breakpoint_management() {
        let mut dbg = Debugger::new();
        assert!(dbg.breakpoints.is_empty());

        dbg.add_breakpoint("@EVENTFIRST");
        assert_eq!(dbg.breakpoints.len(), 1);
        assert!(dbg.breakpoints.contains(&Breakpoint::Function("EVENTFIRST".to_string())));

        dbg.add_breakpoint("ERB/TEST.ERB:10");
        assert_eq!(dbg.breakpoints.len(), 2);

        dbg.remove_breakpoint("@EVENTFIRST");
        assert_eq!(dbg.breakpoints.len(), 1);
        assert!(!dbg.breakpoints.contains(&Breakpoint::Function("EVENTFIRST".to_string())));
    }
}
