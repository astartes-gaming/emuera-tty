//! `EMUERA_LINT=1` with `--check`: point at compiled statements that look
//! like the parser split their line in the wrong place (a function name
//! holding `(`, `,`, spaces or quotes can only come from that), to hunt
//! parser bugs across a whole game.

use crate::ast::*;
use crate::parser::Program;

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
    out.sort();
    out
}
