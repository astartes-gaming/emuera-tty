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
listed here; they're gaps (README, *Implemented* and *Roadmap*).

Each entry: what, what Emuera does (with the C# file and line), what
emuera-tty does (with the Rust function), and why.

Differences made before this file was started were never collected; add
them here when you come across one.

| What | Emuera-EM/EE | emuera-tty | Why |
|---|---|---|---|
| A statement a `#FUNCTION(S)` may not use (input, CALL, BEGIN, PRINTW…) | a load warning for each such line in every function it analyses, then a script error if the line runs (`ErbLoader.setArgument`, `ErbLoader.cs:923`) | the same script error when it runs, reported when the function is first compiled; `--check` lists them all (`parser.rs`, `NOT_IN_FUNCTION` in `vm/builtins.rs`) | Bodies are compiled lazily, to fit in 512 MB: there is no load pass over every line to warn from |
| `VARS = …` / `VARI = …` (a variable the game declared itself, e.g. `#DIMS VARS`, assigned) | always EE's `VARS`/`VARI` declaration: the name before `=` is empty, so a nameless private variable is declared and set, and the game's own `VARS` is left untouched (`LogicalLineParser.cs:421`) | an assignment to the game's variable when no name follows the command (`"VARI" \| "VARS"` in `parser.rs`) | Vanilla Emuera and Anchor have no such commands and assign; eraAS (`RPG_BATTLE.ERB`, `#DIMS VARS`) is written that way |

## Helpers that come with games

Some games ship a separate Windows program next to Emuera. Where
emuera-tty does that program's job itself, the differences are here.

| What | The helper | emuera-tty | Why |
|---|---|---|---|
| eraMidi (`eramidi.exe`, eratohoReverse's music) | polls the clipboard, which Emuera-Anchor fills with printed lines when its clipboard option is on; for text with a ♪ it takes what follows up to the next ♪ and plays `media/reverse/<that>.mid` with BASS whenever that name differs from the last one seen (`clipboard_TextBox_TextChanged` in its IL), so a song named again after a line of dialogue with a ♪ starts over | reads its own printed lines (no clipboard setting needed) in a game folder with `eramidi.exe`, same rule, and plays through FluidSynth with the game's `soundfont.sf2`; a song already playing isn't restarted (`sound::eramidi_song`, `Vm::raw_input`) | The dialogue is full of ♪ ("Fufufu♪"), so eraMidi restarts the music whenever a character's theme is shown again; nothing in the game relies on that |
