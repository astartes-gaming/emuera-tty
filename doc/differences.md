# Deliberate differences from Emuera-EM/EE

Where emuera-tty does something other than Emuera-EM/EE
(`../emuera.em`) on purpose, and why, for future reference. Differences
from Emuera-Anchor's translation system are in
[anchor.md](anchor.md#deliberate-differences).

The rule: copy Emuera, bugs included, where not doing so would make a
game not work, or not work as expected, because the games are written
against it. Differ to fix a bug or do the right thing, as long as saves
stay compatible both ways and nothing the games rely on in Emuera breaks.
Things that differ only because they aren't implemented yet aren't
listed here; they're gaps (the wiki's *Implemented* and *Roadmap* pages, `doc/wiki/`).

Each entry: what, what Emuera does (with the C# file and line), what
emuera-tty does (with the Rust function), and why.

Differences made before this file was started were never collected; add
them here when you come across one.

| What | Emuera-EM/EE | emuera-tty | Why |
|---|---|---|---|
| A statement a `#FUNCTION(S)` may not use (input, CALL, BEGIN, PRINTW…) | a load warning for each such line in every function it analyses, then a script error if the line runs (`ErbLoader.setArgument`, `ErbLoader.cs:923`) | the same script error when it runs, reported when the function is first compiled; `--check` lists them all (`parser.rs`, `NOT_IN_FUNCTION` in `vm/builtins.rs`) | Bodies are compiled lazily, to fit in 512 MB: there is no load pass over every line to warn from |
| PuchiEmuera's FADE, SHAKE, FONTEDGE, FONTEDGE_COLOR, RESET_FONTEDGE_COLOR, OUTPUTHINTS (eraSumireTeru's engine, `PuchiEmuEra_説明書.pdf`) | unknown names: an assignment that fails to parse, a script error when it runs | the effects are skipped, OUTPUTHINTS copies `resources/HINT/*.dat` next to the game as `.txt`; a line assigning to one of the names stays an assignment (`PUCHI_COMMANDS` in `vm/builtins.rs`) | eraSumireTeru uses them in 237 lines, so most of its scenes stopped; a terminal can't fade, shake or outline text anyway |
| `VARS = …` / `VARI = …` (a variable the game declared itself, e.g. `#DIMS VARS`, assigned) | always EE's `VARS`/`VARI` declaration: the name before `=` is empty, so a nameless private variable is declared and set, and the game's own `VARS` is left untouched (`LogicalLineParser.cs:421`) | an assignment to the game's variable when no name follows the command (`"VARI" \| "VARS"` in `parser.rs`) | Vanilla Emuera and Anchor have no such commands and assign; eraAS (`RPG_BATTLE.ERB`, `#DIMS VARS`) is written that way |
| File order with 読み込み順をファイル名順にソートする (which of `A.ERB` / `A_B.ERB` loads first, so wins a function both define, and comes first for event functions and `#DIM` order) | `Array.Sort` with .NET's culture order (`Config.getFiles`, `Config.cs:402`): ICU on .NET 10, so `_` before `.` | ICU order, except for a game that ships only .NET Framework builds of Emuera (no `.NETCoreApp` in its .exe files): then Windows' NLS order, `.` before `_`, hyphens skipped (`loader::Listing::of`, `name_order`) | Games were written on the Emuera they ship. eraSumireTeru (PuchiAnchor) says its COMABLE.ERB hides COMABLE_DUMMY.ERB's functions; in ICU order a dummy `@COM_ABLE107` lets a command run that jumps to a missing function. EraMeiQGaiden (Emuera1821, Anchor) has 33 folders where the orders differ; eraAL-K ships EE on .NET 7 and keeps ICU |
| Greek and Cyrillic letters with MS Gothic or MS Mincho (or no font set), "Greek and Cyrillic" setting `auto` | each drawn as the font measures it, full-width, so a Russian line reads `П р и в е т` (`StringMeasure.cs:66`) | a letter on its own keeps two columns; letters next to each other take one each (`letter_widths` in `tty.rs`) | Japanese games line up lone letters as MS Gothic draws them (the `Д` of `(´Д｀)`, a `Σ` in a stat line), so those stay wide; a word in those scripts, as in a translated game, can't be read spaced out and lines up with nothing. `wide` gives Emuera's widths |

## Helpers that come with games

Some games ship a separate Windows program next to Emuera. Where
emuera-tty does that program's job itself, the differences are here.

| What | The helper | emuera-tty | Why |
|---|---|---|---|
| eraMidi (`eramidi.exe`, eratohoReverse's music) | polls the clipboard, which Emuera-Anchor fills with printed lines when its clipboard option is on; for text with a ♪ it takes what follows up to the next ♪ and plays `media/reverse/<that>.mid` with BASS whenever that name differs from the last one seen (`clipboard_TextBox_TextChanged` in its IL), so a song named again after a line of dialogue with a ♪ starts over | reads its own printed lines (no clipboard setting needed) in a game folder with `eramidi.exe`, same rule, and plays through FluidSynth with the game's `soundfont.sf2`; a song already playing isn't restarted (`sound::eramidi_song`, `Vm::raw_input`) | The dialogue is full of ♪ ("Fufufu♪"), so eraMidi restarts the music whenever a character's theme is shown again; nothing in the game relies on that |
