# Design

## Context

Emuera.NET (github.com/epizo12345/emuera_Fork, checked out beside emuera-tty as `../emuera_Fork`) is a separate fork from Emuera-EM/EE, built on .NET 10 with SkiaSharp rendering and AngleSharp HTML parsing. Its game, eraMegatenP, ships `Emuera.dll`, `Emuera.runtimeconfig.json`, `Emuera.NET <version>.profile` files and `setting.json`; its `emuera.config` sets `フォント名:EraMTFont`, a font in the game's `font/` folder.

Differences found so far:
- Built-ins: 25 functions EM lacks (`DICT_*`, `SQL_*`, `G_POLYGON_*`, `GETCSVNOBY*`, `HASH_XXH3(2)`, `CHKVARDATA`…) and the statement `MATCHALL`; eraMegatenP uses `GETCSVNOBYNICKNAME`, `HASH_XXH32`, `MATCHALL`.
- HTML: `<div>` width/height optional (auto-size to content), its own attribute names, `display` absolute modes; lines start LEFT whatever ALIGNMENT says (EM too).
- Fonts: every `.ttf`/`.otf` in `font/` is loaded and known by family and file name (UI/FontFactory.cs).
- `setting.json`: `UseNewRandom` (a different RNG), `UseRenameInCharaCSV`, `UseScopedVariableInstruction` (VARI/VARS), `CheckUTF8withBOM`, plus display-only switches.

## Goals / Non-Goals

**Goals:** eraMegatenP plays as on Emuera.NET; other Emuera.NET games get what they use; nothing changes for EM/EE games.

**Non-goals:** the built-ins no local game uses (SQL, DICT, polygons) until one does; Emuera.NET's Web packager; its WinForms debug UI.

## Decisions

### No engine switch, where possible

Each Emuera.NET addition is something EM rejects (an attribute name, a div without size, an unknown built-in) or reads a file EM games don't have (`setting.json`, `font/`). So they are accepted for every game rather than behind a detected "this is an Emuera.NET game" mode. Where the two engines would disagree on the same input, detection (the shipped files in the table above) decides, and the difference goes in `doc/differences.md`.

### Glyphs only the game's font has: pictures, else the game's fallback

eraMegatenP's dungeon tiles are private-use characters of EraMTFont, coloured like text. A terminal can't draw them from its own font.

- When the game's configured font comes from its `font/` folder and the terminal shows pictures, a character the font has in the private-use areas (U+E000–U+F8FF, planes 15–16) is drawn as a picture of that glyph, in the text's colour, filling its cells (one cell, or two where Emuera measures it full-width). Built through the picture pipeline as a generated sprite, cached by (character, colour, cell size).
- When the terminal can't show pictures, GETFONT and GETCONFIG("フォント名") report the terminal's font, not the game's, so a game that checks for its font (eraMegatenP's `MTFONT_CONVERT`, `UPPER_PLANE_MIGRATE`) uses its own text fallback. The font is decided at start-up, as GETFONT is read once per screen.

Alternatives: always the fallback (works everywhere, but loses the tiles where they could show); lookalike characters (no good lookalikes for tiles).

### HTML div auto-size

A div without width uses its content's widest line (plus padding), without height its line count, as ConsoleDivElement does; emuera-tty used the console width.

## Risks / Trade-offs

- Many small glyph pictures per screen (a dungeon map is ~300 cells) → one picture per distinct (glyph, colour); the kitty backend reuses ids. If it's slow, merge a row's glyphs into one picture.
- Reporting another font to GETFONT changes what a game computes → only when the game's font is from `font/` and pictures are off.
