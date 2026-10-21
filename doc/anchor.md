# Emuera-Anchor: what it adds, and where emuera-tty stands

Emuera-Anchor (source: `../emuEra`, https://gitgud.io/Bepsi/emuEra, last
commit 2018-07-13, "Anchor V1.3b") is vanilla Emuera 1821 plus a
translation system for English fan translations. This file records what
Anchor does that Emuera-EM/EE do not, read from its source, and what
emuera-tty does about each. Paths are under `../emuEra/Emuera/`.

Anchor's clipboard feature, the Ctrl-T / Ctrl-R / Ctrl-O hotkeys and the
English clipboard config keys were later taken into Emuera-EE, so they
are not Anchor-only; they are listed at the end because emuera-tty still
lacks parts of them.

## 1. The `*_TR.csv` name tables

`GameData/Variable/Translation.cs` `loadTranslationData` (l. 214).

- One file per CSV name table, read from the **CSV folder itself** (not
  subfolders): `Abl Talent Exp Mark Palam Param Item Train Base Source Ex
  EQUIP TEQUIP Flag TFLAG Cflag Tcvar CSTR Stain Cdflag1 Cdflag2 Str TSTR
  SaveStr GLOBAL GLOBALS ALL` + `_TR.csv`.
- Lines come from `EraStreamReader.ReadEnabledLine` (`Sub/EraStreamReader.cs:62`):
  split by .NET's ReadLine (`\r\n`, `\n` or a lone `\r`), blank lines
  skipped, white space (space, tab, U+3000) before the text dropped, and
  the lines from a `{` line to a `}` line joined, a space after each. A
  `{` line holding anything else, a stray `}`, a `{` inside a block or a
  missing `}` is an error.
- A line is `original,translation`. Text after `;` is a comment, and a
  line starting with `;` is skipped. Nothing else is trimmed.
- A line without exactly one comma, or a key given twice, is an
  exception: **the game does not start**.
- A `_TR.csv` for any other table is ignored. pops-tw ships
  `Juel_TR.csv`, which Anchor never reads (Juel isn't in the list).

emuera-tty (`src/translate.rs` `enabled_lines`, `Tr::load`): the same,
except that every error above is a load warning naming the file and line,
the line or block is skipped (the first of two duplicate keys is kept),
and the game starts; and a stray byte-order mark at a line's start is
dropped. See *Deliberate differences* below.

## 2. When a name is translated: the translate flag

Anchor threads a `translate` / `tryTranslate` boolean through every
`GetStrValue` call. A CSV name comes out translated only when this flag
is true at the point where it is read.

**Where the flag starts as true.** The main script loop runs with it on:
`GameProc/Process.cs:252` calls `runScriptProc(true)`, so every
statement run by the engine (including those in `CALL`ed functions)
gets `translate = true`. Only these instructions pass it on to their
arguments (`GameProc/Function/Instraction.Child.cs`):

| Instruction | line | note |
|---|---|---|
| PRINT family (PRINT, PRINTS, PRINTV, PRINTFORM, PRINTFORMS, all K/D/L/W/C/LC variants) | 136–149 | PRINTFORMS translates both the expression and the FORM it yields |
| HTML_PRINT | 264 | |
| SET on a string: `=` (FORM text), `'=`, `'= a, b, c`, `+=` / `*=` (built as `X = X + rhs`, `ArgumentBuilder.cs` SP_SET) | 449, 467 | **the stored value is the translated one** |
| RETURNF | 2178 | |

Some statements are handled in `Process.ScriptProc.cs` instead. Of
those, **PRINTPLAIN / PRINTPLAINFORM always translate** (l. 146) and
PRINTBUTTON does not. Anything else (conditions, CALL arguments, most
commands) evaluates with the flag off.

**Where the flag is passed on inside an expression.**

- String `+` (`GameData/Expression/OperatorMethod.cs:229`) and string ×
  integer repetition (l. 275–279). The ternary `? #` does **not** pass
  it on (l. 809).
- `%…%` and `{…}` in FORM strings (`GameData/StrForm.cs:204`).
- `TOUPPER TOLOWER TOHALF TOFULL` (`Creator.Method.cs:2256`), `REPLACE`
  (all three arguments, l. 2307) and `STRFORM` (l. 2514). No other
  built-in function passes it on.
- A `#FUNCTION(S)` call: its body runs with the caller's flag
  (`Process.cs:312` `GetValue(…, translate)` → `runScriptProc(translate)`).

**What a flagged read translates.**

- `XXXNAME:i` (ABLNAME, TALENTNAME… `VariableData.cs:253–278`) through
  its own table only, with no fallback to ALL (`VariableToken.cs:1406`).
- The values of **STR, TSTR, SAVESTR, RESULTS and GLOBALS** through
  `Str_TR.csv` (`VariableToken.cs:848`; `VariableData.cs:199–203`
  register all five with the same token class). This is probably
  unintended for RESULTS, but it is what Anchor does.
- A string **literal / constant-folded value** (`Expression/Term.cs:58`)
  through the table named by the *current line's text*. If the line
  contains `ABLNAME`, `TALENTNAME`… (`searchParentFile`,
  `Translation.cs:275`) that table is used, else ALL.
- PALAM reads use `Param` instead of `Palam` when the game has
  `PARAM.CSV` (§5).

PRINTDATA is not in the list: `PRINT_DATA_Instruction` prints each DATA
line with `term.GetStrValue(exm)`, without the flag.

emuera-tty (`src/vm/mod.rs`: `Vm::tr_run` / `tr_eval`, `Vm::tr_read`,
`Vm::tr_const`, `builtin_fn`'s wrapper in `vm/builtins.rs`): the flag as
above, for all of these statements and expressions, literals and
constants included (§4). PRINTDATA translates too (see *Deliberate
differences*). *Gap: the memory (§3) and PALAM→Param (§5).*

## 3. The `%…%` fallbacks: ALL_TR and the "memory"

`StrForm.cs:256` `FormatPercent`. This runs for a `%…%` value when the
flag is on, **or when the `%…%` has a width** (`%X,10%` turns
translation on even with the flag off). It is skipped when the current
line's text contains `RETURNF`. The steps:

1. The value itself, evaluated with the flag (so `%TALENTNAME:5%` is
   already translated through Talent_TR).
2. Then the whole result is looked up in `ALL_TR.csv`.
3. If that didn't change it, `translateALL` (`Translation.cs:131`) runs:
   - if the result contains the last successful translation, stop;
   - else check the **memory**, newest first. This is the last 20
     `(original, table)` pairs that were *looked up with the flag off
     and had a translation* (`translate`, l. 78–103). The first
     original found inside the result is replaced by its translation.
     This is how `LOCALS '= TALENTNAME:5` (inside a condition or a
     function, flag off) and a later `PRINTFORM [%LOCALS%]` still print
     the translation;
   - else, if ALL_TR exists, take the result with all digits, `【 】 [ ]
     『 』` and `LV` removed. If that remains a key in ALL_TR, replace that
     substring of the original result with its translation.
4. Width padding is applied to the translated text, counting by display
   width.

Details from the source: the memory is added to and then trimmed once
it holds 20 (`if (Count >= 20) RemoveAt(0)`), so 19 pairs stay; besides
names and STR-like values, literals and folded constants read without
the flag are remembered too (`Term.cs` SingleTerm, under the line's
table), as are constants folded at load. `lastRet` is set by every
successful flagged translation (names, literals, the ALL lookup). The
memory replacement and the stripped lookup replace every occurrence
(`String.Replace`); digits are .NET's `\d`, any Unicode digit.

emuera-tty (`Vm::tr_percent`, `Vm::tr_remember`): all of it, with the
differences listed under *Deliberate differences*.

## 4. Constants translated at first parse

`GameData/Function/FunctionMethodTerm.cs:46`: when a built-in function
call is built, each argument is constant-folded **with translate =
true**. So a CSV name with a constant index, given as an argument to
*any* built-in function, is replaced by its translation. Examples:
`STRLENS(TALENTNAME:5)`, `SUBSTRING(ABLNAME:2, 0, 4)`, and every
`%TALENTNAME:5%` (FORM `%…%` is itself a function term). A variable
index (`TALENTNAME:LOCAL`) is not folded, and is read with the flag at
run time.

Operators are function terms too (`OperatorMethod.cs` `ReduceBinaryTerm`
builds a `FunctionMethodTerm`), so the same goes for every operator:
`IF TALENTNAME:5 == "処女"` compares "Virgin" and is false, where
`IF TALENTNAME:LOCAL == "処女"` is true. A constant that is a statement's
whole argument is folded *without* the flag (`checkArgumentType` →
`Restructure(exm)`), and some statements then use it as folded, never
translating it (`ConstStr`): assignments (`ArgumentBuilder.cs` SP_SET),
plain PRINT and PRINTFORM (`IsConst`), and RETURNF (`SingleTerm.GetValue`
returns itself). PRINTS, PRINTV, HTML_PRINT and PRINTPLAIN read theirs
with the flag, which looks the folded text up by the line (§2).

`STRLENS` / `STRLENSU` (`Creator.Method.cs:1947`) also evaluate their
argument with the flag on and look the result up in ALL_TR, so they
measure the translated text.

emuera-tty (`src/translate.rs`, the *constants* section): the parser
marks each constant that folding changes, and the VM evaluates it as
Anchor folded it. *Gap: STRLENS (task 6.2).*

## 5. `PARAM.CSV`

`GameData/ConstantData.cs:600–608`. If `CSV/PARAM.CSV` exists it is
loaded **instead of** `PALAM.CSV`, and `Param_TR.csv` replaces
`Palam_TR.csv`. Separately, and **in every game whether or not it has
PARAM.CSV**, every identifier has `PARAM` replaced by `PALAM` before it
is resolved (`Expression/ExpressionParser.cs:256`). This is a plain
substring replace, so `PARAMLV`, `UP_PARAM` and a user variable
`MYPARAM` are renamed too. VARSIZE's `"PARAM"` string argument is
renamed as well (`Creator.Method.cs:355`). `TR_NAME("PARAM", …)` uses
Param_TR (`TRCommands.cs`).

*Gap in emuera-tty: all of it (emuera-tty only maps `Param_TR.csv` onto
the PALAM table).*

## 6. `TRCHARA*.csv`

`Translation.cs:307` (loading) and `ConstantData.cs:1143–1155, 1277`
(applying).

- Every `TRCHARA*.CSV` under the CSV folder (case-insensitive on
  Windows).
- Blocks run `NO,n` / `番号,n` … `ENDCHR` / `ENDCHAR`. Inside a block:
  `NAME`/`名前`, `CALLNAME`/`呼び名`, `NICKNAME`/`あだ名`,
  `MASTERNAME`/`主人の呼び方` take the value after the first comma;
  `CSTR,i,text` takes the third field.
- Applied while the character CSVs are read: each field the *original*
  `Chara*.csv` sets is replaced. Quirk: if the character has a TRCHARA
  block, NAME / CALLNAME / NICKNAME / MASTERNAME are replaced **even when
  the block doesn't give them** (with ""). CSTR is replaced only for
  indices the block gives.
- Warnings (not errors) for a missing comma, a second NO in a block,
  data before NO, unknown keys, and a CSTR line without a third field.

emuera-tty (`translate.rs` `apply_trchara`): applies the fields given.
*Gaps: `ENDCHAR`, case-insensitive file names and keys, the warnings;
see D5 for the quirk.*

## 7. `TR_NAME(code, index | name)`

`GameData/Function/TRCommands.cs:16`. The code is a string or a bare
variable. Accepted: `ABL BASE CDFLAG1 CDFLAG2 CFLAG CSTR EQUIP EX EXP
FLAG GLOBAL GLOBALS ITEM MARK PALAM/PARAM SAVESTR SOURCE STAIN STR
TALENT TCVAR TEQUIP TFLAG TRAIN TSTR`, each also with a `NAME` suffix.
With a string key the key must be defined in that CSV; with an integer
the index must have a name. Anything else is a **script error**
("Invalid first/second argument"). It always translates (flag on).

emuera-tty: implemented, but an unknown code / key / index returns the
original or "" instead of an error. *Gap: the errors.*

## 8. Engine-printed lists

| What | Anchor | emuera-tty |
|---|---|---|
| TRAIN command list | Train_TR (`Process.SystemProc.cs:311`) | yes |
| PRINT_SHOPITEM / shop | Item_TR (`Process.ScriptProc.cs:230`) | yes |
| PRINT_ITEM | Item_TR (`VariableEvaluator.cs:869`) | yes |
| PRINT_PALAM | Palam/Param_TR (`VariableEvaluator.cs:992`) | yes |
| PRINT_ABL / TALENT / MARK / EXP | **not translated** | translated (kept, see D7) |

## 9. Other Anchor-only things

- `Anchor- Path to a custom window icon (PNG)` config key. Means nothing
  in a terminal; emuera-tty should keep it when it rewrites
  `emuera.config` and otherwise ignore it.
- English UI and error messages, and a translated debug window. These
  are Windows UI; emuera-tty's own messages are English already.
- Number parsing that ignores the Windows locale. A .NET bug; it doesn't
  apply to Rust.

## Deliberate differences

Where emuera-tty does something other than Anchor on purpose, and why.
The rule: copy Anchor where not doing so would make a game not work, or
not work as expected; differ to fix a bug or do the right thing, as long
as saves stay compatible both ways and nothing the games rely on in
Anchor breaks. Differences from Emuera-EM/EE are in
[differences.md](differences.md).

| What | Anchor | emuera-tty | Why |
|---|---|---|---|
| Games without `*_TR.csv` / `TRCHARA*.csv` | the translation machinery always runs (the translate flag, the memory, `PARAM`→`PALAM` renaming) | none of it runs (`Tr::active`) | Such games then behave exactly as in Emuera-EM/EE, output and saves byte for byte (design D8) |
| A malformed TR line (not one comma, misplaced `{` / `}`) or a key given twice (§1) | the game does not start | a load warning with file and line; the line or block is skipped, the first key kept | One bad line costs one translation instead of the whole game (design D6) |
| A byte-order mark at the start of a line other than the first (§1) | kept, as part of the key | dropped | A key starting with an invisible BOM can never match: files pasted together would silently lose a translation |
| A TRCHARA block without NAME, CALLNAME, NICKNAME or MASTERNAME (§6) | that field is set to "" | the CSV value is kept | Anchor's blanking erases names the translator didn't mean to touch (design D6) |
| PRINT_ABL / TALENT / MARK / EXP (§8) | the CSV names, untranslated | translated | Display only: a translated game looks translated, and scripts never see these strings (design D7) |
| PRINTDATA (§2) | DATA lines printed without the flag | with the flag, as PRINTFORM | Display only, like D7; emuera-tty translated it before Anchor parity work started, and turning it off would make translated games look worse |
| A literal read with the flag (§2, `Term.cs` SingleTerm) | the literal object is overwritten with its translation: later reads, flagged or not, see the translated text, and a second flagged read can translate it again | each read looks the literal up afresh | Anchor's overwrite is a bug: the same line gives different text the second time it runs |
| The text searched for the line's table and RETURNF (§2, §3) | `Filename:LineNo:RowLine`, so the file name counts too | the line only | A file name containing `TALENTNAME` or `RETURNF` is an accident, not something a translation relies on |
| The memory and lastRet at game start (§3) | already hold pairs from the constants folded while the scripts loaded | start empty | emuera-tty compiles function bodies when they first run (to fit in 512 MB), so there is no load-time folding to replay; only the first percent fallbacks of a session can differ |
| Digits removed by the stripped lookup (§3) | any Unicode digit (`\d`) | ASCII and full-width digits | The only digits era games use; Rust has no Unicode digit class without an extra crate |
| RETURNF lines (§3) | any line whose text contains `RETURNF` skips the percent fallbacks | the same, but the line is only searched when it has a statement that reads with the flag (PRINT, assignments, RETURNF, HTML_PRINT) | Searching every line cost about 5% of a full compile; a width `%…%` in another statement on a line that also contains RETURNF is the only case that differs |
| `PARAM`→`PALAM` renaming (§5) | in every game | only in games with `PARAM.CSV` or TR files | Keeps every other game byte-identical; games that need the renaming have one of those (design D8) |

## Shared with Emuera-EE, partly missing in emuera-tty

Not Anchor-only and not part of the translation plan, but these came from
Anchor and players coming from it will expect them:

| Anchor / EE | emuera-tty |
|---|---|
| Clipboard copy (`CB*` config) | yes (OSC 52) |
| Ctrl-T title, Ctrl-R restart | yes |
| Ctrl-O: reload chosen `.ERB` files | no; Ctrl-O is the settings overlay, F5 / Ctrl-E reload everything |
| Ctrl-C / Ctrl-Insert with nothing selected: a window with all the text on screen | no |
| Ctrl-Up / Ctrl-Down, Ctrl + wheel: scroll the clipboard back through earlier lines (`CBScrollCount`) | no |
| Ctrl-V / Shift-Insert: paste into the input line | depends on the terminal's paste |
| Clipboard **on** by default (Anchor; EE defaults it off) | off by default; only matters when `emuera.config` has no such line |
