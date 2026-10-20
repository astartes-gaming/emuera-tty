# Design

## Context

See proposal.md (Why) and `doc/anchor.md` (Anchor's behaviour, with source references). What emuera-tty has today:

- `src/translate.rs` `Tr`: table → original → translation, loaded from `*_TR.csv` under the CSV folder. `Tr::name` falls back to ALL. `apply_trchara` patches character templates.
- `Vm::eval_form_into(form, out, tr)` (`src/vm/mod.rs`): `tr` is true only for PRINT / PRINTDATA / HTML_PRINT FORMs. `Vm::translated` looks up a `%…%` value in its own table if it is an `XXXNAME` variable, else in ALL. `PrintContent::Expr` (PRINTS) goes through `translated`, so any value gets an ALL lookup. PRINTV gets nothing.
- Function bodies compile lazily on first call. `Body` keeps `(file, line)` per instruction, but not the line text.
- Every translation path is skipped when `csv.tr.is_empty()`.

Anchor's model is a boolean passed into every `GetStrValue`. emuera-tty evaluates through `&mut self` methods on `Vm`, which return `Value`. Adding a parameter to all of them would touch hundreds of call sites in `vm/mod.rs` and `vm/builtins.rs`.

## Goals / Non-Goals

**Goals:**
- The observable behaviour in the three specs, with no change at all for games without TR / TRCHARA / PARAM files.
- No measurable slowdown in untranslated games (the interpreter's hot loop gets at most one predictable branch per statement).

**Non-Goals:**
- Reproducing Anchor's internal order of evaluation where it doesn't change output (e.g. when constant folding happens).
- Anchor's UI: dialogs, debug window, custom icon.
- Translating text that Anchor leaves alone, apart from PRINT_ABL / TALENT / MARK / EXP, which already translate here and stay that way (D7).

## Decisions

### D1. The flag is VM state, not a parameter

Two fields on `Vm`:
- `tr_run: bool`: Anchor's `runScriptProc(translate)`. It is true while the engine runs statements (system flow, CALL, JUMP, events). When a `#FUNCTION(S)` is called *inside an expression*, it is saved and set to the caller's `tr_eval` for that call, then restored.
- `tr_eval: bool`: the flag for the expression being evaluated now. Each statement sets it to `tr_run && statement passes the flag` (PRINT family except raw text, PRINTDATA, HTML_PRINT, string Assign / AssignForm, ReturnF). PRINTPLAIN sets it to true. It is reset to false after the statement.
- Built-in functions that pass the flag on (TOUPPER, TOLOWER, TOHALF, TOFULL, REPLACE, STRFORM, and the `+` and string-repeat operators) leave `tr_eval` as it is for their arguments. Every other built-in clears it while evaluating its arguments and restores it afterwards. The `? #` ternary clears it for its branches.

*Alternative considered:* a `tr: bool` parameter on `eval` / `eval_str` / `eval_form_into` and through `builtin_fn`. It is closer to Anchor's code, but it changes hundreds of call sites and every future builtin has to remember to pass it. The save/restore pattern keeps the decision in a few places: statement dispatch, `builtin_fn` entry, and `#FUNCTION` calls.

When `csv.tr.is_empty()`, `tr_run` is never set, so every check is one false branch.

### D2. Reads check the flag at the variable

Variable reads (`Vm::eval` on `Expr::Var`) translate when `tr_eval` is on and the variable is a CSV name array (own table, via `csvdata::table_for_var`), or one of STR / TSTR / SAVESTR / RESULTS / GLOBALS (the Str table). With the flag off and a translation existing, the pair is pushed to the memory (D4). `Tr::name`'s fallback to ALL is removed. ALL is used only by the percent fallbacks, literals and STRLENS.

This drops one thing emuera-tty does today that Anchor doesn't: an ALL lookup on any PRINTS value (`PRINTS LOCALS`). Anchor only looks up literals there (D3).

### D3. Per-line facts are computed when the body compiles

Anchor inspects the current line's *text* in two places: the table for literals (`searchParentFile`) and the RETURNF check. The parser computes both from the raw line while compiling a body, and stores them as one byte per instruction in a new `Body::tr_line: Vec<u8>` (a table index for literals, plus a "line contains RETURNF" bit). This happens only when the game has TR files, which the parser is told when it is created; for other games the vec stays empty. Literal and constant-folded reads use the current instruction's byte.

*Alternative:* keeping line text in `Body`. It costs far more memory (the README's Memory section budgets compiled code), and only these two bits are ever needed.

### D4. The memory and last-translation state live in `Vm`

`Tr` stays immutable shared data. `Vm` gets `tr_memory: VecDeque<(Rc<str>, u8)>` (20 entries, newest at the back) and `tr_last: Option<Rc<str>>`. They are cleared on restart and on reload, and not saved, matching Anchor's static fields (reset only when the program restarts).

### D5. Constant names in builtin arguments

Anchor folds constants with translation when it first builds a call. emuera-tty keeps folding as it is and does the equivalent at run time: when `builtin_fn` evaluates an argument that is a CSV name variable with all-literal indices, it reads it translated whatever the flag (own table only). A `%…%` whose expression is such a constant, or a literal, is marked constant at parse (a bit on `FormPart::Str`), and gets only its own translation, with no percent fallbacks. This gives the same output without moving folding earlier.

### D6. Lenient, warned TR loading

As decided with the maintainer: malformed lines and duplicate keys become load warnings (first definition kept), shown like other load warnings, and the game starts. Values are no longer trimmed (Anchor doesn't trim). Lines are read as Anchor's EraStreamReader reads them, so files tested in Anchor read the same: split where .NET's ReadLine splits (`\r\n`, `\n`, a lone `\r`), white space (space, tab, U+3000) before the original dropped, `{` … `}` blocks joined into one line; nothing else is trimmed. One deliberate difference: a stray byte-order mark at a line's start is dropped too, where Anchor would keep it in a key that then never matches. TRCHARA replaces only the fields a block gives; Anchor's blanking is not reproduced.

### D7. PRINT_ABL & co. stay translated

Anchor prints those lists from the raw CSV names. emuera-tty translates them already, the output is only for display, and turning it off would make translated games look worse. The spec records this as an intended difference.

### D8. PARAM renaming happens in the parser, gated

`PARAM`→`PALAM` is applied to identifier tokens in `parser.rs` / `expr.rs` when the game has PARAM.CSV or any TR file, and to VARSIZE's literal argument. Anchor renames in every game; gating keeps other games byte-identical, which the project requires. `csvdata.rs` loads `PARAM.CSV` in place of `PALAM.CSV` and maps `Param_TR` onto the PALAM table only in that case. Otherwise `Palam_TR` applies, and `Param_TR` is ignored, as in Anchor.

### D9. Every string assignment form is flagged

Anchor's SET builder (`GameProc/Function/ArgumentBuilder.cs`, SP_SET) turns a string `+=` / `*=` into `X = X + rhs` / `X = X * rhs`, and `=` into a FORM term. All of them are then evaluated by `SET_Instruction` with the flag. So in emuera-tty, `Instr::Assign` (any `AsgOp` on a string target) and `Instr::AssignForm` both set `tr_eval` for their right-hand side, and the `+` / repeat passes it on. Integer assignments are unaffected (they hold no names).

## Risks / Trade-offs

- [Translated SET values change script logic] Anchor games compare translated text (they were tested that way), but a game partly translated with TR files and partly not could compare against the original. → It matches Anchor, which is the reference these games were tested on. `EMUERA_TRACE` shows the stored values when debugging.
- [Forgetting to clear `tr_eval` in a new builtin] D1 clears it at `builtin_fn` entry by default; only the allow-list passes it on, so a new builtin is safe without doing anything.
- [Error unwinding leaves the flag set] `BEGIN` / `QUIT` / errors unwind as `Err(Ctrl)`. → Statement dispatch sets the flags at the start of every statement, rather than relying only on restoring them. The `#FUNCTION` call restores `tr_run` on both `Ok` and `Err`.
- [Memory heuristic mistranslates] Anchor's substring replacement can hit unrelated text. → It's Anchor's behaviour and translators tuned their files to it; it only runs in translated games.
- [Saves differ from older emuera-tty saves of the same game] Values saved by earlier emuera-tty versions are the originals. → Both load fine; only newly assigned values are translated. The release notes should say so.
- [The specs come from reading Anchor's source, not from running it] Emuera can't run here (no .NET or Wine; see CLAUDE.md), and Anchor is an Emuera build, so the fixture outputs can't be compared against a live Anchor. → Every behaviour in the specs is traced to a file and line in `doc/anchor.md`, and the tests check those scenarios. Where possible, files Anchor wrote serve as the comparison, as the project does for Emuera: a save Anchor made in a translated game shows which values it stored translated (task 10.3).
- [Lazy compile, D3] `tr_line` is computed only for compiled bodies, and is rebuilt with them after eviction. No extra state is kept for uncompiled functions.

## Migration Plan

No data migration. All behaviour is gated on the game's files. Rolling back means reverting the change; saves made in between stay loadable either way (they hold ordinary strings).