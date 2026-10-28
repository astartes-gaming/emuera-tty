//! Expression and FORM-string parser (char-level recursive descent).
//!
//! Two entry points matter:
//!
//! * [`parse_expr`] / [`ExprParser`] — Emuera expressions. Precedence is
//!   handled by `binary(level)`, one level per row of the table in
//!   `binop`, lowest (`||`) first; `primary` handles literals, `@"…"`,
//!   `\@…\@`, `'text`, calls, `#DEFINE` macros and `VAR:idx:idx` chains.
//! * [`parse_form`] — FORM strings (`text %str% {int} \@ c ? a # b \@`).
//!
//! The awkward part is nesting: `@"a %F("x")% b"` contains a quote that
//! does *not* end the outer string, because it is inside a `%…%`
//! expression. The `scan_*` functions find where such regions really end,
//! mirroring Emuera's own lexer, so every caller agrees on the boundaries.

use crate::ast::*;
use crate::fxhash::HashMap;
// Arc (aliased as Rc): parsed programs are built on several threads
use std::sync::Arc as Rc;

pub type PResult<T> = Result<T, String>;

/// Cursor over one expression's characters. `defs` are the program's
/// `#DEFINE`s (expanded inline, `depth` guards against runaway recursion).
pub struct ExprParser<'a> {
    s: Vec<char>,
    pub pos: usize,
    defs: &'a HashMap<String, String>,
    depth: usize,
}

/// Can `c` appear in an identifier? Anything that isn't whitespace or an
/// operator/punctuation character — so Japanese names like `好感度` work.
pub fn is_ident_char(c: char) -> bool {
    if c.is_ascii() {
        // a lookup table for ASCII (the lexer asks this for every character)
        const fn table() -> [bool; 128] {
            let mut t = [true; 128];
            let mut i = 0;
            while i < 128 {
                if (i as u8).is_ascii_whitespace() || i == 0x0b {
                    t[i] = false;
                }
                i += 1;
            }
            let ops = b"+-*/%=!<>&|^~?#:,()[]{}\"'@\\$;.";
            let mut k = 0;
            while k < ops.len() {
                t[ops[k] as usize] = false;
                k += 1;
            }
            t
        }
        static T: [bool; 128] = table();
        return T[c as usize];
    }
    !(c.is_whitespace() || c == '\u{3000}')
}

impl<'a> ExprParser<'a> {
    pub fn new(src: &str, defs: &'a HashMap<String, String>) -> Self {
        ExprParser { s: src.chars().collect(), pos: 0, defs, depth: 0 }
    }

    /// A sub-parser for a `#DEFINE` body, one expansion level deeper.
    fn with_depth(src: &str, defs: &'a HashMap<String, String>, depth: usize) -> Self {
        ExprParser { s: src.chars().collect(), pos: 0, defs, depth }
    }

    /// The unparsed remainder (for error messages and callers that take
    /// over after an expression, e.g. assignments).
    pub fn rest(&self) -> String {
        self.s[self.pos.min(self.s.len())..].iter().collect()
    }

    pub fn skip_ws(&mut self) {
        while self.pos < self.s.len() && (self.s[self.pos].is_whitespace() || self.s[self.pos] == '\u{3000}') {
            self.pos += 1;
        }
    }

    pub fn peek(&self) -> Option<char> {
        self.s.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.s.get(self.pos + n).copied()
    }

    pub fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.pos >= self.s.len()
    }

    fn starts_with(&self, t: &str) -> bool {
        t.chars().enumerate().all(|(k, c)| self.s.get(self.pos + k) == Some(&c))
    }

    /// Skip whitespace and consume `t` if it comes next.
    pub fn eat(&mut self, t: &str) -> bool {
        self.skip_ws();
        if self.starts_with(t) {
            self.pos += t.chars().count();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, t: &str) -> PResult<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(format!("expected '{}' near '{}'", t, self.rest()))
        }
    }

    /// Read an identifier (not starting with a digit), if there is one.
    pub fn ident(&mut self) -> Option<String> {
        self.skip_ws();
        let start = self.pos;
        if let Some(c) = self.peek() {
            if c.is_ascii_digit() || !is_ident_char(c) {
                return None;
            }
        }
        while let Some(c) = self.peek() {
            if is_ident_char(c) {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos > start {
            Some(self.s[start..self.pos].iter().collect())
        } else {
            None
        }
    }

    /// Comma-separated expression list (allows empty entries).
    pub fn expr_list(&mut self) -> PResult<Vec<Expr>> {
        let mut out = Vec::new();
        if self.at_end() {
            return Ok(out);
        }
        loop {
            self.skip_ws();
            if self.peek() == Some(',') || self.peek().is_none() || self.peek() == Some(')') {
                out.push(Expr::Empty);
            } else if let Some(list) = self.define_list()? {
                out.extend(list);
            } else {
                out.push(self.expr()?);
            }
            if !self.eat(",") {
                break;
            }
            // a trailing comma adds no value, as in Emuera's
            // ReduceArguments (`X += 1,` is one value, `F(a,)` one argument)
            self.skip_ws();
            if self.peek().is_none() || self.peek() == Some(')') {
                break;
            }
        }
        Ok(out)
    }

    /// An argument that is just the name of a #DEFINE whose body is a list
    /// (`#DEFINE ＃X＃ FLAG:A,3`, then `GETBIT(＃X＃)` or `SETBIT ＃X＃`):
    /// its values, each an argument, as Emuera puts the body's tokens in
    /// place of the name. None (and nothing read) for anything else.
    fn define_list(&mut self) -> PResult<Option<Vec<Expr>>> {
        let save = self.pos;
        let Some(name) = self.ident() else { return Ok(None) };
        self.skip_ws();
        let body = self.defs.get(&name).or_else(|| self.defs.get(&name.to_uppercase()));
        let ends = matches!(self.peek(), None | Some(',') | Some(')'));
        match body {
            Some(body) if ends && split_top(body, ',').len() > 1 && self.depth <= 32 => {
                let mut sub = ExprParser::with_depth(body, self.defs, self.depth + 1);
                let list = sub.expr_list()?;
                if !sub.at_end() {
                    return Err(format!("bad #DEFINE body for {}", name));
                }
                Ok(Some(list))
            }
            _ => {
                self.pos = save;
                Ok(None)
            }
        }
    }

    /// A full expression, including the lowest-precedence `c ? a # b`.
    pub fn expr(&mut self) -> PResult<Expr> {
        let c = self.binary(0)?;
        if self.eat("?") {
            let a = self.expr()?;
            self.expect("#")?;
            let b = self.expr()?;
            return Ok(Expr::Ternary(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    /// Consume a binary operator of the given precedence level (0 binds
    /// loosest). Levels are Emuera's (OperatorCode.cs priorities): the
    /// logical operators `|| && ^^ !& !|` share one level and the bitwise
    /// `| ^ &` another, each read left to right (so `a || b && c` is
    /// `(a || b) && c`), then equality < comparison < shifts < `+ -` <
    /// `* / %`.
    fn binop(&mut self, level: usize) -> Option<BinOp> {
        self.skip_ws();
        let ops: &[(&str, BinOp)] = match level {
            0 => &[("||", BinOp::Or), ("&&", BinOp::And), ("^^", BinOp::Xor), ("!&", BinOp::Nand), ("!|", BinOp::Nor)],
            1 => &[("|", BinOp::BitOr), ("^", BinOp::BitXor), ("&", BinOp::BitAnd)],
            2 => &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
            3 => &[("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)],
            4 => &[("<<", BinOp::Shl), (">>", BinOp::Shr)],
            5 => &[("+", BinOp::Add), ("-", BinOp::Sub)],
            6 => &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)],
            _ => return None,
        };
        for (t, op) in ops {
            if self.starts_with(t) {
                // don't eat compound assignment / other ops by prefix
                let after = self.peek_at(t.chars().count());
                let bad = match *t {
                    "|" => matches!(after, Some('|') | Some('=')),
                    "&" => matches!(after, Some('&') | Some('=')),
                    "^" => matches!(after, Some('^') | Some('=')),
                    "+" | "-" | "*" | "/" | "%" => after == Some('=') || (*t == "+" && after == Some('+')) || (*t == "-" && after == Some('-')),
                    "<" => matches!(after, Some('<') | Some('=')),
                    ">" => matches!(after, Some('>') | Some('=')),
                    "<<" | ">>" => after == Some('='),
                    _ => false,
                };
                if bad {
                    continue;
                }
                self.pos += t.chars().count();
                return Some(*op);
            }
        }
        None
    }

    /// Left-associative chain of operators at `level` and tighter ones.
    fn binary(&mut self, level: usize) -> PResult<Expr> {
        if level > 6 {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.binop(level) {
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    /// Prefix operators: `++X`, `--X`, `-`, `+`, `!`, `~`.
    fn unary(&mut self) -> PResult<Expr> {
        self.skip_ws();
        if self.starts_with("++") || self.starts_with("--") {
            let d = if self.starts_with("++") { 1 } else { -1 };
            self.pos += 2;
            let e = self.unary()?;
            if let Expr::Var(v) = e {
                return Ok(Expr::IncDec(v, true, d));
            }
            return Err("++/-- needs a variable".into());
        }
        match self.peek() {
            Some('-') => {
                self.pos += 1;
                let e = self.unary()?;
                if let Expr::Int(i) = e {
                    return Ok(Expr::Int(-i));
                }
                Ok(Expr::Unary(UnOp::Neg, Box::new(e)))
            }
            Some('+') => {
                self.pos += 1;
                self.unary()
            }
            Some('!') => {
                self.pos += 1;
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)))
            }
            Some('~') => {
                self.pos += 1;
                Ok(Expr::Unary(UnOp::BitNot, Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    /// A primary, then an optional postfix `X++` / `X--` on a variable.
    fn postfix(&mut self) -> PResult<Expr> {
        let e = self.primary(true)?;
        if let Expr::Var(v) = &e {
            // Emuera's lexer skips spaces between tokens: `X ++` too
            let at = self.pos;
            self.skip_ws();
            if self.starts_with("++") || self.starts_with("--") {
                let d = if self.starts_with("++") { 1 } else { -1 };
                self.pos += 2;
                return Ok(Expr::IncDec(v.clone(), false, d));
            }
            self.pos = at;
        }
        Ok(e)
    }

    /// Integer literal: decimal, `0x…`, `0b…`, plus Emuera's `1p3` (shift)
    /// and `1e3` (power of ten) suffixes.
    fn number(&mut self) -> PResult<Expr> {
        let start = self.pos;
        let radix = if self.starts_with("0x") || self.starts_with("0X") {
            self.pos += 2;
            16
        } else if self.starts_with("0b") || self.starts_with("0B") {
            self.pos += 2;
            2
        } else {
            10
        };
        let ds = self.pos;
        while let Some(c) = self.peek() {
            if c.is_digit(radix) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let digits: String = self.s[ds..self.pos].iter().collect();
        let bad = || format!("bad number '{}'", self.s[start..self.pos].iter().collect::<String>());
        // hex and binary fill all 64 bits, as C#'s Convert.ToInt64(s, 16):
        // `0xffffffffffffffff` is -1 (erarl's bit arrays)
        let mut v =
            if radix == 10 { i64::from_str_radix(&digits, radix).map_err(|_| bad())? } else { u64::from_str_radix(&digits, radix).map_err(|_| bad())? as i64 };
        // Emuera: 1p3 == 1<<3, 1e3 == 1000
        if radix == 10 {
            if let Some(c @ ('p' | 'P' | 'e' | 'E')) = self.peek() {
                if self.peek_at(1).map(|d| d.is_ascii_digit()).unwrap_or(false) {
                    self.pos += 1;
                    let es = self.pos;
                    while self.peek().map(|d| d.is_ascii_digit()).unwrap_or(false) {
                        self.pos += 1;
                    }
                    let e: u32 = self.s[es..self.pos].iter().collect::<String>().parse().unwrap_or(0);
                    v = if c == 'p' || c == 'P' { v << e } else { v * 10i64.pow(e) };
                }
            }
        }
        Ok(Expr::Int(v))
    }

    /// A plain `"…"` literal (no interpolation), with `\n`, `\t`, `\"`.
    pub fn string_lit(&mut self) -> PResult<String> {
        // at '"'
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("unterminated string".into()),
                Some('"') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') => {
                    self.pos += 1;
                    match self.peek() {
                        // Emuera's escapes: \s space, \S full-width space
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('s') => out.push(' '),
                        Some('S') => out.push('　'),
                        Some(c) => out.push(c),
                        None => {}
                    }
                    self.pos += 1;
                }
                Some(c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Ok(out)
    }

    /// The innermost terms. `allow_index` is false while parsing an index
    /// itself, so `A:B:C` is one variable with two indices rather than
    /// `A:(B:C)`.
    fn primary(&mut self, allow_index: bool) -> PResult<Expr> {
        self.skip_ws();
        let Some(c) = self.peek() else { return Err("unexpected end of expression".into()) };
        if c.is_ascii_digit() {
            return self.number();
        }
        if c == '"' {
            return Ok(Expr::Str(self.string_lit()?));
        }
        if c == '@' && self.peek_at(1) == Some('"') {
            let start = self.pos + 2;
            let end = scan_form(&self.s, start, FormEnd::Quote).ok_or("unterminated form string")?;
            let raw: String = self.s[start..end].iter().collect();
            self.pos = end + 1;
            return Ok(Expr::Form(Rc::new(parse_form(&raw, self.defs)?)));
        }
        if c == '\\' && self.peek_at(1) == Some('@') {
            // string-valued `\@ cond ? a # b \@`
            let end = scan_ternary(&self.s, self.pos + 2).ok_or("unterminated \\@ in expression")?;
            let raw: String = self.s[self.pos..end].iter().collect();
            self.pos = end;
            return Ok(Expr::Form(Rc::new(parse_form(&raw, self.defs)?)));
        }
        if c == '\'' {
            // Emuera simple string: 'text runs to the next comma
            self.pos += 1;
            let start = self.pos;
            while self.peek().is_some_and(|c| c != ',') {
                self.pos += 1;
            }
            return Ok(Expr::Str(self.s[start..self.pos].iter().collect()));
        }
        if c == '(' {
            self.pos += 1;
            let e = self.expr()?;
            self.expect(")")?;
            return Ok(e);
        }
        if c == '-' {
            self.pos += 1;
            let e = self.primary(allow_index)?;
            return Ok(match e {
                Expr::Int(i) => Expr::Int(-i),
                e => Expr::Unary(UnOp::Neg, Box::new(e)),
            });
        }
        let Some(mut name) = self.ident() else {
            return Err(format!("unexpected '{}' in expression", self.rest()));
        };
        // LOCAL@FUNC (LOCALS, ARG, ARGS too): another function's own
        if self.peek() == Some('@') && matches!(name.to_uppercase().as_str(), "LOCAL" | "LOCALS" | "ARG" | "ARGS") {
            self.pos += 1;
            match self.ident() {
                Some(f) => name = format!("{}@{}", name, f),
                None => return Err(format!("{}@: no function name", name)),
            }
        }
        // #DEFINE macro expansion (names are kept upper-cased: Emuera
        // ignores case, `#DEFINE VirginKiss 0` is used as VirginKiss)
        let def = self.defs.get(&name).or_else(|| if name.chars().any(|c| c.is_lowercase()) { self.defs.get(&name.to_uppercase()) } else { None });
        if let Some(body) = def {
            if self.depth > 32 {
                return Err("#DEFINE recursion too deep".into());
            }
            // First, try parsing the body as an expression to see if it's a simple variable reference.
            // This handles the case where `#DEFINE KFLAG JUEL` should allow `KFLAG:1` -> `JUEL:1`.
            let mut sub = ExprParser::with_depth(body, self.defs, self.depth + 1);
            let e = sub.expr()?;
            let _body_is_var = matches!(&e, Expr::Var(_));
            if !sub.at_end() {
                return Err(format!("bad #DEFINE body for {}", name));
            }
            // Emuera replaces the name by the body's tokens, so indices
            // after it belong to the variable it names (`#DEFINE KFLAG
            // JUEL`, then `KFLAG:1` is `JUEL:1`)
            if let (Expr::Var(v), true) = (&e, allow_index) {
                let more = self.index_chain()?;
                if !more.is_empty() {
                    let idx: Vec<Expr> = v.idx.iter().cloned().chain(more).collect();
                    return Ok(Expr::Var(Rc::new(VarRef::new(v.name.clone(), idx))));
                }
                // It's a variable reference but no extra indices - return the var
                return Ok(e);
            }
            // For non-variable bodies (expressions, strings, etc.), do textual substitution
            // like Emuera's lexical analyzer: insert the body text into the token stream.
            // This preserves operator precedence (e.g., `#DEFINE HDR_BARE 2 + 3` then
            // `HDR_BARE * 2` becomes `2 + 3 * 2` = 8, not `(2 + 3) * 2` = 10).
            let remaining: String = self.s[self.pos..].iter().collect();
            let new_input = if remaining.trim().is_empty() { body.clone() } else { format!("{} {}", body, remaining) };
            // Replace this parser's input with the expanded text and re-parse.
            self.s = new_input.chars().collect();
            self.pos = 0;
            self.depth += 1;
            // Re-parse from the start (which now begins with the macro body)
            return self.primary(allow_index);
        }
        // Emuera accepts a space between a function name and its '('
        let save = self.pos;
        self.skip_ws();
        if self.peek() != Some('(') {
            self.pos = save;
        }
        if self.peek() == Some('(') {
            self.pos += 1;
            let args = if self.eat(")") {
                Vec::new()
            } else {
                let a = self.expr_list()?;
                self.expect(")")?;
                a
            };
            return Ok(Expr::Call(Box::new((name, args, Default::default()))));
        }
        let idx = if allow_index { self.index_chain()? } else { Vec::new() };
        Ok(Expr::Var(Rc::new(VarRef::new(name.into(), idx))))
    }

    /// An index chain after a variable: `:a:b:c` (Emuera also accepts
    /// `NAME :a`).
    fn index_chain(&mut self) -> PResult<Vec<Expr>> {
        let mut idx = Vec::new();
        loop {
            let save = self.pos;
            self.skip_ws();
            if self.peek() != Some(':') {
                self.pos = save;
                break;
            }
            self.pos += 1;
            self.skip_ws();
            idx.push(self.primary(false)?);
        }
        Ok(idx)
    }

    /// Parse a variable reference (assignment target).
    pub fn var_ref(&mut self) -> PResult<Rc<VarRef>> {
        match self.primary(true)? {
            Expr::Var(v) => Ok(v),
            other => Err(format!("expected variable, got {:?}", other)),
        }
    }
}

/// Parse `src` as exactly one expression (trailing text is an error).
pub fn parse_expr(src: &str, defs: &HashMap<String, String>) -> PResult<Expr> {
    let mut p = ExprParser::new(src, defs);
    let e = p.expr()?;
    if !p.at_end() {
        return Err(format!("trailing garbage '{}'", p.rest()));
    }
    Ok(e)
}

/// Parse a comma list such as command arguments; `a,,b` yields an
/// [`Expr::Empty`] in the middle.
pub fn parse_expr_list(src: &str, defs: &HashMap<String, String>) -> PResult<Vec<Expr>> {
    let mut p = ExprParser::new(src, defs);
    let e = p.expr_list()?;
    if !p.at_end() {
        return Err(format!("trailing garbage '{}'", p.rest()));
    }
    Ok(e)
}

// ---- nesting-aware scanners (mirror Emuera's lexer) ----
//
// A FORM string's `%..%` / `{..}` regions are expressions, which may hold
// `".."`, `@".."` and `\@ c ? a # b \@`, nested arbitrarily. These return
// char indices into `ch`, or None if the input ends first.

#[derive(Clone, Copy, PartialEq)]
enum FormEnd {
    Quote,
    YenAt,
    SharpOrYenAt,
}

/// Index just past the closing `"` of a plain string starting at `ch[i] == '"'`.
fn skip_plain_str(ch: &[char], mut i: usize) -> Option<usize> {
    i += 1;
    while i < ch.len() {
        match ch[i] {
            '\\' => i += 2,
            '"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// The length in bytes of the FORM string `s` starts with (at its opening
/// `"`, the `@` before it not included) up to and with its closing `"`;
/// None when it isn't closed.
pub fn form_string_len(s: &str) -> Option<usize> {
    let ch: Vec<char> = s.chars().collect();
    let end = scan_form(&ch, 1, FormEnd::Quote)?;
    Some(ch[..=end].iter().map(|c| c.len_utf8()).sum())
}

/// Split FORM text at `sep` outside its `%…%`, `{…}` and `\@…\@` parts
/// and escapes (RETURNFORM's arguments: `RETURNFORM %A,10,LEFT%, B`).
pub fn split_form_top(s: &str, sep: char) -> Vec<String> {
    let ch: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' && ch.get(i + 1) == Some(&'@') {
            i = scan_ternary(&ch, i + 2).unwrap_or(ch.len());
            continue;
        }
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == '%' || c == '{' {
            if let Some(j) = scan_expr(&ch, i + 1, if c == '%' { '%' } else { '}' }) {
                i = j + 1;
                continue;
            }
        }
        if c == sep {
            out.push(ch[start..i].iter().collect());
            start = i + 1;
        }
        i += 1;
    }
    out.push(ch[start.min(ch.len())..].iter().collect());
    out
}

/// Index of `term` at paren depth 0, scanning an expression from `i`.
fn scan_expr(ch: &[char], mut i: usize, term: char) -> Option<usize> {
    let mut depth = 0i32;
    while i < ch.len() {
        let c = ch[i];
        if c == '"' {
            i = skip_plain_str(ch, i)?;
            continue;
        }
        if c == '@' && ch.get(i + 1) == Some(&'"') {
            i = scan_form(ch, i + 2, FormEnd::Quote)? + 1;
            continue;
        }
        if c == '\\' && ch.get(i + 1) == Some(&'@') {
            i = scan_ternary(ch, i + 2)?;
            continue;
        }
        if c == term && depth == 0 {
            return Some(i);
        }
        if c == '(' || (term == '}' && c == '{') {
            depth += 1;
        } else if c == ')' || (term == '}' && c == '}') {
            depth -= 1;
        }
        i += 1;
    }
    None
}

/// `i` is just after an opening `\@`; returns the index just past the closing `\@`.
fn scan_ternary(ch: &[char], i: usize) -> Option<usize> {
    let q = scan_expr(ch, i, '?')?;
    Some(scan_form(ch, q + 1, FormEnd::YenAt)? + 2)
}

/// Index of the FORM terminator (`"`, `\@` or `#`) starting the scan at `i`.
fn scan_form(ch: &[char], mut i: usize, end: FormEnd) -> Option<usize> {
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' && ch.get(i + 1) == Some(&'@') {
            if end != FormEnd::Quote {
                return Some(i);
            }
            i = scan_ternary(ch, i + 2)?;
            continue;
        }
        match c {
            '\\' => i += 2,
            '%' | '{' => {
                let close = if c == '%' { '%' } else { '}' };
                // unmatched '%' / '{' is literal text
                i = scan_expr(ch, i + 1, close).map_or(i + 1, |j| j + 1);
            }
            '"' if end == FormEnd::Quote => return Some(i),
            '#' if end == FormEnd::SharpOrYenAt => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Split at top-level occurrences of `sep` (outside quotes/parens/braces/%..%).
pub fn split_top(s: &str, sep: char) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // copy nested strings / \@ sections through whole
        let skip = if c == '"' {
            skip_plain_str(&chars, i)
        } else if c == '@' && chars.get(i + 1) == Some(&'"') {
            scan_form(&chars, i + 2, FormEnd::Quote).map(|j| j + 1)
        } else if c == '\\' && chars.get(i + 1) == Some(&'@') {
            scan_ternary(&chars, i + 2)
        } else {
            None
        };
        if let Some(j) = skip {
            cur.extend(&chars[i..j]);
            i = j;
            continue;
        }
        if in_str {
            cur.push(c);
            if c == '\\' && i + 1 < chars.len() {
                cur.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            // unterminated string: rest is one piece
            in_str = true;
            cur.push(c);
        } else if c == '(' || c == '{' {
            depth += 1;
            cur.push(c);
        } else if c == ')' || c == '}' {
            depth -= 1;
            cur.push(c);
        } else if c == sep && depth == 0 {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
        i += 1;
    }
    out.push(cur);
    out
}

/// The optional `,width,LEFT` tail of `%expr,width,align%`: pad to width
/// (right-aligned unless LEFT).
fn width_align(parts: &[String], defs: &HashMap<String, String>) -> PResult<(Option<Expr>, Align)> {
    let w = match parts.get(1) {
        Some(s) if !s.trim().is_empty() => Some(parse_expr(s.trim(), defs)?),
        _ => None,
    };
    let a = match parts.get(2).map(|s| s.trim().to_uppercase()) {
        Some(ref s) if s == "LEFT" => Align::Left,
        _ => Align::Right,
    };
    Ok((w, a))
}

/// Parse a FORM string: `text %strexpr% {intexpr} \@ c ? a # b \@`.
pub fn parse_form(src: &str, defs: &HashMap<String, String>) -> PResult<Form> {
    let ch: Vec<char> = src.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    let flush = |lit: &mut String, parts: &mut Vec<FormPart>| {
        if !lit.is_empty() {
            // built a char at a time: drop the growth slack, it's kept for good
            let mut s = std::mem::take(lit);
            s.shrink_to_fit();
            parts.push(FormPart::Lit(s));
        }
    };
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' && i + 1 < ch.len() {
            let n = ch[i + 1];
            if n == '@' {
                // conditional section: \@ cond ? a # b \@
                let start = i + 2;
                let q = scan_expr(&ch, start, '?').ok_or("unterminated \\@ in FORM")?;
                let k = scan_form(&ch, q + 1, FormEnd::SharpOrYenAt).ok_or("unterminated \\@ in FORM")?;
                let (a_end, end) = if ch[k] == '#' { (k, scan_form(&ch, k + 1, FormEnd::YenAt).ok_or("unterminated \\@ in FORM")?) } else { (k, k) };
                let text = |a: usize, b: usize| ch[a..b].iter().collect::<String>().trim().to_string();
                let cond = parse_expr(&text(start, q), defs)?;
                let a = text(q + 1, a_end);
                let b = if end > a_end { text(a_end + 1, end) } else { String::new() };
                flush(&mut lit, &mut parts);
                parts.push(FormPart::Cond(Box::new((cond, parse_form(&a, defs)?, parse_form(&b, defs)?))));
                i = end + 2;
                continue;
            }
            match n {
                'n' => lit.push('\n'),
                's' => lit.push(' '),
                'S' => lit.push('　'),
                't' => lit.push('\t'),
                _ => lit.push(n),
            }
            i += 2;
            continue;
        }
        if c == '%' || c == '{' {
            let close = if c == '%' { '%' } else { '}' };
            let Some(j) = scan_expr(&ch, i + 1, close) else {
                // unmatched: treat literally
                lit.push(c);
                i += 1;
                continue;
            };
            let inner: String = ch[i + 1..j].iter().collect();
            let sp = split_top(&inner, ',');
            let e = parse_expr(sp[0].trim(), defs)?;
            let (w, a) = width_align(&sp, defs)?;
            flush(&mut lit, &mut parts);
            let w = w.map(Box::new);
            parts.push(if c == '%' { FormPart::Str(e, w, a) } else { FormPart::Int(e, w, a) });
            i = j + 1;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    flush(&mut lit, &mut parts);
    parts.shrink_to_fit();
    Ok(Form { parts })
}

#[cfg(test)]
mod ident_tests {
    #[test]
    fn ident_chars() {
        for c in "+-*/%=!<>&|^~?#:,()[]{}\"'@\\$;. \t\n\r\u{3000}\u{0b}\u{0c}".chars() {
            assert!(!super::is_ident_char(c), "{:?}", c);
        }
        for c in "aZ09_あ漢ｱ".chars() {
            assert!(super::is_ident_char(c), "{:?}", c);
        }
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;

    /// A trailing comma adds no value, as in Emuera; one in the middle is
    /// an omitted value.
    #[test]
    fn trailing_comma() {
        let n = |s: &str| parse_expr_list(s, &Default::default()).unwrap().len();
        assert_eq!(n("10 + RAND:10,"), 1);
        assert_eq!(n("1, 2 ,  "), 2);
        assert_eq!(n("1,,3"), 3);
        assert_eq!(n(",2"), 2);
        let call = parse_expr("F(1,)", &Default::default()).unwrap();
        assert!(matches!(&call, Expr::Call(c) if c.1.len() == 1), "{:?}", call);
    }
}
