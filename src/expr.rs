//! Expression and FORM-string parser (char-level recursive descent).

use crate::ast::*;
use std::collections::HashMap;
use std::rc::Rc;

pub type PResult<T> = Result<T, String>;

pub struct Defines<'a> {
    pub map: &'a HashMap<String, String>,
}

pub struct ExprParser<'a> {
    s: Vec<char>,
    pub pos: usize,
    defs: &'a HashMap<String, String>,
    depth: usize,
}

pub fn is_ident_char(c: char) -> bool {
    !(c.is_whitespace()
        || c == '\u{3000}'
        || "+-*/%=!<>&|^~?#:,()[]{}\"'@\\$;.".contains(c))
}

impl<'a> ExprParser<'a> {
    pub fn new(src: &str, defs: &'a HashMap<String, String>) -> Self {
        ExprParser { s: src.chars().collect(), pos: 0, defs, depth: 0 }
    }

    fn with_depth(src: &str, defs: &'a HashMap<String, String>, depth: usize) -> Self {
        ExprParser { s: src.chars().collect(), pos: 0, defs, depth }
    }

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
        let mut i = self.pos;
        for c in t.chars() {
            if self.s.get(i) != Some(&c) {
                return false;
            }
            i += 1;
        }
        true
    }

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
        if self.eat(t) { Ok(()) } else { Err(format!("expected '{}' near '{}'", t, self.rest())) }
    }

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
        if self.pos > start { Some(self.s[start..self.pos].iter().collect()) } else { None }
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
            } else {
                out.push(self.expr()?);
            }
            if !self.eat(",") {
                break;
            }
        }
        Ok(out)
    }

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

    fn binop(&mut self, level: usize) -> Option<BinOp> {
        self.skip_ws();
        let ops: &[(&str, BinOp)] = match level {
            0 => &[("||", BinOp::Or), ("!|", BinOp::Nor)],
            1 => &[("&&", BinOp::And), ("!&", BinOp::Nand)],
            2 => &[("|", BinOp::BitOr)],
            3 => &[("^", BinOp::BitXor)],
            4 => &[("&", BinOp::BitAnd)],
            5 => &[("==", BinOp::Eq), ("!=", BinOp::Ne)],
            6 => &[("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)],
            7 => &[("<<", BinOp::Shl), (">>", BinOp::Shr)],
            8 => &[("+", BinOp::Add), ("-", BinOp::Sub)],
            9 => &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Mod)],
            _ => return None,
        };
        for (t, op) in ops {
            if self.starts_with(t) {
                // don't eat compound assignment / other ops by prefix
                let after = self.peek_at(t.chars().count());
                let bad = match *t {
                    "|" => matches!(after, Some('|') | Some('=')),
                    "&" => matches!(after, Some('&') | Some('=')),
                    "^" | "+" | "-" | "*" | "/" | "%" => after == Some('=') || (*t == "+" && after == Some('+')) || (*t == "-" && after == Some('-')),
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

    fn binary(&mut self, level: usize) -> PResult<Expr> {
        if level > 9 {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(op) = self.binop(level) {
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

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

    fn postfix(&mut self) -> PResult<Expr> {
        let e = self.primary(true)?;
        if let Expr::Var(v) = &e {
            if self.starts_with("++") || self.starts_with("--") {
                let d = if self.starts_with("++") { 1 } else { -1 };
                self.pos += 2;
                return Ok(Expr::IncDec(v.clone(), false, d));
            }
        }
        Ok(e)
    }

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
        let mut v = i64::from_str_radix(&digits, radix)
            .map_err(|_| format!("bad number '{}'", self.s[start..self.pos].iter().collect::<String>()))?;
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
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
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
            self.pos += 1;
            // raw scan to the closing quote respecting \"
            self.pos += 1;
            let mut raw = String::new();
            loop {
                match self.peek() {
                    None => return Err("unterminated form string".into()),
                    Some('"') => {
                        self.pos += 1;
                        break;
                    }
                    Some('\\') => {
                        raw.push('\\');
                        self.pos += 1;
                        if let Some(n) = self.peek() {
                            raw.push(n);
                            self.pos += 1;
                        }
                    }
                    Some(ch) => {
                        raw.push(ch);
                        self.pos += 1;
                    }
                }
            }
            return Ok(Expr::Form(Rc::new(parse_form(&raw, self.defs)?)));
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
        let Some(name) = self.ident() else {
            return Err(format!("unexpected '{}' in expression", self.rest()));
        };
        // #DEFINE macro expansion
        if let Some(body) = self.defs.get(&name) {
            if self.depth > 32 {
                return Err("#DEFINE recursion too deep".into());
            }
            let mut sub = ExprParser::with_depth(body, self.defs, self.depth + 1);
            let e = sub.expr()?;
            if !sub.at_end() {
                return Err(format!("bad #DEFINE body for {}", name));
            }
            return Ok(e);
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
            return Ok(Expr::Call(name, args));
        }
        let mut idx = Vec::new();
        if allow_index {
            // index chain: NAME:a:b:c (no whitespace before ':')
            while self.peek() == Some(':') {
                self.pos += 1;
                self.skip_ws();
                idx.push(self.primary(false)?);
            }
        }
        Ok(Expr::Var(Rc::new(VarRef { name, idx })))
    }

    /// Parse a variable reference (assignment target).
    pub fn var_ref(&mut self) -> PResult<Rc<VarRef>> {
        match self.primary(true)? {
            Expr::Var(v) => Ok(v),
            other => Err(format!("expected variable, got {:?}", other)),
        }
    }
}

pub fn parse_expr(src: &str, defs: &HashMap<String, String>) -> PResult<Expr> {
    let mut p = ExprParser::new(src, defs);
    let e = p.expr()?;
    if !p.at_end() {
        return Err(format!("trailing garbage '{}'", p.rest()));
    }
    Ok(e)
}

pub fn parse_expr_list(src: &str, defs: &HashMap<String, String>) -> PResult<Vec<Expr>> {
    let mut p = ExprParser::new(src, defs);
    let e = p.expr_list()?;
    if !p.at_end() {
        return Err(format!("trailing garbage '{}'", p.rest()));
    }
    Ok(e)
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
            parts.push(FormPart::Lit(std::mem::take(lit)));
        }
    };
    while i < ch.len() {
        let c = ch[i];
        if c == '\\' && i + 1 < ch.len() {
            let n = ch[i + 1];
            if n == '@' {
                // conditional section, find matching \@
                let start = i + 2;
                let mut j = start;
                let mut nest = 0;
                let mut end = None;
                while j + 1 < ch.len() + 1 && j < ch.len() {
                    if ch[j] == '\\' && j + 1 < ch.len() && ch[j + 1] == '@' {
                        // a nested \@ opens only if a '?' follows before the next \@ ... keep simple:
                        if nest == 0 {
                            end = Some(j);
                            break;
                        }
                        nest -= 1;
                        j += 2;
                        continue;
                    }
                    j += 1;
                }
                let end = end.ok_or("unterminated \\@ in FORM")?;
                let inner: String = ch[start..end].iter().collect();
                let q = split_top(&inner, '?');
                if q.len() < 2 {
                    return Err(format!("bad \\@ conditional: {}", inner));
                }
                let cond = parse_expr(q[0].trim(), defs)?;
                let rest = q[1..].join("?");
                let h = split_top(&rest, '#');
                let a = h[0].trim().to_string();
                let b = if h.len() > 1 { h[1..].join("#").trim().to_string() } else { String::new() };
                flush(&mut lit, &mut parts);
                parts.push(FormPart::Cond(cond, parse_form(&a, defs)?, parse_form(&b, defs)?));
                i = end + 2;
                continue;
            }
            match n {
                'n' => lit.push('\n'),
                's' => lit.push(' '),
                _ => lit.push(n),
            }
            i += 2;
            continue;
        }
        if c == '%' || c == '{' {
            let close = if c == '%' { '%' } else { '}' };
            let mut j = i + 1;
            let mut depth = 0;
            let mut in_str = false;
            while j < ch.len() {
                let d = ch[j];
                if in_str {
                    if d == '\\' {
                        j += 2;
                        continue;
                    }
                    if d == '"' {
                        in_str = false;
                    }
                } else if d == '"' {
                    in_str = true;
                } else if d == '(' || (c == '{' && d == '{') {
                    depth += 1;
                } else if d == ')' || (c == '{' && d == '}' && depth > 0) {
                    depth -= 1;
                } else if d == close && depth == 0 {
                    break;
                }
                j += 1;
            }
            if j >= ch.len() {
                // unmatched: treat literally
                lit.push(c);
                i += 1;
                continue;
            }
            let inner: String = ch[i + 1..j].iter().collect();
            let sp = split_top(&inner, ',');
            let e = parse_expr(sp[0].trim(), defs)?;
            let (w, a) = width_align(&sp, defs)?;
            flush(&mut lit, &mut parts);
            parts.push(if c == '%' { FormPart::Str(e, w, a) } else { FormPart::Int(e, w, a) });
            i = j + 1;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    flush(&mut lit, &mut parts);
    Ok(Form { parts })
}
