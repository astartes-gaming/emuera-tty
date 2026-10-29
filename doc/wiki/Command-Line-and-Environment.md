## Command-line options

```sh
emuera-tty [GAME_DIR | GIT_URL] [options]
```

| Option | |
|---|---|
| `--choose` | open the game picker even if a game is remembered |
| `--clone URL` | clone a game's git repository in the launcher and select it (a URL as `GAME_DIR` does the same) |
| `--gfx auto\|kitty\|iterm\|sixel\|fb\|none` | image backend (also `EMUERA_GFX`) |
| `--check` | parse the game, list errors, exit |
| `--headless` | plain stdin/stdout mode (scripts, tests); `--echo` shows typed input |
| `--strict` | unimplemented commands are errors instead of warnings |
| `--game-debug` | enable `[IF_DEBUG]` blocks and `;#;` lines |
| `--vm-debug` | enable interactive debugger (breakpoints, step, watches) |
| `--colors truecolor\|256\|16` | colour depth, when detection guesses wrong (also `EMUERA_COLORS`) |
| `--ambiguous wide\|narrow\|single\|osc66` | how box-drawing and symbol characters get Emuera's two columns (also `EMUERA_AMBIGUOUS`) |
| `--translate-log FILE` | append every new translation to `FILE`, skipping rows it already has (see [A log for translators](Translation#a-log-for-translators)) |
| `--translate-load FILE` | read translations from a translation log, before any translator is asked; repeat for several (see [Reusing a log as translations](Translation#reusing-a-log-as-translations)) |
| `--translate-storage FILE` | both of the above, with the one file |
| `--translation-filter FILE` | script files and lines of text that are never translated; repeat for several (see [Keeping files and lines untranslated](Translation#keeping-files-and-lines-untranslated)) |
| `--libretranslate HOST:PORT` | the LibreTranslate server (default `127.0.0.1:5000`; also `EMUERA_LIBRETRANSLATE`) |
| `--retro[=line]` | terminfo-driven ASCII frontend for retro terminals (auto for non-ANSI `$TERM`) |
| `--term NAME` | terminfo name of the terminal (default `$TERM`, `vt100` with `--serial`) |
| `--serial DEV` | play on a serial terminal; `--baud N`, `--serial-format 8N1`, `--flow xon\|rts\|none` |
| `--size COLSxROWS` | screen size when the terminal can't report it |
| `--saves` / `--backup-saves` | list the game's saves / back them all up |
| `--install` / `--upgrade` / `--uninstall` / `--reset-data` | see the guide |
| `--manual` | the full manual page |

`--colors`, `--ambiguous` and `--libretranslate` set the matching
environment variable for the run, so they win over the variable and over
the saved setting. A value can also be written `--colors=256`.

### Environment variables

- `FRAMEBUFFER=/dev/fbN` picks another framebuffer.
- `EMUERA_MOUSE=mice|off|term` overrides the console-mouse choice (see [Mouse](Terminals#mouse)).
- `EMUERA_GAMEPAD=off|/dev/input/eventN` turns gamepads off or reads just one device as a pad (see [Gamepads and the Steam Deck](Gamepads-and-Steam-Deck)).
- `EMUERA_AMBIGUOUS=wide|narrow|osc66` (or `--ambiguous`) overrides how box-drawing and symbol
  characters are fitted to Emuera's two columns (detected at startup:
  terminals that draw them wide need nothing, others are padded; `osc66`
  tries kitty's text-sizing protocol instead, which some may prefer).
- `EMUERA_MEASURE=0` skips what emuera-tty does at startup: measuring how wide the terminal
  really draws those characters (terminals' width tables differ), which keeps maps
  aligned in Konsole, kitty and others.
- `EMUERA_TRACE` logs calls (see [When something goes wrong](Troubleshooting)).
- `EMUERA_THREADS=N` caps the thread count game scripts are read on.
- `EMUERA_STACK_MB=N` sets the script thread's stack (default 512; a system
  that allows less gets the most it allows). Scripts may nest one call per
  16 KB of it, up to 4,000.
- `EMUERA_TIMING=1` prints how long each loading phase took and the memory
  in use after it.
- `EMUERA_SEED=N` fixes the random
  number seed, so a run can be repeated exactly (tests, bug reports).

The variables for debugging are under [Debugging](Debugging).

### Speed

Game scripts are read on every CPU core (AnonTW
loads in 0.3 s). Running the script itself is one
thread by nature (every line may read what the previous one wrote), but
pictures a screen needs are decoded and scaled on all cores. The
interpreter resolves each variable once (a reference remembers its global
or per-function slot), jumps straight to the matching CASE of a
SELECTCASE made of constants, and avoids copying strings when comparing,
calling functions or querying DataTables: on AnonTW's shop screens it runs
about three times faster than the first versions.
