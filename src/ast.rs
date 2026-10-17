//! Expression and instruction types. Function bodies are compiled into a
//! flat instruction list with resolved jump targets so the VM can run
//! GOTO/labels/loops without recursion.

use std::rc::Rc;

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Str(String),
    Form(Rc<Form>),
    Var(Rc<VarRef>),
    Call(String, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// ++X / X++ / --X / X--  (pre, delta)
    IncDec(Rc<VarRef>, bool, i64),
    /// Omitted argument (e.g. `FOO(1,,3)`).
    Empty,
}

#[derive(Debug, Clone)]
pub struct VarRef {
    pub name: String,
    pub idx: Vec<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Mul,
    Div,
    Mod,
    Add,
    Sub,
    Shl,
    Shr,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    And,
    Or,
    Nand,
    Nor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
pub enum FormPart {
    Lit(String),
    /// %expr,width,align%   (string interpolation; ints are stringified)
    Str(Expr, Option<Expr>, Align),
    /// {expr,width,align}
    Int(Expr, Option<Expr>, Align),
    /// \@ cond ? a # b \@
    Cond(Expr, Form, Form),
}

#[derive(Debug, Clone, Default)]
pub struct Form {
    pub parts: Vec<FormPart>,
}

impl Form {
    pub fn lit(s: &str) -> Form {
        Form { parts: vec![FormPart::Lit(s.to_string())] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AsgOp {
    Set,
    /// '=  (string expression assignment)
    SetExpr,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

/// How a PRINT-family command finishes its output.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrintFlags {
    pub newline: bool,
    pub wait: bool,
    /// K: force kana conversion (ignored), D: ignore SETCOLOR
    pub default_color: bool,
    /// C / LC: column-aligned (right / left)
    pub column: Option<Align>,
    pub single: bool,
    pub plain: bool,
    /// N: newline without wait in EE (treated like L)
    pub no_auto_button: bool,
}

#[derive(Debug, Clone)]
pub enum PrintContent {
    Raw(String),
    Form(Form),
    Expr(Expr),
    /// PRINTV: list of expressions concatenated
    Exprs(Vec<Expr>),
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    Static(String),
    Form(Form),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallKind {
    Call,
    Jump,
    CallF,
}

#[derive(Debug, Clone)]
pub enum CaseCond {
    Eq(Expr),
    Range(Expr, Expr),
    Is(BinOp, Expr),
}

#[derive(Debug, Clone)]
pub enum Args {
    None,
    Exprs(Vec<Expr>),
    Form(Form),
    Raw(String),
}

#[derive(Debug, Clone)]
pub enum Instr {
    Nop,
    /// a line that failed to parse: (message, fatal). Unknown commands are
    /// non-fatal (warned once and skipped), like other unimplemented commands.
    BadLine(Rc<str>, bool),
    Print(PrintContent, PrintFlags),
    /// PRINTDATA: pick one random entry; each entry may have several lines.
    PrintData(Vec<Vec<Form>>, PrintFlags),
    /// STRDATA var: pick one random entry into var
    StrData(Option<Rc<VarRef>>, Vec<Vec<Form>>),
    Assign(Rc<VarRef>, AsgOp, Vec<Expr>),
    /// Assignment whose RHS is a FORM string (string variable `=`/`+=`)
    AssignForm(Rc<VarRef>, AsgOp, Form),
    Eval(Expr),
    Jump(usize),
    JumpIfNot(Expr, usize),
    ForBegin { var: Rc<VarRef>, start: Expr, end: Expr, step: Option<Expr>, slot: usize, exit: usize },
    ForNext { var: Rc<VarRef>, slot: usize, body: usize, exit: usize },
    SelectEval(Expr, usize),
    CaseTest(usize, Vec<CaseCond>, usize),
    Call { target: CallTarget, args: Vec<Expr>, kind: CallKind, try_: bool, catch: Option<usize> },
    /// TRYCALLLIST etc: first existing function wins.
    CallList(Vec<(CallTarget, Vec<Expr>)>, CallKind),
    Goto { target: CallTarget, try_: bool, catch: Option<usize> },
    GotoList(Vec<CallTarget>),
    Return(Vec<Expr>),
    ReturnF(Option<Expr>),
    Begin(String),
    Restart,
    Quit,
    Throw(Form),
    Command(String, Args),
}

#[derive(Debug, Clone)]
pub struct DimDecl {
    pub name: String,
    pub is_str: bool,
    pub dims: Vec<Expr>,
    pub init: Vec<Expr>,
    pub is_const: bool,
    pub is_static: bool,
    pub is_global: bool,
    pub is_savedata: bool,
    pub is_chara: bool,
    pub is_ref: bool,
    pub is_dynamic: bool,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub target: Rc<VarRef>,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EventOrder {
    Pri,
    Normal,
    Later,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub code: Vec<Instr>,
    /// (file index, line number) per instruction
    pub lines: Vec<(u32, u32)>,
    pub labels: std::collections::HashMap<String, usize>,
    pub dims: Vec<DimDecl>,
    pub is_function: bool,
    pub returns_str: bool,
    pub order: EventOrder,
    pub single: bool,
    pub only: bool,
    pub n_slots: usize,
    pub file: u32,
    pub line: u32,
}
