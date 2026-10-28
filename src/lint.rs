//! `EMUERA_LINT=1` with `--check`: point at compiled statements that look
//! like the parser split their line in the wrong place (a function name
//! holding `(`, `,`, spaces or quotes can only come from that), to hunt
//! parser bugs across a whole game.
//!
//! It also lists every `>=` / `<=` between strings (and `CASE IS >=` /
//! `IS <=` in a string SELECTCASE). Emuera, since vanilla, evaluates both
//! as `<` (OperatorMethod's GreaterEqualStrStr / LessEqualStrStr test
//! `c < 0`; CaseExpression uses the same operators): this finds the games
//! that would change if that were fixed.

use crate::ast::*;
use crate::parser::Program;
use std::collections::{HashMap, HashSet};

/// What decides whether an expression is a string, as the parser saw it.
struct Types<'a> {
    prog: &'a Program,
    global_str: &'a HashSet<&'a str>,
    /// the function's own `#DIM` / `#DIMS` (they hide globals)
    local: HashMap<&'a str, bool>,
}

impl Types<'_> {
    fn var_is_str(&self, name: &str) -> bool {
        match self.local.get(name) {
            Some(&s) => s,
            None => crate::parser::is_system_str_var(name) || self.global_str.contains(name),
        }
    }

    fn is_str(&self, e: &Expr) -> bool {
        match e {
            Expr::Str(_) | Expr::Form(_) => true,
            Expr::Var(v) => self.var_is_str(&v.name),
            Expr::Call(c) => {
                let n = c.0.to_uppercase();
                // the game's functions win over built-ins, as at run time
                match self.prog.funcs.get(&n) {
                    Some(f) => f[0].is_function && f[0].returns_str,
                    None => crate::vm::builtins::returns_str(&n),
                }
            }
            Expr::Binary(BinOp::Add | BinOp::Mul, a, b) => self.is_str(a) || self.is_str(b),
            Expr::Ternary(_, a, _) => self.is_str(a),
            _ => false,
        }
    }

    /// `>=` / `<=` between strings anywhere in `e`.
    fn expr(&self, e: &Expr, out: &mut Vec<String>) {
        match e {
            Expr::Binary(op, a, b) => {
                if matches!(op, BinOp::Ge | BinOp::Le) && self.is_str(a) && self.is_str(b) {
                    out.push(format!("string {} (Emuera evaluates it as <)", if *op == BinOp::Ge { ">=" } else { "<=" }));
                }
                self.expr(a, out);
                self.expr(b, out);
            }
            Expr::Unary(_, a) => self.expr(a, out),
            Expr::Ternary(c, a, b) => [c, a, b].iter().for_each(|x| self.expr(x, out)),
            Expr::Call(c) => c.1.iter().for_each(|x| self.expr(x, out)),
            Expr::Var(v) | Expr::IncDec(v, ..) => v.idx.iter().for_each(|x| self.expr(x, out)),
            Expr::Form(f) => self.form(f, out),
            Expr::Int(_) | Expr::Str(_) | Expr::Empty => {}
        }
    }

    fn form(&self, f: &Form, out: &mut Vec<String>) {
        for p in &f.parts {
            match p {
                FormPart::Lit(_) => {}
                FormPart::Str(e, w, _) | FormPart::Int(e, w, _) => {
                    self.expr(e, out);
                    if let Some(w) = w {
                        self.expr(w, out);
                    }
                }
                FormPart::Cond(c) => {
                    self.expr(&c.0, out);
                    self.form(&c.1, out);
                    self.form(&c.2, out);
                }
            }
        }
    }

    fn target(&self, t: &CallTarget, out: &mut Vec<String>) {
        if let CallTarget::Form(f) = t {
            self.form(f, out);
        }
    }

    fn var(&self, v: &VarRef, out: &mut Vec<String>) {
        v.idx.iter().for_each(|x| self.expr(x, out));
    }
}

/// Every string `>=` / `<=`, "file:line: @FUNC: …".
fn string_compares(prog: &Program, out: &mut Vec<String>) {
    let global_str: HashSet<&str> = prog.global_dims.iter().filter(|d| d.is_str).map(|d| d.name.as_str()).collect();
    for f in prog.funcs.values().flatten() {
        let body = f.body();
        let t = Types { prog, global_str: &global_str, local: body.dims.iter().map(|d| (d.name.as_str(), d.is_str)).collect() };
        let mut found = Vec::new();
        let at = |pc: usize| body.lines.get(pc).copied().unwrap_or((f.file, f.line));
        for p in &f.params {
            t.var(&p.target, &mut found);
            if let Some(d) = &p.default {
                t.expr(d, &mut found);
            }
        }
        for d in &body.dims {
            d.dims.iter().chain(&d.init).for_each(|x| t.expr(x, &mut found));
        }
        let mut hits: Vec<((u32, u32), String)> = found.drain(..).map(|m| ((f.file, f.line), m)).collect();
        // SELECTCASE slot → whether it selects on a string
        let mut select_str = HashMap::new();
        for (pc, ins) in body.code.iter().enumerate() {
            let exprs = |es: &[Expr], found: &mut Vec<String>| es.iter().for_each(|x| t.expr(x, found));
            match ins {
                Instr::Print(PrintContent::Form(fm), _) | Instr::Throw(fm) => t.form(fm, &mut found),
                Instr::Print(PrintContent::Expr(e), _) | Instr::Eval(e) | Instr::JumpIfNot(e, _) | Instr::ReturnF(Some(e)) => t.expr(e, &mut found),
                Instr::Print(PrintContent::Exprs(es), _) | Instr::Return(es) => exprs(es, &mut found),
                Instr::PrintData(fs, _) => fs.iter().flatten().for_each(|fm| t.form(fm, &mut found)),
                Instr::StrData(v, fs) => {
                    if let Some(v) = v {
                        t.var(v, &mut found);
                    }
                    fs.iter().flatten().for_each(|fm| t.form(fm, &mut found));
                }
                Instr::Assign(v, _, es) => {
                    t.var(v, &mut found);
                    exprs(es, &mut found);
                }
                Instr::AssignForm(v, _, fm) => {
                    t.var(v, &mut found);
                    t.form(fm, &mut found);
                }
                Instr::ForBegin(fb) => {
                    t.var(&fb.var, &mut found);
                    [&fb.start, &fb.end].into_iter().chain(&fb.step).for_each(|x| t.expr(x, &mut found));
                }
                Instr::ForNext { var, .. } => t.var(var, &mut found),
                Instr::SelectEval(e, slot, _) => {
                    t.expr(e, &mut found);
                    select_str.insert(*slot, t.is_str(e));
                }
                Instr::CaseTest(slot, conds, _) => {
                    for c in conds {
                        match c {
                            CaseCond::Eq(e) => t.expr(e, &mut found),
                            CaseCond::Range(a, b) => {
                                t.expr(a, &mut found);
                                t.expr(b, &mut found);
                            }
                            CaseCond::Is(op, e) => {
                                if matches!(op, BinOp::Ge | BinOp::Le) && select_str.get(slot) == Some(&true) {
                                    found.push(format!("CASE IS {} on a string (Emuera evaluates it as <)", if *op == BinOp::Ge { ">=" } else { "<=" }));
                                }
                                t.expr(e, &mut found);
                            }
                        }
                    }
                }
                Instr::Call(c) => {
                    t.target(&c.target, &mut found);
                    exprs(&c.args, &mut found);
                }
                Instr::CallList(es, _) => es.iter().for_each(|(tg, a)| {
                    t.target(tg, &mut found);
                    exprs(a, &mut found);
                }),
                Instr::Goto(g) => t.target(&g.0, &mut found),
                Instr::GotoList(ts) => ts.iter().for_each(|tg| t.target(tg, &mut found)),
                Instr::Command(c) => match &c.1 {
                    Args::Exprs(es) => exprs(es, &mut found),
                    Args::Form(fm) => t.form(fm, &mut found),
                    Args::None | Args::Raw(_) => {}
                },
                Instr::Print(PrintContent::Raw(_), _)
                | Instr::ReturnF(None)
                | Instr::BadLine(..)
                | Instr::Jump(_)
                | Instr::End
                | Instr::Begin(_)
                | Instr::Restart
                | Instr::Quit => {}
            }
            hits.extend(found.drain(..).map(|m| (at(pc), m)));
        }
        for ((file, line), m) in hits {
            out.push(format!("{}:{}: @{}: {}", prog.files.get(file as usize).map_or("?", |s| s.as_str()), line, f.name, m));
        }
    }
}

/// Text a function / label name can't hold.
fn bad_name(s: &str) -> bool {
    s.chars().any(|c| !crate::expr::is_ident_char(c) && c != '@')
}

fn check_target(t: &CallTarget, what: &str, out: &mut Vec<String>) {
    match t {
        CallTarget::Static(s) if bad_name(s) => out.push(format!("{} name `{}`", what, s)),
        CallTarget::Form(f) => {
            for p in &f.parts {
                if let FormPart::Lit(s) = p {
                    if s.chars().any(|c| matches!(c, '(' | ')' | ',' | '"' | ' ' | '\t' | '%')) {
                        out.push(format!("{} name part `{}`", what, s));
                    }
                }
            }
        }
        _ => {}
    }
}

/// FORM text that still holds `%`, `{` or `}`: a hole the parser could
/// not close (kept as text).
fn check_form(f: &Form, out: &mut Vec<String>) {
    for p in &f.parts {
        match p {
            FormPart::Lit(s) if s.contains(['%', '{', '}']) => out.push(format!("FORM text `{}`", s)),
            FormPart::Cond(c) => {
                check_form(&c.1, out);
                check_form(&c.2, out);
            }
            _ => {}
        }
    }
}

pub fn lint(prog: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for f in prog.funcs.values().flatten() {
        let body = f.body();
        for (pc, ins) in body.code.iter().enumerate() {
            let mut found = Vec::new();
            match ins {
                Instr::Call(c) => check_target(&c.target, "call", &mut found),
                Instr::CallList(es, _) => es.iter().for_each(|(t, _)| check_target(t, "call list", &mut found)),
                Instr::Goto(g) => check_target(&g.0, "goto", &mut found),
                Instr::GotoList(ts) => ts.iter().for_each(|t| check_target(t, "goto list", &mut found)),
                Instr::Print(PrintContent::Form(f), _) | Instr::AssignForm(_, _, f) | Instr::Throw(f) => check_form(f, &mut found),
                Instr::PrintData(es, _) | Instr::StrData(_, es) => es.iter().flatten().for_each(|f| check_form(f, &mut found)),
                Instr::Command(c) => {
                    if let Args::Form(f) = &c.1 {
                        check_form(f, &mut found);
                    }
                }
                _ => {}
            }
            for m in found {
                let (file, line) = body.lines.get(pc).copied().unwrap_or((f.file, f.line));
                out.push(format!("{}:{}: @{}: {}", prog.files.get(file as usize).map_or("?", |s| s.as_str()), line, f.name, m));
            }
        }
    }
    string_compares(prog, &mut out);
    // by file, then line number (not as text: 5 before 10)
    out.sort_by_cached_key(|l| {
        let mut it = l.splitn(3, ':');
        let file = it.next().unwrap_or("").to_string();
        (file, it.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0), l.clone())
    });
    // several on one line: the line once
    out.dedup();
    out
}
