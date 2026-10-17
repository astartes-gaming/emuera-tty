//! The program representation shared by the parser and the VM.
//!
//! ERB source is compiled once at startup into these types:
//!
//! * [`Expr`] — expressions (`1 + X:2`, `@"text %NAME%"`, `FOO(1, 2)`).
//! * [`Form`] — Emuera "FORM strings": literal text with `%…%`, `{…}` and
//!   `\@ c ? a # b \@` holes, used by PRINTFORM, `@"…"` and string `=`.
//! * [`Instr`] — one statement. A function body is a *flat* `Vec<Instr>`:
//!   IF/loops/SELECTCASE/GOTO become jumps to instruction indices that the
//!   parser patches in, so the VM's main loop is just "run `code[pc]`,
//!   advance `pc`" and needs no recursion for control flow.
//! * [`Function`] — a compiled `@FUNCTION` with its parameters, private
//!   `#DIM`s, labels and per-instruction source positions (for errors).
//!
//! All identifiers are stored upper-cased (Emuera is case-insensitive).

// Arc (aliased as Rc): parsed programs are built on several threads
use std::sync::Arc as Rc;

/// A shared, immutable name: cloned into every resolved variable location,
/// so a clone must be a reference-count bump, not a string copy.
pub type Name = Rc<str>;

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i64),
    Str(String),
    /// `@"…"` or a `\@ … \@` ternary used as a value: evaluated to a string.
    Form(Rc<Form>),
    Var(Rc<VarRef>),
    /// User function or built-in; resolved at run time (user functions win).
    /// Boxed (with the width / ternary parts of [`FormPart`]) to keep
    /// `Expr` at 32 bytes: the parsed program holds millions of them.
    Call(Box<(String, Vec<Expr>)>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `c ? a # b` (Emuera uses `#` where C uses `:`).
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    /// ++X / X++ / --X / X--  (pre, delta)
    IncDec(Rc<VarRef>, bool, i64),
    /// Omitted argument (e.g. `FOO(1,,3)`).
    Empty,
}

/// A variable with its index chain: `CFLAG:TARGET:好感度` is
/// `name = "CFLAG"`, `idx = [TARGET, 好感度]`. A bare name used as an index
/// may be a CSV name (`好感度`), resolved against the variable's name table.
#[derive(Debug, Clone)]
pub struct VarRef {
    pub name: Name,
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
    /// `!&` — not (a && b)
    Nand,
    /// `!|` — not (a || b)
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
    Str(Expr, Option<Box<Expr>>, Align),
    /// {expr,width,align}
    Int(Expr, Option<Box<Expr>>, Align),
    /// \@ cond ? a # b \@
    Cond(Box<(Expr, Form, Form)>),
}

/// A FORM string: concatenation of its parts at evaluation time.
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

/// How a PRINT-family command finishes its output. The command name is
/// decoded once by the parser (`PRINTFORMDL` → form + default colour +
/// newline), so the VM only looks at these flags.
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

/// What a PRINT prints: raw text (`PRINT`), a FORM (`PRINTFORM`), one
/// expression (`PRINTS`) or several (`PRINTV`).
#[derive(Debug, Clone)]
pub enum PrintContent {
    Raw(String),
    Form(Form),
    Expr(Expr),
    /// PRINTV: list of expressions concatenated
    Exprs(Vec<Expr>),
}

/// The function / label named by CALL, JUMP, GOTO…: fixed, or built at run
/// time from a FORM (`CALLFORM EVENT_%NAME%`).
#[derive(Debug, Clone)]
pub enum CallTarget {
    Static(String),
    Form(Form),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CallKind {
    Call,
    /// JUMP: call, then return from the *calling* function too.
    Jump,
    /// CALLF: call a #FUNCTION, discarding its value.
    CallF,
}

/// One condition of a CASE line: `CASE 1`, `CASE 3 TO 5`, `CASE IS >= 10`.
#[derive(Debug, Clone)]
pub enum CaseCond {
    Eq(Expr),
    Range(Expr, Expr),
    Is(BinOp, Expr),
}

/// Arguments of a generic built-in command, in the form that command
/// expects: a comma list, a FORM string, or the raw rest of the line.
#[derive(Debug, Clone)]
pub enum Args {
    None,
    Exprs(Vec<Expr>),
    Form(Form),
    Raw(String),
}

/// One compiled statement. Jump targets (`usize`) are indices into the
/// owning function's `code`. "Slots" are per-call scratch cells (see
/// `vm::Slot`) holding loop bounds and the SELECTCASE value, so nested
/// constructs don't clash.
#[derive(Debug, Clone)]
pub enum Instr {
    /// a line that failed to parse: (message, fatal). Unknown commands are
    /// non-fatal (warned once and skipped), like other unimplemented commands.
    BadLine(Rc<str>, bool),
    Print(PrintContent, PrintFlags),
    /// PRINTDATA: pick one random entry; each entry may have several lines.
    PrintData(Vec<Vec<Form>>, PrintFlags),
    /// STRDATA var: pick one random entry into var
    StrData(Option<Rc<VarRef>>, Vec<Vec<Form>>),
    Assign(Rc<VarRef>, AsgOp, Vec<Expr>),
    /// Assignment whose RHS is a FORM string (string variable `=` only;
    /// `+=` takes an expression)
    AssignForm(Rc<VarRef>, AsgOp, Form),
    /// An expression evaluated for its side effects (`X++`, CALLF-style calls).
    Eval(Expr),
    Jump(usize),
    /// Jump when the condition is false (IF / SIF / WHILE tests).
    JumpIfNot(Expr, usize),
    /// FOR/REPEAT entry: set `var = start`, keep (end, step) in `slot`,
    /// and jump to `exit` if the loop runs zero times.
    ForBegin(Box<ForBegin>),
    /// NEXT/REND: `var += step`; jump back to `body` while still in range.
    ForNext { var: Rc<VarRef>, slot: usize, body: usize },
    /// SELECTCASE: evaluate once into `slot`.
    SelectEval(Expr, usize),
    /// CASE: test the value in `slot` against the conditions; on a miss
    /// jump to the next CASE (third field).
    CaseTest(usize, Vec<CaseCond>, usize),
    /// CALL / JUMP / CALLF and their TRY*/FORM variants. With `try_`, a
    /// missing function is not an error; `catch` is where TRYC* goes then.
    Call(Box<CallInstr>),
    /// TRYCALLLIST etc: first existing function wins.
    CallList(Vec<(CallTarget, Vec<Expr>)>, CallKind),
    /// GOTO / TRYGOTO …: (target, try_, catch) as for `Call`
    Goto(Box<(CallTarget, bool, Option<usize>)>),
    GotoList(Vec<CallTarget>),
    /// RETURN a, b, …: values go to RESULT:0, RESULT:1, …
    Return(Vec<Expr>),
    /// RETURNF x: the value of a #FUNCTION(S) call.
    ReturnF(Option<Expr>),
    /// BEGIN TRAIN / SHOP / …: unwind to the system loop and switch state.
    Begin(String),
    Restart,
    Quit,
    Throw(Form),
    /// Any other built-in command, dispatched by name in `vm::builtins`.
    Command(Box<(String, Args)>),
}

// The two largest instructions live behind a Box: every instruction is as
// big as the largest variant, and there are millions of them (AnonTW: 2.1M).

/// See [`Instr::ForBegin`].
#[derive(Debug, Clone)]
pub struct ForBegin {
    pub var: Rc<VarRef>,
    pub start: Expr,
    pub end: Expr,
    pub step: Option<Expr>,
    pub slot: usize,
    pub exit: usize,
}

/// See [`Instr::Call`].
#[derive(Debug, Clone)]
pub struct CallInstr {
    pub target: CallTarget,
    pub args: Vec<Expr>,
    pub kind: CallKind,
    pub try_: bool,
    pub catch: Option<usize>,
}

/// A `#DIM` / `#DIMS` declaration (global in .ERH, or private to a function).
#[derive(Debug, Clone)]
pub struct DimDecl {
    /// upper-cased (identifiers are case-insensitive)
    pub name: String,
    /// as written in the script: Emuera saves variables under this spelling
    pub decl_name: String,
    pub is_str: bool,
    /// Sizes per dimension; may name CONSTs, so evaluated at allocation.
    pub dims: Vec<Expr>,
    pub init: Vec<Expr>,
    pub is_const: bool,
    /// Private dims keep their value between calls unless DYNAMIC / REF
    /// (Emuera's default); see `Parser::parse_dim`.
    pub is_static: bool,
    pub is_global: bool,
    pub is_savedata: bool,
    pub is_chara: bool,
    /// `#DIM REF`: a parameter aliasing the caller's variable.
    pub is_ref: bool,
    pub is_dynamic: bool,
}

/// A function parameter: the variable it binds to (`ARG:1`, a private
/// dim…) and its optional default.
#[derive(Debug, Clone)]
pub struct Param {
    pub target: Rc<VarRef>,
    pub default: Option<Expr>,
}

/// Event function ordering (#PRI before normal before #LATER).
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
    /// `$LABEL` → instruction index, for GOTO.
    pub labels: crate::fxhash::HashMap<String, usize>,
    /// Private `#DIM`s declared in this function.
    pub dims: Vec<DimDecl>,
    /// `#FUNCTION` / `#FUNCTIONS`: callable inside expressions.
    pub is_function: bool,
    pub returns_str: bool,
    pub order: EventOrder,
    /// `#SINGLE`: stop running further handlers of this event.
    pub single: bool,
    /// `#ONLY`: this handler replaces all others of the event.
    pub only: bool,
    /// Number of [`Instr`] slots a call needs.
    pub n_slots: usize,
    pub file: u32,
    pub line: u32,
}

#[cfg(test)]
mod tests {
    /// The parsed program holds millions of these (AnonTW: 2.1M
    /// instructions): growing them costs hundreds of MB.
    #[test]
    fn node_sizes_stay_small() {
        assert!(std::mem::size_of::<super::Instr>() <= 48);
        assert!(std::mem::size_of::<super::Expr>() <= 32);
        assert!(std::mem::size_of::<super::FormPart>() <= 48);
    }
}
