# Proposal

## Why

eraMegatenP is written for Emuera.NET (the `emuera_Fork` engine, 0.2.6.0), not Emuera-EM/EE, and runs badly here: its title logo was misplaced, older saves stopped on a missing built-in, and its dungeon map and party screen are broken. Emuera.NET differs from EM in its built-ins, its HTML_PRINT dialect (a real HTML parser), its game-folder fonts, and a `setting.json` of its own. Games written for it should play here as they do there.

## What Changes

- The Emuera.NET built-ins its games use (`GETCSVNOBY*`, `HASH_XXH32`, `MATCHALL`: done), and the rest of its additions when a game needs them.
- Its HTML_PRINT dialect: `<div>` without width/height sizes to its content; its attribute names (`border_width`, `border_color`, `background_color`, `hover-background-color`, `display`), `<img xpos display>`, HTML lines left-aligned whatever ALIGNMENT says (done for both engines).
- Game-folder fonts (`font/*.ttf|otf`, as Emuera.NET's FontFactory): the configured font is found there; characters that only it has (private-use glyphs, eraMegatenP's dungeon tiles) are drawn as small pictures where the terminal shows pictures, and the game is told otherwise where it can't (so games with a text fallback use it).
- `setting.json` switches that change scripts: `UseRenameInCharaCSV`, `UseNewRandom`, `UseScopedVariableInstruction`, `CheckUTF8withBOM`.
- eraMegatenP's dungeon party screen as Emuera.NET shows it.

Nothing changes for Emuera-EM/EE games: every addition is something EM rejects or doesn't have, or follows a setting EM games don't have.

## Capabilities

### New Capabilities

- `emuera-net-compat`: what games written for Emuera.NET get: built-ins, HTML dialect, setting.json.
- `game-fonts`: fonts a game ships in its `font/` folder, and characters only they have.

### Modified Capabilities

None.

## Impact

- `src/vm/builtins.rs`, `src/html.rs`, `src/console.rs` (div sizes), `src/tty.rs` + `src/gfx/` (glyph pictures), `src/resources.rs` (game fonts), `src/csvdata.rs`, `src/vm/mod.rs` (RNG), a reader for `setting.json`.
- Tests: `tests/emuera_net.rs`; render tests for glyph pictures.
- Docs: wiki *Implemented*, *Pictures and Graphics*; `doc/differences.md` where we differ.
