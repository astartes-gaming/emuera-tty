//! ERB/ERH statement parser: turns source files into a [`Program`].
//!
//! Pipeline per file:
//!
//! 1. `logical_lines` — strip comments-only lines, apply `[SKIPSTART]`,
//!    `[IF_DEBUG]`/`[IF X]` conditional blocks, `[[rename]]` substitution
//!    and `{ … }` line joining, yielding (line number, text) pairs.
//! 2. `.ERH` headers (`parse_erh`): `#DEFINE`s first, then global `#DIM`s.
//! 3. `.ERB` files (`parse_erb`): an `@NAME(args)` line starts a function;
//!    every following line goes through `statement`, which emits
//!    [`Instr`]s into a [`FuncBuilder`].
//!
//! Block statements are compiled with a stack of open blocks (`Ctl`): IF
//! emits a `JumpIfNot` with a placeholder target, ELSE/ENDIF *patch* it
//! once the target is known, BREAK/CONTINUE are recorded on the innermost
//! loop and patched at NEXT/WEND/LOOP. SIF is special: its jump is patched
//! after the next statement, whatever it is (`after_stmt`).
//!
//! A line that fails to parse becomes an [`Instr::BadLine`] rather than
//! disappearing, so jumps and SIF still line up and the error surfaces if
//! the line is actually executed.

use crate::ast::*;
use crate::expr::{parse_expr, parse_expr_list, parse_form, split_top, ExprParser};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// One loaded .ERB/.ERH file (path relative to the game, decoded text).
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

/// Everything compiled from a game: what the VM runs.
#[derive(Default)]
pub struct Program {
    pub files: Vec<String>,
    /// Functions by uppercase name; several entries == event function.
    pub funcs: HashMap<String, Vec<Rc<Function>>>,
    /// `#DIM`s from .ERH files, allocated at game start / RESETDATA.
    pub global_dims: Vec<DimDecl>,
    /// `#DEFINE NAME body`: expanded textually inside expressions.
    pub defines: HashMap<String, String>,
    /// Parse errors, "file:line: message"; shown by `--check`.
    pub errors: Vec<String>,
}

/// System string variables (all others are integer). Whether a variable is a
/// string decides how `X = …` parses: string `=` takes FORM text.
pub fn is_system_str_var(n: &str) -> bool {
    matches!(
        n,
        "STR" | "SAVESTR" | "RESULTS" | "TSTR" | "LOCALS" | "ARGS" | "GLOBALS" | "NAME" | "CALLNAME" | "NICKNAME"
            | "MASTERNAME" | "CSTR" | "DRAWLINESTR" | "WINDOW_TITLE" | "MONEYLABEL" | "GAMEBASE_AUTHOR"
            | "GAMEBASE_INFO" | "GAMEBASE_YEAR" | "GAMEBASE_TITLE" | "LASTLOAD_TEXT" | "SAVEDATA_TEXT"
            | "ABLNAME" | "TALENTNAME" | "EXPNAME" | "MARKNAME" | "PALAMNAME" | "TRAINNAME" | "BASENAME"
            | "SOURCENAME" | "EXNAME" | "ITEMNAME" | "EQUIPNAME" | "TEQUIPNAME" | "FLAGNAME" | "TFLAGNAME"
            | "CFLAGNAME" | "TCVARNAME" | "CSTRNAME" | "STAINNAME" | "STRNAME" | "TSTRNAME" | "SAVESTRNAME"
            | "GLOBALNAME" | "GLOBALSNAME" | "CDFLAGNAME1" | "CDFLAGNAME2" | "TSTRNAME_" | "STR_"
    )
}

/// An open block while compiling, with the instruction indices still
/// waiting for a jump target.
enum Ctl {
    /// `test`: the pending JumpIfNot of the current IF/ELSEIF branch;
    /// `ends`: jumps from the end of each branch to ENDIF.
    If { test: Option<usize>, ends: Vec<usize> },
    Select { slot: usize, test: Option<usize>, ends: Vec<usize>, first: bool },
    For { begin: usize, slot: usize, var: Rc<VarRef>, breaks: Vec<usize>, conts: Vec<usize> },
    While { top: usize, test: usize, breaks: Vec<usize>, conts: Vec<usize> },
    Do { top: usize, breaks: Vec<usize>, conts: Vec<usize> },
    /// TRYC*… CATCH … ENDCATCH: `call` gets its catch target patched.
    TryC { call: usize, ends: Vec<usize> },
    /// PRINTDATA (`flags`) / STRDATA (`var`): collects DATA lines until
    /// ENDDATA; DATALIST…ENDLIST groups several lines into one entry.
    Data { var: Option<Rc<VarRef>>, flags: Option<PrintFlags>, entries: Vec<Vec<Form>>, list: Option<Vec<Form>> },
    CallList { kind: CallKind, goto: bool, entries: Vec<(CallTarget, Vec<Expr>)> },
}

/// The function being compiled.
struct FuncBuilder {
    f: Function,
    /// Open blocks, innermost last.
    ctl: Vec<Ctl>,
    /// SIF jumps waiting for "the end of the next statement".
    sif: Vec<usize>,
    /// Private `#DIMS` names (string-typed for assignment parsing).
    str_locals: HashSet<String>,
    /// Source position recorded for each emitted instruction.
    cur_line: (u32, u32),
}

impl FuncBuilder {
    /// Append an instruction; returns its index (for later patching).
    fn emit(&mut self, i: Instr) -> usize {
        self.f.code.push(i);
        self.f.lines.push(self.cur_line);
        self.f.code.len() - 1
    }
    fn pc(&self) -> usize {
        self.f.code.len()
    }
    /// Fill in the jump / exit / catch target of instruction `at`.
    fn patch(&mut self, at: usize, target: usize) {
        match &mut self.f.code[at] {
            Instr::Jump(t) => *t = target,
            Instr::JumpIfNot(_, t) => *t = target,
            Instr::ForBegin { exit, .. } => *exit = target,
            Instr::CaseTest(_, _, t) => *t = target,
            Instr::Call { catch, .. } => *catch = Some(target),
            Instr::Goto { catch, .. } => *catch = Some(target),
            _ => {}
        }
    }
    /// Reserve a per-call scratch slot (FOR bounds, SELECTCASE value).
    fn slot(&mut self) -> usize {
        self.f.n_slots += 1;
        self.f.n_slots - 1
    }
}

pub struct Parser<'a> {
    pub prog: Program,
    /// `_Rename.csv` pairs for `[[name]]` substitution.
    rename: &'a [(String, String)],
    /// Global `#DIMS` names.
    global_str: HashSet<String>,
    /// `--debug`: keep `;#;` lines and `[IF_DEBUG]` blocks.
    debug: bool,
}

/// Cut a trailing `; comment`, ignoring `;` inside quotes (quote nesting
/// pairs up, so a plain toggle is enough).
fn strip_comment(s: &str) -> &str {
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        if c == '"' && prev != '\\' {
            in_str = !in_str;
        }
        if c == ';' && !in_str {
            return &s[..i];
        }
        prev = c;
    }
    s
}

/// Split "CMD rest" -> (CMD uppercase, rest after exactly one separator char).
fn split_cmd(line: &str) -> (String, &str) {
    let end = line
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || *c == '\u{3000}' || (!crate::expr::is_ident_char(*c)))
        .map(|(i, _)| i)
        .unwrap_or(line.len());
    let cmd = line[..end].to_uppercase();
    let mut rest = &line[end..];
    if let Some(c) = rest.chars().next() {
        if c == ' ' || c == '\t' || c == '\u{3000}' {
            rest = &rest[c.len_utf8()..];
        }
    }
    (cmd, rest)
}

/// Decode a PRINT-family name: `PRINT[SINGLE|PLAIN][V|S|FORM|FORMS][K|D][L|W|C|LC|N]`.
/// Returns the content kind (R raw, V, S, F form, G forms) and the flags,
/// or None if `cmd` isn't a PRINT variant.
pub fn parse_print_name(cmd: &str) -> Option<(char, PrintFlags)> {
    let mut s = cmd.strip_prefix("PRINT")?;
    let mut fl = PrintFlags::default();
    if let Some(r) = s.strip_prefix("SINGLE") {
        fl.single = true;
        s = r;
    } else if let Some(r) = s.strip_prefix("PLAIN") {
        fl.plain = true;
        s = r;
    }
    // mid: R=raw, V, S, F=FORM, G=FORMS
    let mut mid = 'R';
    for (p, m) in [("FORMS", 'G'), ("FORM", 'F'), ("V", 'V'), ("S", 'S')] {
        if let Some(r) = s.strip_prefix(p) {
            // Avoid PRINTS... eating SINGLE etc. (already handled)
            mid = m;
            s = r;
            break;
        }
    }
    if let Some(r) = s.strip_prefix('K') {
        s = r;
    } else if let Some(r) = s.strip_prefix('D') {
        fl.default_color = true;
        s = r;
    }
    match s {
        "" => {}
        "L" | "N" => fl.newline = true,
        "W" => {
            fl.newline = true;
            fl.wait = true;
        }
        "C" => fl.column = Some(Align::Right),
        "LC" => fl.column = Some(Align::Left),
        _ => return None,
    }
    Some((mid, fl))
}

/// PRINTDATA[K|D][L|W] → flags, or None for other commands.
fn print_data_flags(cmd: &str) -> Option<PrintFlags> {
    let s = cmd.strip_prefix("PRINTDATA")?;
    let mut fl = PrintFlags::default();
    let s = s.strip_prefix('K').or_else(|| s.strip_prefix('D')).unwrap_or(s);
    match s {
        "" => {}
        "L" => fl.newline = true,
        "W" => {
            fl.newline = true;
            fl.wait = true
        }
        _ => return None,
    }
    Some(fl)
}

/// Commands whose argument is a raw string (no expression parsing).
fn raw_arg_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "DATA" | "CUSTOMDRAWLINE" | "ALIGNMENT" | "STRLEN" | "STRLENU" | "TIMES" | "DEBUGPRINT" | "DEBUGPRINTL"
            | "SETCOLORBYNAME" | "SETBGCOLORBYNAME" | "BEGIN" | "SORTCHARA" | "PRINTBUTTONS" | "DOTRAIN_" | "BITMAP_CACHE_ENABLE"
    )
}

/// Commands whose argument is a FORM string.
fn form_arg_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "DATAFORM" | "THROW" | "DRAWLINEFORM" | "STRLENFORM" | "STRLENFORMU" | "PUTFORM" | "REUSELASTLINE"
            | "DEBUGPRINTFORM" | "DEBUGPRINTFORML" | "ENCODETOUNI_" | "SAVENOS_"
    )
}

/// Block markers that only matter to Emuera's message skipping.
const IGNORED_BLOCKS: &[&str] = &["NOSKIP", "ENDNOSKIP"];

impl<'a> Parser<'a> {
    pub fn new(rename: &'a [(String, String)], debug: bool) -> Self {
        Parser { prog: Program::default(), rename, global_str: HashSet::new(), debug }
    }

    /// Preprocess raw text into logical (line_no, text) pairs.
    fn logical_lines(&self, text: &str) -> Vec<(u32, String)> {
        let mut out = Vec::new();
        let mut skip_depth = 0;
        let mut cond_stack: Vec<bool> = Vec::new(); // true = active
        let mut join: Option<(u32, String)> = None;
        for (no, raw) in text.lines().enumerate() {
            let no = no as u32 + 1;
            let mut line = raw.trim().to_string();
            if line.starts_with(";!;") {
                line = line[3..].trim().to_string();
            } else if line.starts_with(";#;") {
                if !self.debug {
                    continue;
                }
                line = line[3..].trim().to_string();
            }
            // [[rename]]
            if line.contains("[[") {
                for (k, v) in self.rename {
                    line = line.replace(&format!("[[{}]]", k), v);
                }
            }
            let up = line.to_uppercase();
            if up.starts_with("[SKIPSTART]") {
                skip_depth += 1;
                continue;
            }
            if up.starts_with("[SKIPEND]") {
                if skip_depth > 0 {
                    skip_depth -= 1;
                }
                continue;
            }
            if skip_depth > 0 {
                continue;
            }
            if up.starts_with("[IF_DEBUG]") {
                cond_stack.push(self.debug);
                continue;
            }
            if up.starts_with("[IF_NDEBUG]") {
                cond_stack.push(!self.debug);
                continue;
            }
            if up.starts_with("[IF ") || up.starts_with("[ELSEIF ") {
                let elseif = up.starts_with("[ELSEIF");
                let name = line.trim_start_matches(|c| c != ' ').trim().trim_end_matches(']').trim();
                let v = self.prog.defines.contains_key(&name.to_uppercase());
                if elseif {
                    if let Some(t) = cond_stack.last_mut() {
                        *t = !*t && v;
                    }
                } else {
                    cond_stack.push(v);
                }
                continue;
            }
            if up.starts_with("[ELSE]") {
                if let Some(t) = cond_stack.last_mut() {
                    *t = !*t;
                }
                continue;
            }
            if up.starts_with("[ENDIF]") {
                cond_stack.pop();
                continue;
            }
            if cond_stack.iter().any(|v| !v) {
                continue;
            }
            if line == "{" {
                join = Some((no, String::new()));
                continue;
            }
            if line == "}" {
                if let Some((n, s)) = join.take() {
                    out.push((n, s));
                }
                continue;
            }
            if let Some((_, s)) = join.as_mut() {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(&line);
                continue;
            }
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            out.push((no, line));
        }
        out
    }

    /// Header file: `#DEFINE`s and global `#DIM`s only. All headers are
    /// parsed before any .ERB, so functions can use what they declare.
    pub fn parse_erh(&mut self, file_idx: u32, src: &SourceFile) {
        let lines = self.logical_lines(&src.text);
        // defines first (so dims can use them)
        for (no, line) in &lines {
            let up = line.to_uppercase();
            if up.starts_with("#DEFINE") {
                let rest = strip_comment(line[7..].trim()).trim();
                let (name, body) = match rest.find(|c: char| c.is_whitespace()) {
                    Some(i) => (&rest[..i], rest[i..].trim()),
                    None => (rest, ""),
                };
                self.prog.defines.insert(name.to_uppercase(), body.to_string());
            } else if !(up.starts_with("#DIM")) {
                self.prog.errors.push(format!("{}:{}: unexpected line in ERH: {}", src.path, no, line));
            }
        }
        for (no, line) in &lines {
            let up = line.to_uppercase();
            if up.starts_with("#DIM") {
                match self.parse_dim(strip_comment(line)) {
                    Ok(d) => {
                        if d.is_str {
                            self.global_str.insert(d.name.clone());
                        }
                        self.prog.global_dims.push(d);
                    }
                    Err(e) => self.prog.errors.push(format!("{}:{}: {}", src.path, no, e)),
                }
            }
        }
        let _ = file_idx;
    }

    /// `#DIM[S] [CONST|STATIC|GLOBAL|SAVEDATA|CHARADATA|REF|DYNAMIC…] NAME[, size…][ = init…]`.
    fn parse_dim(&self, line: &str) -> Result<DimDecl, String> {
        let up = line.to_uppercase();
        let is_str = up.starts_with("#DIMS");
        let rest = if is_str { &line[5..] } else { &line[4..] };
        let mut p = ExprParser::new(rest, &self.prog.defines);
        let mut d = DimDecl {
            name: String::new(),
            is_str,
            dims: vec![],
            init: vec![],
            is_const: false,
            is_static: false,
            is_global: false,
            is_savedata: false,
            is_chara: false,
            is_ref: false,
            is_dynamic: false,
        };
        loop {
            let save = p.pos;
            let Some(id) = p.ident() else { return Err("missing #DIM name".into()) };
            match id.to_uppercase().as_str() {
                "CONST" => d.is_const = true,
                "STATIC" => d.is_static = true,
                "GLOBAL" => d.is_global = true,
                "SAVEDATA" => d.is_savedata = true,
                "CHARADATA" => d.is_chara = true,
                "REF" => d.is_ref = true,
                "DYNAMIC" => d.is_dynamic = true,
                _ => {
                    let _ = save;
                    d.name = id.to_uppercase();
                    break;
                }
            }
        }
        // Emuera: a function's private #DIM keeps its value across calls
        // unless DYNAMIC (or REF); only consulted for function-private dims
        if !d.is_dynamic && !d.is_ref {
            d.is_static = true;
        }
        if p.eat(",") {
            loop {
                p.skip_ws();
                // `#DIM REF X,,` leaves the sizes to the referenced array
                if d.is_ref && matches!(p.peek(), None | Some(',')) {
                    d.dims.push(Expr::Int(0));
                } else {
                    d.dims.push(p.expr()?);
                }
                if !p.eat(",") {
                    break;
                }
            }
        }
        if p.eat("=") {
            d.init = p.expr_list()?;
        }
        if !p.at_end() {
            return Err(format!("bad #DIM: {}", line));
        }
        // identifiers are case-insensitive: `#DIMS X, PNMax` names PNMAX
        d.dims = d.dims.into_iter().map(upper_expr).collect();
        d.init = d.init.into_iter().map(upper_expr).collect();
        Ok(d)
    }

    /// Script file: a sequence of `@FUNCTION` blocks.
    pub fn parse_erb(&mut self, file_idx: u32, src: &SourceFile) {
        let lines = self.logical_lines(&src.text);
        let mut fb: Option<FuncBuilder> = None;
        for (no, line) in lines {
            if let Some(stripped) = line.strip_prefix('@') {
                if let Some(b) = fb.take() {
                    self.finish(b, &src.path);
                }
                match self.func_header(stripped, file_idx, no) {
                    Ok(b) => fb = Some(b),
                    Err(e) => self.prog.errors.push(format!("{}:{}: {}", src.path, no, e)),
                }
                continue;
            }
            let Some(b) = fb.as_mut() else {
                self.prog.errors.push(format!("{}:{}: line outside of function", src.path, no));
                continue;
            };
            b.cur_line = (file_idx, no);
            if let Err(e) = self.statement(b, &line) {
                self.prog.errors.push(format!("{}:{}: {} :: {}", src.path, no, e, line));
                // keep the line's place so a pending SIF guards it, as in Emuera
                let fatal = !e.starts_with("unknown command");
                b.emit(Instr::BadLine(format!("{} :: {}", e, line).into(), fatal));
                Self::after_stmt(b);
            }
        }
        if let Some(b) = fb.take() {
            self.finish(b, &src.path);
        }
    }

    /// `@NAME(ARG, ARGS:1 = "x")` or `@NAME, ARG, …`: start a function.
    fn func_header(&self, s: &str, file_idx: u32, line: u32) -> Result<FuncBuilder, String> {
        let s = strip_comment(s).trim();
        let end = s.find(|c: char| c == '(' || c == ',' || c.is_whitespace()).unwrap_or(s.len());
        let name = s[..end].to_uppercase();
        let mut rest = s[end..].trim();
        if rest.starts_with('(') {
            rest = rest[1..].trim_end();
            rest = rest.strip_suffix(')').unwrap_or(rest);
        } else if let Some(r) = rest.strip_prefix(',') {
            rest = r;
        }
        let mut params = Vec::new();
        if !rest.trim().is_empty() {
            for part in split_top(rest, ',') {
                let mut p = ExprParser::new(&part, &self.prog.defines);
                let mut v = p.var_ref()?;
                upper_var(&mut v);
                let default = if p.eat("=") { Some(p.expr()?) } else { None };
                params.push(Param { target: v, default });
            }
        }
        Ok(FuncBuilder {
            f: Function {
                name,
                params,
                code: vec![],
                lines: vec![],
                labels: HashMap::new(),
                dims: vec![],
                is_function: false,
                returns_str: false,
                order: EventOrder::Normal,
                single: false,
                only: false,
                n_slots: 0,
                file: file_idx,
                line,
            },
            ctl: vec![],
            sif: vec![],
            str_locals: HashSet::new(),
            cur_line: (file_idx, line),
        })
    }

    /// Close a function: report unclosed blocks and register it. Several
    /// functions with one name form an event (all are kept).
    fn finish(&mut self, mut b: FuncBuilder, path: &str) {
        if !b.ctl.is_empty() {
            let kinds: Vec<&str> = b
                .ctl
                .iter()
                .map(|c| match c {
                    Ctl::If { .. } => "IF",
                    Ctl::Select { .. } => "SELECTCASE",
                    Ctl::For { .. } => "FOR/REPEAT",
                    Ctl::While { .. } => "WHILE",
                    Ctl::Do { .. } => "DO",
                    Ctl::TryC { .. } => "TRYC*",
                    Ctl::Data { .. } => "PRINTDATA/STRDATA",
                    Ctl::CallList { .. } => "TRY*LIST",
                })
                .collect();
            self.prog.errors.push(format!("{}: @{}: unclosed block(s): {}", path, b.f.name, kinds.join(", ")));
        }
        // falling off the end returns without touching RESULT
        b.emit(Instr::ReturnF(None));
        let name = b.f.name.clone();
        self.prog.funcs.entry(name).or_default().push(Rc::new(b.f));
    }

    fn is_str_var(&self, b: &FuncBuilder, name: &str) -> bool {
        is_system_str_var(name) || self.global_str.contains(name) || b.str_locals.contains(name)
    }

    /// Called after each complete statement: any pending SIF jumps land
    /// here, i.e. just past the statement they guarded.
    fn after_stmt(b: &mut FuncBuilder) {
        let pc = b.pc();
        for s in std::mem::take(&mut b.sif) {
            b.patch(s, pc);
        }
    }

    /// Compile one logical line. Order matters: inside PRINTDATA/TRY*LIST
    /// only their sub-commands are allowed; then labels, `#` directives,
    /// PRINT variants, control flow, other commands, and finally
    /// assignment (anything that isn't a known command).
    fn statement(&mut self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        // Inside PRINTDATA / STRDATA collect DATA lines.
        if let Some(Ctl::Data { .. }) = b.ctl.last() {
            let (cmd, rest) = split_cmd(line);
            match cmd.as_str() {
                "DATA" | "DATAFORM" => {
                    let form = if cmd == "DATA" { Form::lit(rest) } else { parse_form(rest, &self.prog.defines)? };
                    if let Some(Ctl::Data { entries, list, .. }) = b.ctl.last_mut() {
                        match list {
                            Some(l) => l.push(form),
                            None => entries.push(vec![form]),
                        }
                    }
                    return Ok(());
                }
                "DATALIST" => {
                    if let Some(Ctl::Data { list, .. }) = b.ctl.last_mut() {
                        *list = Some(vec![]);
                    }
                    return Ok(());
                }
                "ENDLIST" => {
                    if let Some(Ctl::Data { list, entries, .. }) = b.ctl.last_mut() {
                        if let Some(l) = list.take() {
                            entries.push(l);
                        }
                    }
                    return Ok(());
                }
                "ENDDATA" => {
                    if let Some(Ctl::Data { var, flags, entries, .. }) = b.ctl.pop() {
                        match flags {
                            Some(fl) => b.emit(Instr::PrintData(entries, fl)),
                            None => b.emit(Instr::StrData(var, entries)),
                        };
                        Self::after_stmt(b);
                    }
                    return Ok(());
                }
                _ => return Err("only DATA/DATAFORM/DATALIST allowed inside PRINTDATA".into()),
            }
        }
        if let Some(Ctl::CallList { .. }) = b.ctl.last() {
            let (cmd, rest) = split_cmd(line);
            if cmd == "FUNC" {
                let (t, args) = self.call_target(strip_comment(rest), false)?;
                if let Some(Ctl::CallList { entries, .. }) = b.ctl.last_mut() {
                    entries.push((t, args));
                }
                return Ok(());
            } else if cmd == "ENDFUNC" {
                if let Some(Ctl::CallList { kind, goto, entries }) = b.ctl.pop() {
                    if goto {
                        b.emit(Instr::GotoList(entries.into_iter().map(|(t, _)| t).collect()));
                    } else {
                        b.emit(Instr::CallList(entries, kind));
                    }
                    Self::after_stmt(b);
                }
                return Ok(());
            } else {
                return Err("only FUNC allowed inside TRYCALLLIST".into());
            }
        }

        if let Some(label) = line.strip_prefix('$') {
            let label = strip_comment(label).trim().to_uppercase();
            let pc = b.pc();
            b.f.labels.insert(label, pc);
            return Ok(());
        }
        if line.starts_with('#') {
            return self.directive(b, line);
        }
        // ++X / --X statement
        if line.starts_with("++") || line.starts_with("--") {
            let e = parse_expr(strip_comment(line).trim(), &self.prog.defines)?;
            b.emit(Instr::Eval(upper_expr(e)));
            Self::after_stmt(b);
            return Ok(());
        }

        let (cmd, rest) = split_cmd(line);
        let defs = &self.prog.defines;
        let arg_c = strip_comment(rest).trim();

        // PRINT family
        if let Some(fl) = print_data_flags(&cmd) {
            b.ctl.push(Ctl::Data { var: None, flags: Some(fl), entries: vec![], list: None });
            return Ok(());
        }
        if cmd == "STRDATA" {
            let var = if arg_c.is_empty() { None } else { Some(self.var(arg_c)?) };
            b.ctl.push(Ctl::Data { var, flags: None, entries: vec![], list: None });
            return Ok(());
        }
        if !matches!(cmd.as_str(), "PRINTBUTTON" | "PRINTBUTTONC" | "PRINTBUTTONLC" | "PRINT_IMG" | "PRINT_RECT" | "PRINT_SPACE" | "PRINTCPERLINE" | "PRINTCLENGTH") {
            if let Some((mid, fl)) = parse_print_name(&cmd) {
                let content = match mid {
                    'R' => PrintContent::Raw(rest.to_string()),
                    'F' => PrintContent::Form(upper_form(parse_form(rest, defs)?)),
                    'G' => PrintContent::Exprs(vec![Expr::Call("__FORMS".into(), vec![upper_expr(parse_expr(arg_c, defs)?)])]),
                    'S' => PrintContent::Expr(upper_expr(parse_expr(arg_c, defs)?)),
                    'V' => PrintContent::Exprs(parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect()),
                    _ => unreachable!(),
                };
                b.emit(Instr::Print(content, fl));
                Self::after_stmt(b);
                return Ok(());
            }
        }

        match cmd.as_str() {
            "IF" => {
                let c = self.expr(arg_c)?;
                let t = b.emit(Instr::JumpIfNot(c, 0));
                b.ctl.push(Ctl::If { test: Some(t), ends: vec![] });
            }
            "ELSEIF" => {
                let c = self.expr(arg_c)?;
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::If { test, ends }) = b.ctl.last_mut() else { return Err("ELSEIF without IF".into()) };
                ends.push(j);
                let old = test.take();
                if let Some(t) = old {
                    b.patch(t, pc);
                }
                let t = b.emit(Instr::JumpIfNot(c, 0));
                if let Some(Ctl::If { test, .. }) = b.ctl.last_mut() {
                    *test = Some(t);
                }
            }
            "ELSE" => {
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::If { test, ends }) = b.ctl.last_mut() else { return Err("ELSE without IF".into()) };
                ends.push(j);
                if let Some(t) = test.take() {
                    b.patch(t, pc);
                }
            }
            "ENDIF" => {
                let Some(Ctl::If { test, ends }) = b.ctl.pop() else { return Err("ENDIF without IF".into()) };
                let pc = b.pc();
                if let Some(t) = test {
                    b.patch(t, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "SIF" => {
                let c = self.expr(arg_c)?;
                let t = b.emit(Instr::JumpIfNot(c, 0));
                // nested SIF: keep outer pending; patch both after next real statement
                b.sif.push(t);
                return Ok(());
            }
            "SELECTCASE" => {
                let e = self.expr(arg_c)?;
                let slot = b.slot();
                b.emit(Instr::SelectEval(e, slot));
                b.ctl.push(Ctl::Select { slot, test: None, ends: vec![], first: true });
            }
            "CASE" | "CASEELSE" => {
                let conds = if cmd == "CASE" { Some(self.case_conds(arg_c)?) } else { None };
                let (slot, first) = match b.ctl.last() {
                    Some(Ctl::Select { slot, first, .. }) => (*slot, *first),
                    _ => return Err("CASE without SELECTCASE".into()),
                };
                let j = if !first { Some(b.emit(Instr::Jump(0))) } else { None };
                let pc = b.pc();
                let prev = if let Some(Ctl::Select { test, ends, first, .. }) = b.ctl.last_mut() {
                    if let Some(j) = j {
                        ends.push(j);
                    }
                    *first = false;
                    test.take()
                } else {
                    None
                };
                if let Some(t) = prev {
                    b.patch(t, pc);
                }
                if let Some(conds) = conds {
                    let t = b.emit(Instr::CaseTest(slot, conds, 0));
                    if let Some(Ctl::Select { test, .. }) = b.ctl.last_mut() {
                        *test = Some(t);
                    }
                }
            }
            "ENDSELECT" => {
                let Some(Ctl::Select { test, ends, .. }) = b.ctl.pop() else { return Err("ENDSELECT without SELECTCASE".into()) };
                let pc = b.pc();
                if let Some(t) = test {
                    b.patch(t, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "FOR" | "REPEAT" => {
                let (var, start, end, step) = if cmd == "FOR" {
                    let parts = split_top(arg_c, ',');
                    if parts.len() < 3 {
                        return Err("FOR needs var, start, end".into());
                    }
                    let var = self.var(parts[0].trim())?;
                    let start = self.expr_or_zero(parts[1].trim())?;
                    let end = self.expr(parts[2].trim())?;
                    let step = if parts.len() > 3 && !parts[3].trim().is_empty() { Some(self.expr(parts[3].trim())?) } else { None };
                    (var, start, end, step)
                } else {
                    (Rc::new(VarRef { name: "COUNT".into(), idx: vec![] }), Expr::Int(0), self.expr(arg_c)?, None)
                };
                let slot = b.slot();
                let begin = b.emit(Instr::ForBegin { var: var.clone(), start, end, step, slot, exit: 0 });
                b.ctl.push(Ctl::For { begin, slot, var, breaks: vec![], conts: vec![] });
            }
            "NEXT" | "REND" => {
                let Some(Ctl::For { begin, slot, var, breaks, conts }) = b.ctl.pop() else { return Err(format!("{} without FOR/REPEAT", cmd)) };
                let next_pc = b.pc();
                for c in conts {
                    b.patch(c, next_pc);
                }
                b.emit(Instr::ForNext { var, slot, body: begin + 1 });
                let exit = b.pc();
                b.patch(begin, exit);
                for x in breaks {
                    b.patch(x, exit);
                }
                Self::after_stmt(b);
            }
            "WHILE" => {
                let c = self.expr(arg_c)?;
                let top = b.pc();
                let test = b.emit(Instr::JumpIfNot(c, 0));
                b.ctl.push(Ctl::While { top, test, breaks: vec![], conts: vec![] });
            }
            "WEND" => {
                let Some(Ctl::While { top, test, breaks, conts }) = b.ctl.pop() else { return Err("WEND without WHILE".into()) };
                b.emit(Instr::Jump(top));
                let exit = b.pc();
                b.patch(test, exit);
                for x in breaks {
                    b.patch(x, exit);
                }
                for c in conts {
                    b.patch(c, top);
                }
                Self::after_stmt(b);
            }
            "DO" => {
                let top = b.pc();
                b.ctl.push(Ctl::Do { top, breaks: vec![], conts: vec![] });
            }
            "LOOP" => {
                // DO … LOOP cond: repeat while cond is true
                let c = self.expr(arg_c)?;
                let Some(Ctl::Do { top, breaks, conts }) = b.ctl.pop() else { return Err("LOOP without DO".into()) };
                let lp = b.pc();
                b.emit(Instr::JumpIfNot(Expr::Unary(UnOp::Not, Box::new(c)), top));
                let exit = b.pc();
                for x in breaks {
                    b.patch(x, exit);
                }
                for c in conts {
                    b.patch(c, lp);
                }
                Self::after_stmt(b);
            }
            "BREAK" | "CONTINUE" => {
                let j = b.emit(Instr::Jump(0));
                let mut found = false;
                for c in b.ctl.iter_mut().rev() {
                    match c {
                        Ctl::For { breaks, conts, .. } | Ctl::While { breaks, conts, .. } | Ctl::Do { breaks, conts, .. } => {
                            if cmd == "BREAK" { breaks.push(j) } else { conts.push(j) }
                            found = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if !found {
                    return Err(format!("{} outside loop", cmd));
                }
                Self::after_stmt(b);
            }
            "CALL" | "JUMP" | "TRYCALL" | "TRYJUMP" | "CALLF" | "TRYCALLF" | "CALLFORM" | "JUMPFORM" | "TRYCALLFORM"
            | "TRYJUMPFORM" | "CALLFORMF" | "TRYCALLFORMF" | "TRYCCALL" | "TRYCJUMP" | "TRYCCALLFORM" | "TRYCJUMPFORM" => {
                let form = cmd.contains("FORM");
                let (target, args) = self.call_target(arg_c, form)?;
                let kind = if cmd.contains("JUMP") {
                    CallKind::Jump
                } else if cmd.ends_with("CALLF") || cmd.ends_with("FORMF") {
                    CallKind::CallF
                } else {
                    CallKind::Call
                };
                let try_ = cmd.starts_with("TRY");
                let c = b.emit(Instr::Call { target, args, kind, try_, catch: None });
                if matches!(cmd.as_str(), "TRYCCALL" | "TRYCJUMP" | "TRYCCALLFORM" | "TRYCJUMPFORM") {
                    b.ctl.push(Ctl::TryC { call: c, ends: vec![] });
                    return Ok(());
                }
                Self::after_stmt(b);
            }
            "TRYCGOTO" | "TRYCGOTOFORM" => {
                let target = self.goto_target(arg_c, cmd.contains("FORM"))?;
                let c = b.emit(Instr::Goto { target, try_: true, catch: None });
                b.ctl.push(Ctl::TryC { call: c, ends: vec![] });
            }
            "CATCH" => {
                let j = b.emit(Instr::Jump(0));
                let pc = b.pc();
                let Some(Ctl::TryC { call, ends }) = b.ctl.last_mut() else { return Err("CATCH without TRYC*".into()) };
                ends.push(j);
                let call = *call;
                b.patch(call, pc);
            }
            "ENDCATCH" => {
                let Some(Ctl::TryC { call, ends }) = b.ctl.pop() else { return Err("ENDCATCH without TRYC*".into()) };
                let pc = b.pc();
                if let Instr::Call { catch: None, .. } | Instr::Goto { catch: None, .. } = &b.f.code[call] {
                    b.patch(call, pc);
                }
                for e in ends {
                    b.patch(e, pc);
                }
                Self::after_stmt(b);
            }
            "TRYCALLLIST" | "TRYJUMPLIST" | "TRYGOTOLIST" => {
                let kind = if cmd == "TRYCALLLIST" { CallKind::Call } else { CallKind::Jump };
                b.ctl.push(Ctl::CallList { kind, goto: cmd == "TRYGOTOLIST", entries: vec![] });
            }
            "GOTO" | "TRYGOTO" | "GOTOFORM" | "TRYGOTOFORM" => {
                let target = self.goto_target(arg_c, cmd.contains("FORM"))?;
                b.emit(Instr::Goto { target, try_: cmd.starts_with("TRY"), catch: None });
                Self::after_stmt(b);
            }
            "RETURN" => {
                let v = if arg_c.is_empty() { vec![] } else { parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect() };
                b.emit(Instr::Return(v));
                Self::after_stmt(b);
            }
            "RETURNFORM" => {
                let mut v = Vec::new();
                for p in split_top(arg_c, ',') {
                    v.push(Expr::Form(Rc::new(upper_form(parse_form(p.trim(), defs)?))));
                }
                b.emit(Instr::Return(v));
                Self::after_stmt(b);
            }
            "RETURNF" => {
                let v = if arg_c.is_empty() { None } else { Some(self.expr(arg_c)?) };
                b.emit(Instr::ReturnF(v));
                Self::after_stmt(b);
            }
            "BEGIN" => {
                b.emit(Instr::Begin(arg_c.to_uppercase()));
                Self::after_stmt(b);
            }
            "RESTART" => {
                b.emit(Instr::Restart);
                Self::after_stmt(b);
            }
            "QUIT" => {
                b.emit(Instr::Quit);
                Self::after_stmt(b);
            }
            "THROW" => {
                b.emit(Instr::Throw(upper_form(parse_form(rest, defs)?)));
                Self::after_stmt(b);
            }
            c if IGNORED_BLOCKS.contains(&c) => {}
            _ => {
                if is_command(&cmd) {
                    let args = if raw_arg_cmd(&cmd) {
                        Args::Raw(arg_c.to_string())
                    } else if form_arg_cmd(&cmd) {
                        Args::Form(upper_form(parse_form(rest, defs)?))
                    } else if arg_c.is_empty() {
                        Args::None
                    } else {
                        Args::Exprs(parse_expr_list(arg_c, defs)?.into_iter().map(upper_expr).collect())
                    };
                    b.emit(Instr::Command(cmd, args));
                } else {
                    self.assignment(b, line)?;
                }
                Self::after_stmt(b);
            }
        }
        Ok(())
    }

    /// `VAR op value`. Integer and `'=` assignments take an expression list
    /// (`A = 1, 2, 3` fills A:0..2); a string variable's plain `=` takes
    /// FORM text, as in Emuera (`STR = Hello %NAME%`).
    fn assignment(&self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        let mut p = ExprParser::new(line, &self.prog.defines);
        let mut v = p.var_ref()?;
        upper_var(&mut v);
        p.skip_ws();
        let ops: &[(&str, AsgOp)] = &[
            ("'=", AsgOp::SetExpr),
            ("+=", AsgOp::Add),
            ("-=", AsgOp::Sub),
            ("*=", AsgOp::Mul),
            ("/=", AsgOp::Div),
            ("%=", AsgOp::Mod),
            ("&=", AsgOp::And),
            ("|=", AsgOp::Or),
            ("^=", AsgOp::Xor),
            ("<<=", AsgOp::Shl),
            (">>=", AsgOp::Shr),
        ];
        if p.eat("++") {
            b.emit(Instr::Eval(Expr::IncDec(v, false, 1)));
            return Ok(());
        }
        if p.eat("--") {
            b.emit(Instr::Eval(Expr::IncDec(v, false, -1)));
            return Ok(());
        }
        let mut op = None;
        for (t, o) in ops {
            if p.eat(t) {
                op = Some(*o);
                break;
            }
        }
        let op = match op {
            Some(o) => o,
            None => {
                if p.eat("=") {
                    AsgOp::Set
                } else {
                    return Err(format!("unknown command or bad assignment: '{}'", line));
                }
            }
        };
        let rest = p.rest();
        let is_str = self.is_str_var(b, &v.name);
        // only plain `STR = ...` takes FORM text; `+=` takes a string expression
        if is_str && op == AsgOp::Set {
            // Emuera skips/trims only half-width spaces and tabs here; a
            // leading full-width space is content (map rows start with them)
            let raw = strip_comment_form(&rest);
            let raw = raw.trim_matches([' ', '\t']);
            b.emit(Instr::AssignForm(v, op, upper_form(parse_form(raw, &self.prog.defines)?)));
            return Ok(());
        }
        let rhs_src = strip_comment(&rest).trim();
        let rhs: Vec<Expr> = if rhs_src.is_empty() {
            vec![if is_str { Expr::Str(String::new()) } else { Expr::Int(0) }]
        } else {
            parse_expr_list(rhs_src, &self.prog.defines)?.into_iter().map(upper_expr).collect()
        };
        b.emit(Instr::Assign(v, op, rhs));
        Ok(())
    }

    /// Function attributes: private `#DIM`s, `#FUNCTION(S)`, event order.
    fn directive(&mut self, b: &mut FuncBuilder, line: &str) -> Result<(), String> {
        let line = strip_comment(line).trim();
        let up = line.to_uppercase();
        if up.starts_with("#DIM") {
            let d = self.parse_dim(line)?;
            if d.is_str {
                b.str_locals.insert(d.name.clone());
            }
            b.f.dims.push(d);
            return Ok(());
        }
        match up.split_whitespace().next().unwrap_or("") {
            "#FUNCTION" => b.f.is_function = true,
            "#FUNCTIONS" => {
                b.f.is_function = true;
                b.f.returns_str = true
            }
            "#PRI" => b.f.order = EventOrder::Pri,
            "#LATER" => b.f.order = EventOrder::Later,
            "#SINGLE" => b.f.single = true,
            "#ONLY" => b.f.only = true,
            "#LOCALSIZE" | "#LOCALSSIZE" => {}
            _ => return Err(format!("unknown directive {}", line)),
        }
        Ok(())
    }

    fn expr(&self, s: &str) -> Result<Expr, String> {
        Ok(upper_expr(parse_expr(s, &self.prog.defines)?))
    }

    fn expr_or_zero(&self, s: &str) -> Result<Expr, String> {
        if s.is_empty() { Ok(Expr::Int(0)) } else { self.expr(s) }
    }

    fn var(&self, s: &str) -> Result<Rc<VarRef>, String> {
        let mut p = ExprParser::new(s, &self.prog.defines);
        let mut v = p.var_ref()?;
        if !p.at_end() {
            return Err(format!("bad variable '{}'", s));
        }
        upper_var(&mut v);
        Ok(v)
    }

    /// The conditions of one CASE line: `a`, `a TO b`, `IS op a`, comma-separated.
    fn case_conds(&self, s: &str) -> Result<Vec<CaseCond>, String> {
        let mut out = Vec::new();
        for part in split_top(s, ',') {
            let part = part.trim();
            if part.is_empty() {
                // trailing comma: `CASE 1, 2,`
                continue;
            }
            let up = part.to_uppercase();
            if let Some(r) = up.strip_prefix("IS ") {
                let r_orig = &part[part.len() - r.len()..];
                let r = r_orig.trim();
                let (op, e) = if let Some(x) = r.strip_prefix("<=") {
                    (BinOp::Le, x)
                } else if let Some(x) = r.strip_prefix(">=") {
                    (BinOp::Ge, x)
                } else if let Some(x) = r.strip_prefix("==") {
                    (BinOp::Eq, x)
                } else if let Some(x) = r.strip_prefix("!=") {
                    (BinOp::Ne, x)
                } else if let Some(x) = r.strip_prefix('<') {
                    (BinOp::Lt, x)
                } else if let Some(x) = r.strip_prefix('>') {
                    (BinOp::Gt, x)
                } else if let Some(x) = r.strip_prefix('&') {
                    (BinOp::BitAnd, x)
                } else {
                    return Err(format!("bad CASE IS: {}", part));
                };
                out.push(CaseCond::Is(op, self.expr(e.trim())?));
            } else if let Some(i) = find_word(&up, "TO") {
                out.push(CaseCond::Range(self.expr(part[..i].trim())?, self.expr(part[i + 2..].trim())?));
            } else {
                out.push(CaseCond::Eq(self.expr(part)?));
            }
        }
        Ok(out)
    }

    /// CALL target parsing: `NAME, a, b` / `NAME(a, b)` / FORM variants.
    fn call_target(&self, s: &str, form: bool) -> Result<(CallTarget, Vec<Expr>), String> {
        let s = s.trim();
        // find end of target: first top-level ',' or '('
        let mut end = s.len();
        let mut paren = false;
        let mut depth = 0;
        let mut in_pct = false;
        for (i, c) in s.char_indices() {
            match c {
                '%' if form => in_pct = !in_pct,
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' if depth == 0 && !in_pct => {
                    end = i;
                    break;
                }
                '(' if depth == 0 && !in_pct => {
                    end = i;
                    paren = true;
                    break;
                }
                _ => {}
            }
        }
        let name = s[..end].trim();
        let target = if form {
            CallTarget::Form(upper_form(parse_form(name, &self.prog.defines)?))
        } else {
            CallTarget::Static(name.to_uppercase())
        };
        let mut argsrc = s[end..].trim();
        if paren {
            argsrc = argsrc[1..].trim_end();
            argsrc = argsrc.strip_suffix(')').ok_or("missing ')' in CALL")?;
        } else if let Some(r) = argsrc.strip_prefix(',') {
            argsrc = r;
        }
        let args = if argsrc.trim().is_empty() {
            vec![]
        } else {
            parse_expr_list(argsrc, &self.prog.defines)?.into_iter().map(upper_expr).collect()
        };
        Ok((target, args))
    }

    fn goto_target(&self, s: &str, form: bool) -> Result<CallTarget, String> {
        Ok(if form {
            CallTarget::Form(upper_form(parse_form(s.trim(), &self.prog.defines)?))
        } else {
            CallTarget::Static(s.trim().to_uppercase())
        })
    }
}

fn strip_comment_form(s: &str) -> &str {
    // in FORM assignments ';' still starts a comment unless escaped
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        if c == ';' && prev != '\\' {
            return &s[..i];
        }
        prev = c;
    }
    s
}

/// Position of `w` as a whitespace-delimited word outside quotes
/// (`CASE 1 TO 5`), so names containing TO aren't split.
fn find_word(up: &str, w: &str) -> Option<usize> {
    let mut in_str = false;
    let b = up.as_bytes();
    for (i, c) in up.char_indices() {
        if c == '"' {
            in_str = !in_str;
        }
        if in_str || !up[i..].starts_with(w) {
            continue;
        }
        let before = if i == 0 { b' ' } else { b[i - 1] };
        let after = b.get(i + w.len()).copied().unwrap_or(b' ');
        if (before == b' ' || before == b'\t') && (after == b' ' || after == b'\t') {
            return Some(i);
        }
    }
    None
}

/// Normalize identifiers to uppercase (Emuera is case-insensitive by default).
pub fn upper_expr(e: Expr) -> Expr {
    match e {
        Expr::Var(v) => {
            let mut v = v;
            upper_var(&mut v);
            Expr::Var(v)
        }
        Expr::Call(n, a) => Expr::Call(n.to_uppercase(), a.into_iter().map(upper_expr).collect()),
        Expr::Unary(o, x) => Expr::Unary(o, Box::new(upper_expr(*x))),
        Expr::Binary(o, a, b) => Expr::Binary(o, Box::new(upper_expr(*a)), Box::new(upper_expr(*b))),
        Expr::Ternary(a, b, c) => Expr::Ternary(Box::new(upper_expr(*a)), Box::new(upper_expr(*b)), Box::new(upper_expr(*c))),
        Expr::IncDec(v, p, d) => {
            let mut v = v;
            upper_var(&mut v);
            Expr::IncDec(v, p, d)
        }
        Expr::Form(f) => Expr::Form(Rc::new(upper_form((*f).clone()))),
        e => e,
    }
}

pub fn upper_var(v: &mut Rc<VarRef>) {
    let inner = Rc::make_mut(v);
    // CSV-name indices (`ABL:技巧`) are upper-cased too: name-table
    // lookups are case-insensitive, so nothing is lost.
    inner.name = inner.name.to_uppercase();
    inner.idx = std::mem::take(&mut inner.idx).into_iter().map(upper_expr).collect();
}

pub fn upper_form(f: Form) -> Form {
    Form {
        parts: f
            .parts
            .into_iter()
            .map(|p| match p {
                FormPart::Str(e, w, a) => FormPart::Str(upper_expr(e), w.map(upper_expr), a),
                FormPart::Int(e, w, a) => FormPart::Int(upper_expr(e), w.map(upper_expr), a),
                FormPart::Cond(c, a, b) => FormPart::Cond(upper_expr(c), upper_form(a), upper_form(b)),
                l => l,
            })
            .collect(),
    }
}

/// Known statement-style commands (everything that is not an assignment).
pub fn is_command(c: &str) -> bool {
    crate::vm::builtins::is_builtin_command(c) || crate::vm::builtins::is_builtin_function(c)
}
