## emuera-tty's settings (F3)

F3 or Ctrl-O in a game (F3 unless a keyboard macro is stored on it), or *⚙
Settings* in the launcher. ↑ / ↓ choose, ← / → or Enter change, Esc closes;
changes are saved at once in `~/.config/emuera-tty/config` and, except
where marked, apply to the running game:

| Setting | Values | |
|---|---|---|
| Images | auto, kitty, iterm, sixel, fb, none | how pictures are drawn; `none`: pictures are never decoded or scaled (and those already held are freed) until another choice is made |
| Colours | auto, truecolor, 256, 16 | colour depth sent to the terminal |
| Readable colours | normal, high, off | lighten coloured text that is too dark for its background |
| Wide symbols | auto, wide, narrow, single, osc66 | how symbols get Emuera's two columns |
| Backslash as ¥ | auto, on, off | draw `\` in game text as ¥, as Japanese fonts do; `auto`: when the game's font is one (MS Gothic, Emuera's default, MS Mincho, Meiryo), so eraTW's `\0` money shows `¥0` |
| Greek and Cyrillic | auto, wide, narrow | how many columns Greek and Cyrillic letters in game text take: Emuera's default font, MS Gothic (and MS Mincho), draws them full-width, two columns each, and games written for it line up that way; other fonts draw them narrow. `auto`: as the game's font (two columns when it sets none); `narrow` reads better for text written in those scripts |
| Copy to clipboard | game, on, off | printed text to the clipboard (see [Copying text to the clipboard](Playing#copying-text-to-the-clipboard)); `game`: as emuera.config says |
| Bottom bar details | on, off | hovered button's value, running function |
| Keyboard pointer | auto, on, off | cursor keys move a pointer, Insert / Shift+Enter click |
| Gamepad | auto, off | read gamepads (Steam Deck, Xbox, PlayStation…; Linux), see [Gamepads and the Steam Deck](Gamepads-and-Steam-Deck) |
| Show memory | off, on | memory in use in the bottom row too (always on the settings title) |
| Translator | off, auto, local, argos, translate-shell, deepl, file | off: no translation; auto: whichever is installed, this computer's first; file: no translator, only the translations read with `--translate-load` (see [Reusing a log as translations](Translation#reusing-a-log-as-translations)) |
| Translate to | en, es, pt, fr, de, it, ru, ja, zh, ko | show the game in that language (see [Translating the game](Translation#translating-the-game)) |
| Translate from | auto, ja, en | the game's language; auto: per line |
| Translate what | auto, dialog, all | characters' dialog, or all text; auto: all unless into English |
| Sound | auto, on, off | PLAYSOUND / PLAYBGM; `auto`: not over SSH |
| Wheel scrolls | 3, 1, 5, 10 | lines per wheel step |
| Console mouse | auto, off | gpm / `/dev/input/mice` (next start) |
| Display | auto, ansi, retro | `retro`: the ASCII terminfo frontend (next start) |

The overlay shows the *Random seed* in use: selectable for its description,
but read-only (start with `EMUERA_SEED=N` to choose it; the game's
`RANDOMIZE` / `INITRAND` change it). *Environment variables…* (shown only
when one is set) lists every `EMUERA_*` variable emuera-tty reads with the
value it sees (● set, ○ not set; Enter says what one does; read-only, as
they are read at start). It also has *Game settings…* and *Manage saves…*
(see [Game settings](#game-settings-emueras-settings-window) and [Managing
saves](Playing#managing-saves)), *Clear the picture cache* (frees the
decoded sprites whatever *Images* says; they load again when shown),
*Recolor…* (draws a colour of the game's as another, written `from:to` in
hex, comma separated: `01183c:111d3b` turns AnonTW's time-stop navy into a
softer blue; the game still sees its own colours; also `EMUERA_RECOLOR`),
*Switch game…* and *Quit*. Command-line options and `EMUERA_*` variables
override saved settings.

### Readable colours

Games are drawn for Emuera's black (or game-set)
background, and some colours there are hard to read, like pure blue, or
AnonTW's blue text on the dark navy background of time stop. emuera-tty
lightens coloured text until it reaches a minimum contrast (WCAG 3:1, or
4.5:1 on *high*), keeping its hue. Greys are left as dim as the game made
them, since games use them for shading, but no dimmer: where the game sets
no background, your terminal's own shows (emuera-tty asks the terminal for
it), and a grey gets at least the contrast it had on Emuera's black, so
AnonTW's dark `C_GRAY` stays readable on a dark grey theme. The window
background set by `SETBGCOLOR` covers the whole screen, as in Emuera.

## Game settings (Emuera's settings window)

*Game settings…* in the settings overlay is a copy of Emuera's own settings
window, with the settings that do something in a terminal and can't
break a game, on Emuera's tabs:

| Tab | Settings |
|---|---|
| Environment | make autosaves, lines in history log, number of save slots (20–80: the save menu's pages, `SAVENOS`), milliseconds for infinite loop warning (0 = disabled, default 5000) |
| Display | PRINTC items per line and width, don't break lines inside a button |
| Font | text colour, selected button colour, the font for text drawn into pictures |
| System2 | compress save data (Emuera reads compressed saves too) |
| Clipboard | all of the clipboard options (see [Copying text to the clipboard](Playing#copying-text-to-the-clipboard)) |

Left out: what only a window has (size, position, FPS, menu bar,
keyboard macros), what changes how the scripts are read (the System,
Compatibility and Debug tabs), the font size and line height (games work
out where pictures go from them), the save folder (existing saves would
seem to vanish), the background and history colours (the terminal's own
are used) and Rikai.

← / → (or Tab, or a click) switch tabs, ↑ / ↓ choose, Enter or a click
toggles a check box or asks for a number, a colour (`R,G,B` or `#RRGGBB`)
or text, `+` / `-` step a number, `d` puts back the game's value. Each
setting shows where its value comes from: nothing for Emuera's default,
*game* for the game's (`CSV/_default.config`), *changed* for yours,
*fixed* when `CSV/_fixed.config` doesn't let it change. Changes go into
the game's `emuera.config` as Emuera's settings window writes them
(UTF-8, Japanese or English keys as the game's files use, other lines
kept), so Emuera.exe sees them too; the running game takes them when the
page closes.
