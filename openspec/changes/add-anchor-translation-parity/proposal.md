# Proposal

## Why

Players want games that need Emuera-Anchor: English fan translations that ship `*_TR.csv` / `TRCHARA*.csv` files and are tested only against Anchor. emuera-tty reads those files, but it translates by a simpler rule than Anchor's ("a `%…%` being printed"). So text Anchor shows in English stays Japanese here: names passed through variables, `#FUNCTIONS` results, PRINTV, `STR:n`, `STRLENS` widths. Games with `PARAM.CSV` don't load their parameter names at all. The analysis, with source references, is in `doc/anchor.md`.

## What Changes

- Anchor's **translate flag** (the rule for when a value counts as being printed): on for statements the engine runs; carried into PRINT* (not plain `PRINT text`), PRINTPLAIN (always), HTML_PRINT, string SET and RETURNF; passed on through `+`, string repetition, FORM `%…%`, TOUPPER/TOLOWER/TOHALF/TOFULL, REPLACE, STRFORM and `#FUNCTION(S)` bodies.
- What a flagged read translates: `XXXNAME:i` through its own table; the values of STR, TSTR, SAVESTR, RESULTS and GLOBALS through Str_TR; string literals through the table named on the line (or ALL).
- **BREAKING (Anchor-translated games only):** a string SET in flagged code stores the translated text, as Anchor does, so those values and the saves holding them match Anchor's. Games without `*_TR` / `TRCHARA` files are unaffected.
- The `%…%` **fallbacks**: an ALL_TR lookup on the whole value, then a 20-entry memory of names read with the flag off, then a lookup with digits and brackets stripped. A width on the `%…%` turns translation on; RETURNF lines skip the fallbacks; a `%…%` that is constant at parse time gets only its own table.
- **Constant folding**: a constant-index CSV name given to any built-in function is translated, and STRLENS/STRLENSU measure translated text.
- **`PARAM.CSV`** is read instead of `PALAM.CSV` when present, and Param_TR is used. In games with PARAM.CSV or any TR file, every identifier has `PARAM` renamed to `PALAM`, as Anchor does (Anchor renames in every game).
- **TRCHARA**: `ENDCHAR`, case-insensitive file names and keys, and Anchor's load warnings. Only the fields a block gives are replaced; Anchor's blanking of the fields a block leaves out is not copied.
- **TR_NAME**: a script error for an unknown code, key or index, as in Anchor.
- `*_TR.csv` problems (a line without exactly one comma, a duplicate key) are shown as load warnings and the game starts. Anchor refuses to start.
- Values in `*_TR.csv` are no longer trimmed, matching Anchor (a translation may start with a space for alignment).
- README "Translated games" section and `doc/emuera-tty.1` updated.

Out of scope: the hotkey and clipboard extras Anchor shares with EE (listed at the end of `doc/anchor.md`), and Anchor's custom-icon config key (kept when the config is rewritten, otherwise ignored). PRINT_ABL/TALENT/MARK/EXP stay translated here, though Anchor leaves them in Japanese.

## Capabilities

### New Capabilities

- `anchor-translation`: the `*_TR.csv` name tables, when a value is translated (the translate flag), the `%…%` fallbacks, constant folding, STRLENS/STRLENSU, the engine-printed lists, and `TR_NAME`.
- `anchor-character-translation`: `TRCHARA*.csv` files and how they apply to character templates.
- `param-csv`: Anchor's `PARAM.CSV` as a replacement for `PALAM.CSV`, with identifier renaming.

### Modified Capabilities

(none; `openspec/specs/` is empty)

## Impact

- Code: `src/translate.rs` (tables, memory, fallbacks, TRCHARA), `src/vm/mod.rs` (the flag through FORM / PRINT / SET / RETURNF / `#FUNCTION` evaluation), `src/vm/builtins.rs` (TR_NAME, STRLENS(U), the string functions that pass the flag on), `src/parser.rs` (PARAM renaming, constant folding with translation), `src/csvdata.rs` (PARAM.CSV, the TR table map), `tests/`.
- Behaviour is gated: all translation work runs only when the game has a `*_TR.csv` or `TRCHARA*.csv`; reading PARAM.CSV only when it exists; PARAM renaming only in either kind of game. Every other game must play exactly as before; the existing tests, the sample self-test and an AnonTW / pops-tw regression run check this.
- Performance: the flag adds one boolean to string evaluation. The memory and fallbacks run only for printed `%…%` values in translated games.
- Docs: `README.md`, `doc/emuera-tty.1`, `doc/anchor.md`.
