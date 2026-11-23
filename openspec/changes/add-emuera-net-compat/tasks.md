# Tasks

## 1. Built-ins

- [x] 1.1 `GETCSVNOBYNAME` / `CALLNAME` / `NICKNAME` / `MASTERNAME`, `HASH_XXH32`, `MATCHALL` (`7df26ec`, `tests/emuera_net.rs`).
- [ ] 1.2 Re-run the built-in survey over every local game shipping Emuera.NET; implement any others they use.

## 2. HTML dialect

- [x] 2.1 HTML_PRINT lines start left-aligned; `border_width` / `border_color` / `background_color` on `<div>` (`b132a79`).
- [ ] 2.2 `<div>` without width / height sizes to its content (ConsoleDivElement). Verify: unit test; eraMegatenP's title faces framed as in Emuera.
- [ ] 2.3 `display` (relative / absolute modes), `hover-background-color`, `<img xpos display>`. Verify: unit tests against HtmlManager.cs.

## 3. Game fonts

- [ ] 3.1 Load `font/*.ttf|otf` (FontFactory.LoadFontFolder): known by family and file name; GDRAWTEXT and friends use them. Verify: unit test with eraMegatenP's EraMTFont.
- [ ] 3.2 Private-use glyphs of the game's font drawn as pictures in the text's colour where pictures show. Verify: render test (placements emitted for PUA cells); eraMegatenP's dungeon map by hand.
- [ ] 3.3 Without pictures, GETFONT / GETCONFIG("フォント名") report the terminal's font when the configured one comes from `font/`. Verify: inline-game test; eraMegatenP's map prints its emoji tiles with `--gfx none`.

## 4. setting.json

- [ ] 4.1 Read `setting.json` (game folder); `UseRenameInCharaCSV`, `UseScopedVariableInstruction`, `CheckUTF8withBOM`. Verify: inline-game tests.
- [ ] 4.2 `UseNewRandom` (Emuera.NET's other RNG): check what eraMegatenP sets (false) and implement only if a game turns it on.

## 5. eraMegatenP screens

- [ ] 5.1 Dungeon party block (portraits, names, HP) as Emuera.NET shows it. Needs a current-version dungeon save.

## 6. Docs

- [ ] 6.1 Wiki *Implemented* (Emuera.NET section), *Pictures and Graphics* (game fonts, glyph pictures), `doc/differences.md`.
