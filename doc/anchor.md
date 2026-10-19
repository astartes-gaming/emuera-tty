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
- A line is `original,translation`. Text after `;` is a comment, and a
  line starting with `;` is skipped. Neither side is trimmed.
- A line without exactly one comma, or a key given twice, is an
  exception: **the game does not start**.
- A `_TR.csv` for any other table is ignored. pops-tw ships
  `Juel_TR.csv`, which Anchor never reads (Juel isn't in the list).

emuera-tty (`src/translate.rs`): also reads subfolders, trims both sides
and a trailing comma, keeps the first of two duplicate keys, and gives no
warnings. *Gap: only the warnings; see design D6.*

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

emuera-tty does steps 1, 2 and 4 for PRINT/PRINTDATA/HTML_PRINT FORMs.
*Gap: step 3, the width trigger and the RETURNF exception.*

## 4. Constants translated at first parse

`GameData/Function/FunctionMethodTerm.cs:46`: when a built-in function
call is built, each argument is constant-folded **with translate =
true**. So a CSV name with a constant index, given as an argument to
*any* built-in function, is replaced by its translation. Examples:
`STRLENS(TALENTNAME:5)`, `SUBSTRING(ABLNAME:2, 0, 4)`, and every
`%TALENTNAME:5%` (FORM `%…%` is itself a function term). A variable
index (`TALENTNAME:LOCAL`) is not folded, and is read with the flag at
run time.

`STRLENS` / `STRLENSU` (`Creator.Method.cs:1947`) also evaluate their
argument with the flag on and look the result up in ALL_TR, so they
measure the translated text.

*Gap in emuera-tty: all of §4.*

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
