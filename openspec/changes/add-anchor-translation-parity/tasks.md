# Tasks

## 1. Test scaffolding

- [x] 1.1 Move the inline-game helpers from `tests/sample_selftest.rs` (`run`, plus a new `game(dir, files: &[(&str, &str)])` that writes CSV/ERB files into a temp dir) into `tests/common/mod.rs`, and use them from `sample_selftest.rs`. Verify: `cargo test` passes with the same three tests.
- [x] 1.2 Record a baseline in `../emuera-tty-baseline/` (see its `BASELINE.md`): `--headless` output of AnonTW and eraTohoK (no TR files; `sample/` too, for reference, though it has TR files), 800 seeded random inputs each through `drive.py`, with the before binary kept as `bin/emuera-tty.before`. Follow CLAUDE.md's *Testing beyond `cargo test`* rules: scratch copies, one memory-capped process at a time. Also record pops-tw, which ships Anchor TR files and TRCHARA, as a "before" snapshot: a 40-step `tools/gamed.py` session replayed by `replay.sh` (its title can't be passed headless). It is not reproducible, because its random numbers drift with wall time in the TUI. Verify: two runs of each headless game are identical under `compare.sh`, and the pops-tw transcript and slot-1 save exist.

## 2. TR table loading (spec: TR table files, TR file problems are warnings; design D6)

- [x] 2.1 In `src/translate.rs`, stop trimming keys and values (drop only one trailing `\r`), match `<TABLE>_TR.csv` names case-insensitively, and collect load warnings for lines without exactly one comma and for duplicate keys (first kept). Show them like the other load warnings. Verify: unit tests for an untrimmed value, a comment, a duplicate key and a two-comma line.
- [x] 2.2 Remove `Tr::name`'s fallback to ALL, and keep ALL available as its own lookup. Verify: the unit test "No ALL fallback for a name read" passes, and the existing `tr_files` test is updated to match.
- [x] 2.3 Add the gate the specs share: one `Vm` / `csvdata` predicate "game has TR or TRCHARA files". Verify: a test game without TR files gives `LOCALS '= TALENTNAME:5` and prints the CSV name (scenario "Game without TR files").
- [x] 2.4 README "Translated games": document the warnings and the untrimmed values. Verify: the text matches the implemented messages.

## 3. The translate flag (spec: Translate flag, Translated string assignment, Flag passed through expressions, What a flagged read translates; design D1, D2, D9)

- [ ] 3.1 Add `tr_run` / `tr_eval` to `Vm`. Set `tr_run` for engine-run statements. In statement dispatch, set `tr_eval` per statement (PRINT family except raw text, PRINTDATA, HTML_PRINT, PRINTPLAIN always, string Assign / AssignForm with any operator, ReturnF), and set it again at the start of each statement so errors can't leave it on. Verify: `tests/anchor_translation.rs` covers the "PRINTV", "condition", "plain PRINT text" and "PRINTBUTTON" scenarios.
- [ ] 3.2 Translate flagged reads at the variable: CSV name arrays through their own table; STR, TSTR, SAVESTR, RESULTS and GLOBALS through the Str table. Replace `Vm::translated` / `eval_form_tr` with this, dropping the ALL lookup on arbitrary PRINTS values. Verify: the "RESULTS through Str_TR" scenario passes, and the sample self-test still passes.
- [ ] 3.3 Assignments: `=`, `'=`, `'=` lists, `+=` and `*=` on string targets store the flagged value. Verify: the "Expression assignment", "FORM assignment" and "Appending" scenarios pass, and a save written afterwards holds the translated text (check the saved bytes).
- [ ] 3.4 Pass-through: `builtin_fn` clears `tr_eval` for its arguments except for TOUPPER, TOLOWER, TOHALF, TOFULL, REPLACE and STRFORM; `+` and string repeat keep it; `? #` clears it; a `#FUNCTION(S)` called in an expression runs with `tr_run = tr_eval` and restores it on `Ok` and `Err`. Verify: the "Concatenation", "User function result" and "Ternary does not pass the flag" scenarios pass, plus a test that a script error inside a flagged `#FUNCTIONS` leaves later conditions untranslated.
- [ ] 3.5 README "Translated games": rewrite the "a name is translated when it is printed" paragraph to describe the flag in player terms (printed, assigned, returned). Verify: every example in it is a test in `tests/anchor_translation.rs`.

## 4. Per-line facts (spec: Flagged string literals; design D3)

- [ ] 4.1 Tell the parser whether the game has TR files. When it does, have it fill `Body::tr_line`: one byte per instruction holding the literal-table index from the raw line (first `<TABLE>NAME` found in TR table order, else ALL) and a "line contains RETURNF" bit. When it doesn't, leave the vec empty. Verify: unit test on a parsed body: `PRINTS TALENTNAME:5` → Talent, `PRINTS "男"` → ALL, an untranslated game → empty vec.
- [ ] 4.2 Flagged literal and constant-folded string reads use the current instruction's table. Verify: the "Literal through ALL" and "Literal on a TALENTNAME line" scenarios pass.
- [ ] 4.3 Check memory: `EMUERA_MEMSTATS` on pops-tw (its own TR files) shows the compiled-code size growing by at most one byte per instruction. Verify: before/after numbers noted in the PR.

## 5. Percent fallbacks and the memory (spec: Memory of untranslated reads, Percent values get an ALL lookup, Memory fallback, Stripped ALL lookup, RETURNF lines skip the fallbacks, Padding counts translated text; design D4)

- [ ] 5.1 Add `tr_memory` (20 entries) and `tr_last` to `Vm`. Push on unflagged reads that have a translation, set `tr_last` on flagged successes, and clear both on restart and reload. Verify: the "Twenty-first pair" scenario passes, plus a test that restart (Ctrl-R path) empties the memory.
- [ ] 5.2 In FORM `%…%` evaluation, run the flag-or-width trigger, then the ALL lookup, `tr_last` check, memory and stripped lookup in order, skipping all of them on RETURNF lines, and pad by display width afterwards. Verify: the "Whole value in ALL_TR", "Width turns translation on", "Value from a variable", "Level suffix", "FORM in RETURNF" and "Padded name" scenarios pass.
- [ ] 5.3 Check speed: the AnonTW 800-input session with a stand-in `Talent_TR.csv` takes no more than 5% more CPU than without it, and without TR files no more than baseline (task 1.2) plus noise. Verify: `time` numbers noted in the PR.

## 6. Constants and STRLENS (spec: Constant names given to built-in functions, STRLENS measures translated text; design D5)

- [ ] 6.1 In `builtin_fn` argument evaluation, read a CSV name variable with all-literal indices translated (own table) whatever the flag. Mark constant `%…%` parts at parse (a bit on `FormPart::Str`) so they get only their own translation. Verify: the "SUBSTRING of a constant name" and "Variable index is not folded" scenarios pass, and a constant `%TALENTNAME:5%` whose value is also an ALL_TR key prints the Talent_TR translation.
- [ ] 6.2 STRLENS / STRLENSU evaluate their argument with the flag on, then do an ALL lookup before measuring. Verify: the "Width of a translated name" scenario passes.

## 7. TR_NAME (spec: TR_NAME)

- [ ] 7.1 Raise script errors for an unknown table code, an undefined string key and an index without a name, worded like Anchor's ("Invalid first argument" / "Invalid second argument"). Accept `PARAM` / `PARAMNAME` (Param_TR when PARAM.CSV was read). Verify: the "Index", "Unknown key" and "Unknown table" scenarios pass.
- [ ] 7.2 README and man page: document the errors. Verify: `emuera-tty --manual` shows the updated text.

## 8. TRCHARA (spec: anchor-character-translation)

- [ ] 8.1 In `apply_trchara`, match file names and keys case-insensitively, accept `ENDCHAR`, and keep replacing only the fields a block gives. Verify: unit tests for "Name with a suffix in a subfolder", "Lower-case file name", "Full block" and "Block with only CALLNAME".
- [ ] 8.2 Emit Anchor's warnings (fields, second NO, data before NO, bad or unknown NO, unknown key, CSTR without text, CSTR index twice). Verify: the "Unknown key" and "Missing ENDCHR" scenarios pass, and each warning names its file and line.
- [ ] 8.3 Verify the "New game" and "Old save" scenarios end to end in `tests/anchor_translation.rs` (save with no TRCHARA, add one, load).
- [ ] 8.4 README and man page: `ENDCHAR`, case-insensitivity, warnings. Verify: `emuera-tty --manual` shows the updated text.

## 9. PARAM.CSV (spec: param-csv; design D8)

- [ ] 9.1 In `csvdata.rs`, load `PARAM.CSV` (case-insensitive) in place of `PALAM.CSV` when present, naming it in messages, and map `Param_TR.csv` to the PALAM table only in that case. Verify: the "Both files present", "Only PALAM.CSV" and "PRINT_PALAM with Param_TR" scenarios pass.
- [ ] 9.2 In the parser / expression tokenizer, replace `PARAM` with `PALAM` in identifiers when the game has PARAM.CSV or TR files, and in VARSIZE's literal argument. Verify: the "Built-in variables", "User variable with PARAM in its name", "Variable-name string", "Translated game with PALAM.CSV" and "Untranslated game without PARAM.CSV" scenarios pass.
- [ ] 9.3 README (Implemented list) and man page: PARAM.CSV support. Verify: `emuera-tty --manual` mentions PARAM.CSV.

## 10. Integration

- [ ] 10.1 Compare against the task 1.2 baseline: `./run.sh <new binary> GAME after` for `anon-tw` and `eraTohoK` (the games without TR files; `sample/` has TR files and is checked by its self-test), then `compare.sh` against each `a` run. Verify: both report "same".
- [ ] 10.2 Run `cargo test`, `cargo clippy` (no new warnings), and `cargo check --target x86_64-pc-windows-msvc` and one big-endian target (`powerpc64-unknown-linux-musl`). Verify: all succeed.
- [ ] 10.3 Run `replay.sh <new binary> after` for pops-tw and read its screens side by side with `out/pops-tw.a.txt`, noting anything Anchor would translate that is still untranslated, and anything translated that Anchor wouldn't. Random numbers differ between runs, so ignore differences in weather, shop stock and similar. Load and re-save `out/pops-tw.after.sav` and compare with `tools/savdiff.py`, keeping the same noise in mind. If a player can supply a save Anchor wrote in pops-tw, load it, re-save it and compare: string values translated in one and not the other point to a gap in the translate-flag rules. Verify: notes in the PR, with each problem either fixed or filed.
- [ ] 10.4 Update `doc/anchor.md`: mark each gap closed, and add anything learned in 10.3. Verify: no "Gap:" lines remain except those the proposal leaves out of scope.
- [ ] 10.5 Run `openspec validate add-anchor-translation-parity --strict`. Verify: it reports the change is valid.

## Workflow follow-up

- Archive the change with `openspec archive add-anchor-translation-parity` after review, then check that `openspec/specs/` holds the three capabilities.
