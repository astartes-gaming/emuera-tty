`cargo test --release` runs the unit tests, the sample game's self-test and
the **ERB test suite**: small Emuera games that check the engine against
what Emuera does.

- `tests/erb/*.erb`: one file per topic (strings and regular expressions,
  what RESULT holds, indices by CSV name, control flow, characters, saves,
  graphics, XML and data tables, input, the system flow…). Each is a
  `@TEST_MAIN` of `CALL CHECK, "what", condition` lines; `;@input` lines are
  typed in. A bug fixed gets its check here.
- `tests/golden/*.erb` print text that must match the `.out` beside them
  exactly (`UPDATE_GOLDEN=1 cargo test --release --test erb_suite` rewrites
  them after a deliberate change).
- `tests/save/`: a fixed game state whose `save00.sav` / `global.sav` must
  match byte for byte (Emuera's binary format).

The same files run on the **real Emuera**, which is what makes the suite an
independent check: `tools/erb_suite_export.sh DIR` builds them as ordinary
game folders; with Emuera.exe run in each (each writes `emuera.log`),
`tools/erb_suite_compare.py DIR` reports any check that fails there, golden
output that differs and saves that aren't byte-identical.
`tools/erb_coverage.py` lists which of Emuera's 558 built-in functions and
statements no test uses yet (about 90% are used).

`tools/game_smoke.sh GAME_DIR [STEPS] [SEED]` plays a real game with random
menu choices in a throwaway copy and reports script errors.

## Compatibility suite

`compat/` is the [eraBasic compatibility suite](https://gitgud.io/era-games/erabasic-compat)
(mirror: https://github.com/vinumsabbathi789/erabasic-compat),
a git submodule (clone with `--recurse-submodules`, or run
`git submodule update --init`): one game of checks, meant for every
eraBasic interpreter, whose expected values are Emuera-EM/EE's.
`cargo test --release --test compat_suite -- --ignored` runs it; it is
ignored by default but **now passes** (since 679ba6dd). Note that even
though the compat suite passes, there could still be other bugs — see
[When something goes wrong](Troubleshooting).

## Development builds

For development: `cargo test` runs the unit tests and the `sample/` game's
self-test; `cargo build --profile fastbuild --features profile` makes a
quick-to-build optimised binary whose `EMUERA_PPROF=<file>` writes a
sampling profile of the engine itself (time per Rust function and caller
→ callee pairs); `emuera-tty sample --headless --echo` plays the sample in plain
text.
