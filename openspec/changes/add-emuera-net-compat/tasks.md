# Tasks

## 1. Built-ins

- [x] 1.1 `GETCSVNOBYNAME` / `CALLNAME` / `NICKNAME` / `MASTERNAME`, `HASH_XXH32`, `MATCHALL` (`7df26ec`, `tests/emuera_net.rs`).
- [x] 1.2 Re-run the built-in survey over every local game shipping Emuera.NET; implement any others they use. Done: the survey run over era-games and era-archive (278 folders): only eraKATANA uses more, the `SQL_*` functions, now in `vm/sql.rs` behind the default `sqlite` feature (rusqlite, bundled). eraKATANA, eraBEMANI and eraDaL smoke-play clean. (Corrected later: of these only eraMegatenP and eraKATANA ship epizo12345's fork, the one with AngleSharp inside; eraDaL ships Emuera-EE's .NET build, also called Emuera.NET, and eraBEMANI Emuera-EM on .NET Framework with a stray `Emuera.runtimeconfig.json`.)

## 2. HTML dialect

- [x] 2.1 HTML_PRINT lines start left-aligned; `border_width` / `border_color` / `background_color` on `<div>` (`b132a79`).
- [x] 2.2 `<div>` without width / height sizes to its content (ConsoleDivElement). Verify: unit test; eraMegatenP's title faces framed as in Emuera. Done: `div_sizes_to_its_content_and_nests`; a game recognised as Emuera.NET (`loader::ships_emuera_net`) also grows a div that is too small for its content, as Emuera.NET draws content past the box (`emuera_net_divs_hold_their_content`).
- [x] 2.3 `display` (relative / absolute modes), `hover-background-color`, `<img xpos display>`. Verify: unit tests against HtmlManager.cs. Done: `DivPlace` on `Seg::Div`, painted against the window by `paint_on_window` (`div_display_modes`, `window_placed_divs_and_island`); an absolute `<img>` is a div holding it; `hover-background-color` is accepted but not drawn (only commented-out eraMegatenP lines use it).
- [x] 2.4 Divs inside divs (eraMegatenP's message log): `div_content_end`, nested divs painted after their parent (`paint_div`).
- [x] 2.5 `HTML_PRINT_ISLAND` as a layer over the window (Emuera.NET's `_htmlElementListDict`, with depths), out of the log, buttons always live; `HTML_PRINT_ISLAND_CLEAR [depth]`. Verify: `html_island_layers`; eraMegatenP's INPUT_YN_POPUP by hand (drawn at the bottom, `[1] No` clicked, cleared).

## 3. Game fonts

- [x] 3.1 Load `font/*.ttf|otf` (FontFactory.LoadFontFolder): known by family and file name; GDRAWTEXT and friends use them. Verify: unit test with eraMegatenP's EraMTFont. Done: `gamefont.rs` (names from each font's `name` table, the font read when first used); GDRAWTEXT finds them through `resources::font_for`.
- [x] 3.2 Private-use glyphs of the game's font drawn as pictures in the text's colour where pictures show. Verify: render test (placements emitted for PUA cells); eraMegatenP's dungeon map by hand. Done: `Tty::glyph_cells`, pictures named `gamefont::picture_name` and made by `Resources::sprite` (`game_font_symbols_become_pictures`).
- [x] 3.3 Without pictures, GETFONT / GETCONFIG("フォント名") report the terminal's font when the configured one comes from `font/`. Verify: inline-game test; eraMegatenP's map prints its emoji tiles with `--gfx none`. Done: `Vm::reported_font` (`tests/emuera_net.rs`); eraMegatenP headless reports ＭＳ ゴシック and `CSFONT_CONVERT` keeps its text tiles.

## 4. setting.json

- [x] 4.1 Read `setting.json` (game folder); `UseRenameInCharaCSV`, `UseScopedVariableInstruction`, `CheckUTF8withBOM`. Verify: inline-game tests. Done: `loader::net_settings` (the game's folder or the one above); `UseRenameInCharaCSV` renames Chara*.csv (`rename_in_chara_csv`; eraMegatenP's character 9500 now gets its hat, 6, not 0); `CheckUTF8withBOM` gives level-0 load warnings (`check_utf8_with_bom`); `UseScopedVariableInstruction` needs nothing, VARI / VARS always work (a deliberate difference, `doc/differences.md`).
- [x] 4.2 `UseNewRandom` (Emuera.NET's other RNG): check what eraMegatenP sets (false) and implement only if a game turns it on. Done: no local game turns it on (13 setting.json files in era-games and era-archive, 333 folders); not implemented.
- [x] 4.3 Tell epizo12345's fork apart from Emuera-EE's .NET builds, which share the name and the marker files: AngleSharp inside the exe or `Emuera.dll` (`loader::ships_net_fork`, looked up when a div first needs it; `net_fork_on_local_games`).

## 5. eraMegatenP screens

- [ ] 5.1 Dungeon party block (portraits, names, HP) as Emuera.NET shows it. Needs a current-version dungeon save.

## 6. Docs

- [ ] 6.1 Wiki *Implemented* (Emuera.NET section), *Pictures and Graphics* (game fonts, glyph pictures), `doc/differences.md`.
