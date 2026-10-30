# Environment variables

Every environment variable emuera-tty uses. The `EMUERA_*` ones are its own;
the rest are standard ones it reads (or, where noted, sets). Command-line
options and `EMUERA_*` variables override the saved settings. Where a
variable has an option of the same purpose, it is named in the table, and
the option wins over the variable. The translation log is set with options,
not variables: `--translate-log`, `--translate-load` and
`--translate-storage` (and `--translation-filter` for what is never translated).

| Variable | Values | What it does |
|---|---|---|
| `EMUERA_AMBIGUOUS` | `wide`, `narrow`, `single`, `osc66` | How box-drawing and symbol characters get Emuera's two columns (option: `--ambiguous`). The *Wide symbols* setting sets it unless it is already set. |
| `EMUERA_CODE_MB` | number | MB that compiled functions may take (default: 1/16 of the memory). The least recently called are dropped. |
| `EMUERA_COLORS` | `truecolor`, `256`, `16` | The colour depth, when detection guesses wrong (option: `--colors`). The *Colours* setting sets it unless it is already set. |
| `EMUERA_COMMIT` | | Not read at run time: `build.rs` sets it to the git commit of the build, which goes in the save history log. |
| `EMUERA_DEBUG_FRAMES` | any value | Print each frame drawn (size and picture keys) to stderr. |
| `EMUERA_GBUF_MB` | number | MB that graphics buffers may take unpacked (default: 1/8 of the memory). |
| `EMUERA_GFX` | `kitty`, `iterm`, `sixel`, `fb`, `none` | Default for `--gfx`. |
| `EMUERA_GPMCTL` | path | The gpm socket (default `/dev/gpmctl`), for testing the Linux-console mouse. |
| `EMUERA_GPM_VC` | number | The virtual console the gpm mouse is read for (testing). |
| `EMUERA_INJECT` | `STMT\|\|STMT` | Statements run just before the `EMUERA_INJECT_AT`-th input. |
| `EMUERA_INJECT_AT` | number | The input number `EMUERA_INJECT` runs before. |
| `EMUERA_LANG` | `ja`, `zh-Hans`, `zh-Hant`, `ko`, `en`; `x-cjk`, `x-arab`, `x-cyrl`, `x-deva`, `x-avst` | The interface language, over the *Language* setting and the system locale. The `x-` values rewrite every message in that script, made longer, to check the layout. |
| `EMUERA_LIBRETRANSLATE` | `host:port` | The LibreTranslate server (default `127.0.0.1:5000`; option: `--libretranslate`). |
| `EMUERA_LINT` | any value | With `--check`: list statements the parser may have split wrongly, and every `>=` / `<=` between strings (Emuera evaluates both as `<`). |
| `EMUERA_LIST_BUTTONS` | `1` | With `--headless`: print the values of the buttons on screen before each input. |
| `EMUERA_LOG_STYLE` | `emuera` | Headless output with only the lines the game printed, as Emuera's log has them (used by the tests). |
| `EMUERA_MEASURE` | `0` | Skip measuring how wide the terminal draws ambiguous-width characters. |
| `EMUERA_MEMSTATS` | file | Write what the memory holds to this file at every input. |
| `EMUERA_MOUSE` | `off`, `term`, `mice` | `off` / `term`: no console mouse. `mice`: read `/dev/input/mice` even outside the Linux console. The *Console mouse* setting sets `off`. |
| `EMUERA_MOUSE_LOG` | file | Log the mouse events to this file (for a pointer that misbehaves). |
| `EMUERA_PARSE` | `all` | Compile every function at load instead of on its first call. |
| `EMUERA_PICTURES_MB` | number | MB that decoded picture files may take (default: 1/8 of the memory). |
| `EMUERA_PPROF` | file | Native profile, written on a clean exit (needs a build with `--features profile`). |
| `EMUERA_PROFILE` | file | Kept updated with which ERB function runs, sampled every millisecond. |
| `EMUERA_RECOLOR` | `from:to,…` | Draw a colour of the game's as another, in hex (`01183c:111d3b`). Wins over the *Recolor* setting. |
| `EMUERA_SAVE_HISTORY` | `0` | Turn the save history and its log off (saves are still written safely). |
| `EMUERA_SEED` | number | The number the random generator starts from (reproducible runs). |
| `EMUERA_SOUND` | `on`, `off` | Play the game's sounds (default: yes, except over SSH). Wins over the *Sound* setting. |
| `EMUERA_SOUND_PLAYER` | `mpv`, `ffplay`, `mplayer`, `afplay`, `pw-play`, `paplay`, `aplay`, `powershell` | The sound player to use. |
| `EMUERA_THREADS` | number | Cap on the threads used for loading and picture decoding. |
| `EMUERA_TIMING` | `1` | Print how long each loading phase takes and the memory in use. |
| `EMUERA_TRACE` | text | Log calls to functions whose name contains this, with their results, to stderr. |
| `EMUERA_TRANSLATE_CMD` | command | A command that reads a line on stdin and prints its translation, used instead of the built-in translators. |
| `EMUERA_TRANSLATE_FROM` | | Set by emuera-tty for `EMUERA_TRANSLATE_CMD`: the language of the text. |
| `EMUERA_TRANSLATE_MODEL` | name | The Ollama or translateLocally model used for translating. |
| `EMUERA_TRANSLATE_TO` | | Set by emuera-tty for `EMUERA_TRANSLATE_CMD`: the language to translate into. |
| `EMUERA_TRANS_ENGINE` | engine name | The engine translate-shell (`trans`) uses (default: Google). |
| `EMUERA_WARMUP` | `1`, `0` | Decode last session's pictures in the background at start (default: on, off when headless). The *Decode last used pictures* setting sets it unless it is already set. Disabling it can greatly reduce peak memory use at launch on small machines. |
| `APPDATA` | path | Windows: where settings go (`%APPDATA%\emuera-tty`), when `XDG_CONFIG_HOME` isn't set. |
| `COLORTERM` | `truecolor`, `24bit` | Tells emuera-tty the terminal does 24-bit colour. |
| `DEEPL_AUTH_KEY` | key | Your DeepL API key (a free key works). Without one, DeepL's keyless endpoint is used. |
| `FRAMEBUFFER` | path | The framebuffer device for pictures on the Linux console (default `/dev/fb0`). |
| `GHOSTTY_RESOURCES_DIR` | | Set by Ghostty. Inside tmux it tells emuera-tty the outer terminal draws kitty graphics. |
| `GIT_SSH_COMMAND` | command | Used by the git calls for cloning and updating games. If unset, `ssh -o BatchMode=yes` is used, so git never asks for a password. |
| `HOME` | path | The home folder: settings, games and `~/.terminfo`. |
| `KITTY_WINDOW_ID` | | Set by kitty. Inside tmux it tells emuera-tty the outer terminal draws kitty graphics. |
| `LC_TERMINAL` | `iTerm2` | Tells emuera-tty to use iTerm2's inline pictures. |
| `LOCALAPPDATA` | path | Windows: where games extracted from archives go (`%LOCALAPPDATA%\emuera-tty`), when `XDG_DATA_HOME` isn't set. |
| `PATH` | | Where emuera-tty looks for translators, sound players, `git`, `7z` and so on, and which folder `--install` puts the program in. |
| `SSH_CONNECTION`, `SSH_TTY` | | If either is set, *Sound* `auto` stays off (the sound would play on the remote machine). |
| `STY` | | Set by GNU screen: pictures can't be passed through it. |
| `TERM` | terminal name | Picks colour depth, recognises the Linux console and fbterm, chooses the retro frontend for non-ANSI terminals, and is the default `--term`. |
| `TERMINFO`, `TERMINFO_DIRS` | path(s) | Where the retro frontend looks for terminfo entries. |
| `TERM_PROGRAM` | `iTerm.app`, `WezTerm`, … | `iTerm.app` and `WezTerm` get iTerm2 inline pictures; also used to recognise tmux. |
| `TMUX` | | Set by tmux: pictures are sent through tmux's passthrough. |
| `USERPROFILE` | path | Windows: the home folder when `HOME` isn't set. |
| `WINDIR` | path | Windows: where the system fonts are read from (default `C:\Windows`). |
| `XDG_BIN_HOME` | absolute path | Where `--install` puts the program when no folder on `PATH` fits (default `~/.local/bin`). |
| `XDG_CONFIG_HOME` | absolute path | The settings folder is `$XDG_CONFIG_HOME/emuera-tty` (default `~/.config/emuera-tty`). |
| `XDG_DATA_HOME` | absolute path | Games extracted from archives, backups and the picture list are under `$XDG_DATA_HOME/emuera-tty` (default `~/.local/share/emuera-tty`). |
