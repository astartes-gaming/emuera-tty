# Spec Delta

## Purpose

Plays games translated with Emuera-Anchor's `*_TR.csv` system the way Anchor shows them: the same names come out translated in the same places, so fan translations tested on Anchor read correctly here.

## ADDED Requirements

### Requirement: Translation only in translated games
All behaviour in this capability SHALL apply only when the game has at least one `*_TR.csv` file in its CSV folder itself, or a `TRCHARA*.csv` file in the CSV folder or a subfolder. A game without them SHALL behave exactly as if this capability did not exist.

#### Scenario: Game without TR files
- **WHEN** a game with no `*_TR.csv` or `TRCHARA*.csv` runs `LOCALS '= TALENTNAME:5` and `PRINTFORML %LOCALS%`
- **THEN** LOCALS holds the Talent.csv name and the printed line is that name

### Requirement: TR table files
A file `<TABLE>_TR.csv` in the CSV folder itself, not its subfolders, SHALL translate the CSV name table it names. `<TABLE>` is one of Abl, Talent, Exp, Mark, Palam, Param, Item, Train, Base, Source, Ex, EQUIP, TEQUIP, Flag, TFLAG, Cflag, Tcvar, CSTR, Stain, Cdflag1, Cdflag2, Str, TSTR, SaveStr, GLOBAL, GLOBALS or ALL, matched case-insensitively. Each line is `original,translation`. Text from `;` on is a comment. Only white space before the original is dropped.

#### Scenario: Comment and untrimmed value
- **WHEN** `Abl_TR.csv` holds `技巧, Technique; skill`
- **THEN** the key `技巧` maps to ` Technique` (with its leading space)

#### Scenario: Comment-only line
- **WHEN** a line starts with `;`
- **THEN** it is skipped

#### Scenario: Indented line
- **WHEN** a line reads `  感覚,Sense `
- **THEN** the key `感覚` maps to `Sense ` (Anchor's line reader skips leading white space, nothing else)

#### Scenario: TR file in a subfolder
- **WHEN** the game has `CSV/sub/Talent_TR.csv` and no other TR files
- **THEN** it is not read, and the game counts as untranslated

### Requirement: TR file problems are warnings
A line without exactly one comma (after the comment is removed), a misplaced `{` or `}` line, or a key already defined in the same file, SHALL produce a load warning naming the file and line. The game SHALL still start. A bad line SHALL be skipped, and for duplicates the first definition SHALL be kept.

#### Scenario: Duplicate key
- **WHEN** `Talent_TR.csv` defines `処女` twice
- **THEN** a warning names the file and the second line, the first translation is used, and the game starts

#### Scenario: Line with two commas
- **WHEN** a line reads `a,b,c`
- **THEN** a warning names the file and line, and the line is ignored

#### Scenario: Joined lines
- **WHEN** a `{` line, the lines `技巧,` and `Technique`, and a `}` line follow each other
- **THEN** they are read as one line `技巧, Technique ` (a space after each), as Anchor's EraStreamReader joins them

### Requirement: Translate flag
Each string evaluation SHALL run with a translate flag. The flag SHALL be on for statements the engine runs, including in CALLed, JUMPed and event functions. A statement SHALL evaluate its arguments with the flag only if it is: a PRINT-family statement other than plain `PRINT text`, PRINTDATA, HTML_PRINT, a string assignment, or RETURNF. PRINTPLAIN / PRINTPLAINFORM SHALL always translate. Every other evaluation SHALL run with the flag off.

#### Scenario: Name printed with PRINTV
- **WHEN** Talent_TR maps `処女` to `Virgin` and the script runs `PRINTV TALENTNAME:LOCAL` with LOCAL = 5 (Talent 5 = `処女`)
- **THEN** `Virgin` is printed

#### Scenario: Name in a condition
- **WHEN** the same game evaluates `IF TALENTNAME:LOCAL == "処女"`
- **THEN** the comparison sees `処女` and is true

#### Scenario: Plain PRINT text
- **WHEN** the script runs `PRINTL 処女`
- **THEN** `処女` is printed untranslated

#### Scenario: PRINTBUTTON
- **WHEN** the script runs `PRINTBUTTON TALENTNAME:LOCAL, 1`
- **THEN** the button text is `処女`

### Requirement: Translated string assignment
Every string assignment evaluated with the flag on SHALL store the translated value. This covers `=` (FORM text), `'=` (expression), the `'= a, b, c` list form, `+=` (where the variable's old value and the right-hand side are joined with the flag on), and `*=`.

#### Scenario: Expression assignment stores translation
- **WHEN** a game whose Talent_TR maps `処女` to `Virgin` runs `LOCALS '= TALENTNAME:LOCAL` (LOCAL = 5) in a CALLed function
- **THEN** LOCALS holds `Virgin`, and a save written afterwards holds `Virgin`

#### Scenario: FORM assignment
- **WHEN** the same game runs `LOCALS = [%TALENTNAME:LOCAL%]`
- **THEN** LOCALS holds `[Virgin]`

#### Scenario: Appending
- **WHEN** LOCALS is `a` and the game runs `LOCALS += TALENTNAME:LOCAL`
- **THEN** LOCALS holds `aVirgin`

### Requirement: Flag passed through expressions
With the flag on, it SHALL pass to both operands of string `+`, to the string operand of string × integer, to `%…%` and `{…}` in FORM strings, to the arguments of TOUPPER, TOLOWER, TOHALF, TOFULL, REPLACE and STRFORM, and to the body of a `#FUNCTION(S)` called in the expression. It SHALL NOT pass to the branches of `? #` or to other built-in functions' arguments.

#### Scenario: Concatenation
- **WHEN** a flagged PRINTS evaluates `"[" + TALENTNAME:LOCAL + "]"`
- **THEN** `[Virgin]` is printed

#### Scenario: User function result
- **WHEN** `@NAME_OF(ARG)` is `#FUNCTIONS` and does `RETURNF TALENTNAME:ARG`, and a flagged PRINTFORML shows `%NAME_OF(5)%`
- **THEN** `Virgin` is printed

#### Scenario: Ternary does not pass the flag
- **WHEN** a flagged PRINTS evaluates `LOCAL ? TALENTNAME:LOCAL # ""` with LOCAL = 5
- **THEN** `処女` is printed

### Requirement: What a flagged read translates
A flagged read of a CSV name variable (`ABLNAME`, `TALENTNAME` … `GLOBALSNAME`) SHALL use its own table only. A flagged read of STR, TSTR, SAVESTR, RESULTS or GLOBALS SHALL look its value up in Str_TR. PALAM names SHALL use Param_TR instead of Palam_TR when the game has `PARAM.CSV`.

#### Scenario: No ALL fallback for a name read
- **WHEN** Talent_TR lacks `処女`, ALL_TR maps it to `Maiden`, and a flagged PRINTV shows `TALENTNAME:LOCAL`
- **THEN** `処女` is printed

#### Scenario: RESULTS through Str_TR
- **WHEN** Str_TR maps `はい` to `Yes`, RESULTS is `はい`, and a flagged PRINTV shows `RESULTS`
- **THEN** `Yes` is printed

### Requirement: Flagged string literals
A string literal, or a value folded to a constant, read with the flag on SHALL be looked up in a table chosen from the current line's text. The table SHALL be the first one in the TR table order whose upper-cased name followed by `NAME` occurs in the line, or ALL if none does.

#### Scenario: Literal through ALL
- **WHEN** ALL_TR maps `男` to `Man` and the script runs `PRINTS "男"`
- **THEN** `Man` is printed

#### Scenario: Literal on a TALENTNAME line
- **WHEN** the line is `PRINTS TALENTNAME:5` (constant index, folded at parse)
- **THEN** the value is looked up in Talent_TR

### Requirement: Memory of untranslated reads
When a name is looked up with the flag off and its table has a translation, the pair (original, table) SHALL be remembered, unless already remembered. At most 20 pairs SHALL be kept; the oldest is dropped first.

#### Scenario: Twenty-first pair
- **WHEN** 21 distinct translatable names are read with the flag off
- **THEN** the first one read is no longer remembered

### Requirement: Percent values get an ALL lookup
A `%…%` value evaluated at run time with the flag on, or with a width given (which turns the flag on for it), SHALL first be evaluated with the flag. The whole result SHALL then be looked up in ALL_TR. These are the percent fallbacks; if the ALL lookup leaves the value unchanged, the memory fallback follows.

#### Scenario: Whole value in ALL_TR
- **WHEN** LOCALS is `男`, ALL_TR maps `男` to `Man`, and `PRINTFORML [%LOCALS%]` runs
- **THEN** `[Man]` is printed

#### Scenario: Width turns translation on
- **WHEN** a `%TALENTNAME:LOCAL, 10%` is evaluated with the flag off (in a STRFORM inside an IF)
- **THEN** the translated name is used, padded to 10 columns

### Requirement: Memory fallback
If the ALL lookup left a percent value unchanged, then: if the value contains the most recent successful translation, it SHALL stay as it is. Otherwise the remembered originals SHALL be checked newest first, and the first one found inside the value SHALL be replaced by its translation. If none is found, the stripped lookup follows.

#### Scenario: Value from a variable
- **WHEN** `LOCALS '= TALENTNAME:LOCAL` ran with the flag off (inside an IF condition's `#FUNCTIONS`), then `PRINTFORML [%LOCALS%]` runs
- **THEN** `[Virgin]` is printed (from memory)

### Requirement: Stripped ALL lookup
If the memory fallback found nothing and ALL_TR exists, the value with every digit, `【`, `】`, `[`, `]`, `『`, `』` and `LV` removed SHALL be looked up in ALL_TR. If it is a key, that stripped text SHALL be replaced by its translation within the value.

#### Scenario: Level suffix
- **WHEN** ALL_TR maps `技巧` to `Technique` and a flagged `%…%` value is `技巧LV3`
- **THEN** `TechniqueLV3` is printed

### Requirement: RETURNF lines skip the fallbacks
A `%…%` evaluated while the current line is a RETURNF SHALL get only its own translation, with none of the percent fallbacks (ALL lookup, memory, stripped lookup).

#### Scenario: FORM in RETURNF
- **WHEN** a `#FUNCTIONS` does `RETURNF @"%LOCALS%"` with LOCALS = `男` and ALL_TR maps `男` to `Man`
- **THEN** the function returns `男`

### Requirement: Padding counts translated text
A `%…%` or `{…}` width SHALL be applied to the translated text, by display width.

#### Scenario: Padded name
- **WHEN** `PRINTFORML [%TALENTNAME:LOCAL, 8, LEFT%]` translates to `Virgin`
- **THEN** `[Virgin  ]` is printed

### Requirement: Constant names given to built-in functions
A CSV name with a constant index, given as an argument to any built-in function or within a `%…%`, SHALL be replaced by its own-table translation when the line is first parsed, whatever the flag. If the whole `%…%` is then constant, the percent fallbacks SHALL NOT apply to it.

#### Scenario: SUBSTRING of a constant name
- **WHEN** Abl_TR maps `技巧` (ABL 2) to `Technique` and the script evaluates `SUBSTRING(ABLNAME:2, 0, 4)` in an IF condition
- **THEN** the result is `Tech`

#### Scenario: Variable index is not folded
- **WHEN** the same call is `SUBSTRING(ABLNAME:LOCAL, 0, 4)` with LOCAL = 2, in an IF condition
- **THEN** the result is the first 4 columns of `技巧`

### Requirement: STRLENS measures translated text
STRLENS and STRLENSU SHALL evaluate their argument with the flag on, look the result up in ALL_TR, and return the length of that result.

#### Scenario: Width of a translated name
- **WHEN** `STRLENS(TALENTNAME:LOCAL)` is evaluated with LOCAL = 5 and `処女` translates to `Virgin`
- **THEN** the result is 6

### Requirement: Engine-printed lists
The TRAIN command list, the shop list and PRINT_SHOPITEM, PRINT_ITEM and PRINT_PALAM SHALL show translated names from their tables (Train, Item, Item, Item, Palam/Param). PRINT_ABL, PRINT_TALENT, PRINT_MARK and PRINT_EXP SHALL also show translated names, although Anchor does not.

#### Scenario: Shop list
- **WHEN** Item_TR maps `ローター` to `Vibrator` and the shop lists that item
- **THEN** the list shows `Vibrator`

### Requirement: TR_NAME
`TR_NAME(code, key)` SHALL return the translation of a CSV name, or the name itself when it has none. `code` is a string or bare variable naming a table (`ABL` … `TSTR`, optionally with `NAME`; `PALAM` and `PARAM` both mean the PALAM table). `key` is an index with a name or a name defined in that CSV. Otherwise TR_NAME SHALL raise a script error.

#### Scenario: Index
- **WHEN** the script evaluates `TR_NAME(ABL, 2)` and Abl_TR maps `技巧` to `Technique`
- **THEN** the result is `Technique`

#### Scenario: Unknown key
- **WHEN** the script evaluates `TR_NAME("TALENT", "nonexistent")`
- **THEN** a script error names TR_NAME's second argument

#### Scenario: Unknown table
- **WHEN** the script evaluates `TR_NAME("BOGUS", 1)`
- **THEN** a script error names TR_NAME's first argument
