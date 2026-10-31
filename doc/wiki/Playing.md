Emuera games are menus of numbered choices like `[100] Wake Up`.

## Controls

- **Choose**: type the number and press Enter, or **click** it. Buttons
  under the mouse are highlighted; a picture that is a button can be
  clicked anywhere on it. Resting the mouse on a button with a tooltip
  shows it after half a second (or the game's delay).
- **Continue** (when the game waits after text, `▼ (Enter)` at the
  bottom): Enter, any key, or a click.
- **Skip to the next choice**: right-click (or Tab) where the game waits:
  the text that follows runs by without stopping until the game asks for
  a number or a choice, as Emuera's right click does. A right click never
  picks a choice: only a left click does, as in Emuera. (A `FORCEWAIT`
  still stops, and the EE `INPUT default, mouse, canskip` takes its default.)
- **Typing**: ← / → move in the line being typed, Home / End go to its
  start and end, Delete and Backspace erase, Ctrl-U or Esc clear it.
  ↑ / ↓ go through your last 100 inputs, as in Emuera: typed ones and
  buttons clicked alike. One brought back is shown selected, so typing
  replaces it and ← / → edit it.
- **Macros**: one typed line can answer several questions, and F1…F12 keep lines for
  later; see [Macros](#macros) below.
- **Scroll back** through earlier text: PgUp / PgDn, Ctrl+↑ / ↓ (or
  Alt+↑ / ↓) a line at a time, or the mouse wheel. ↑ / ↓ scroll too
  where the game waits (`▼`), and Home / End while nothing is typed. A
  `[scroll -N]` tag shows you're looking at history; End jumps back.
  **Click either the left/right mouse buttons or press Enter while scrolled back to jump
  immediately to the end** (as in Emuera).
- **Settings**: F3 or Ctrl-O opens them over the game (see [Settings](Settings#emuera-ttys-settings-f3)); F3
  not while a keyboard macro is stored on it.
- **Back to the title**: Ctrl-T. **Restart** the game: Ctrl-R (both ask
  first; unsaved progress is lost). **Reload the ERB files** after editing
  them: Ctrl-E or F5 (unless a macro is stored on F5): the game carries on where it was
  with the new code (Emuera-Anchor's hotkeys, as in Emuera-EE; also in
  the settings). *Restart in debug mode* in the settings restarts with
  `--game-debug` (Emuera's debug mode: `[IF_DEBUG]` blocks and `;#;` lines run),
  and *Restart without debug mode* turns it off again.
- **Redraw** a garbled screen: Ctrl-L. **Quit**: Ctrl-C (save first!).
  Ctrl-C also works while the game is busy computing, so a script stuck
  in a loop can always be stopped; the screen keeps updating meanwhile.
- **The bottom row** shows, on the right, the number the button under the
  mouse sends (`→ 105`, also for buttons that don't show it, like map
  squares) and the function asking for input. While a script computes
  for more than a moment it says `… working @FUNCTION`. (*Bottom bar
  details* in the settings turns this off.) The memory emuera-tty uses is
  on the settings page's title, and in the bottom row too with *Show memory*
  (Linux, Android and Windows); both also show the highest use seen this
  session once it is above the current one (`123 MB (peak 150 MB)`).

## Macros

Emuera's keyboard macros: one typed line can answer several questions.

- **Syntax**: `\n` separates the answers, `\e` after one skips the
  text that follows it (as a right click), and `(…)*n` repeats:
  `(0\e\n)*3` answers `0` three times, skipping after each. Esc stops a
  macro that is running.
- **Keys**: Shift+F1…F12 store the line being typed under
  that key, and F1…F12 put it back (at a `▼` wait, the key gives it at
  once). Alt+0…9 (also Ctrl+0…9 in terminals that report it, such as
  kitty) switches between ten groups of twelve keys, and the group's name
  shows for a moment. On the
  Linux console Shift+F1 / F2 are F11 / F12, and Shift+F11 / F12 don't
  exist.
- **Where they are kept**: the game folder's `macro.txt`,
  the file Emuera.exe uses, so macros made in either work in both.
- **The setting**: *Use keyboard macros* (on by default, as in
  Emuera) gives the F keys to the macros, but F3 and F5 stay the settings
  and reload keys as long as no macro is stored on them (where Emuera
  would only empty the line); with it off they always are. Typed text is
  expanded either way, as in Emuera.

## Sound

PLAYSOUND / PLAYBGM play the game's `sound/` files through a
player program the system has: `mpv` or `ffplay` (any format, including
the `.m4a` many games use), `mplayer`, `afplay` on macOS, or for WAV
only `pw-play`, `paplay`, `aplay` and PowerShell on Windows. Install mpv
for full sound (`pkg install mpv` on Termux). Ten effect channels and a
looping BGM, as Emuera-EE; volume changes apply to sounds started after
them. *Sound* in the settings turns it off and on while you play (the BGM picks
up again); it is off over SSH unless set to `on` (the sound would play on
the remote machine). `EMUERA_SOUND=on|off` and `EMUERA_SOUND_PLAYER=mpv`
override.

### MIDI music

MIDI music plays through [FluidSynth](https://www.fluidsynth.org),
found in `PATH` like the other players, with the game's `soundfont.sf2`
if it has one, else the system's:

- Linux: `sudo apt install fluidsynth fluid-soundfont-gm` (Fedora:
  `sudo dnf install fluidsynth fluid-soundfont-gm`).
- Windows: download the Windows zip from
  [FluidSynth's releases](https://github.com/FluidSynth/fluidsynth/releases),
  extract it, and add its `bin` folder to your `PATH` (*Edit environment
  variables for your account* → `Path` → *New*); open a new terminal
  afterwards.
- macOS: `brew install fluidsynth` (untested).

Games that don't bring a SoundFont need one on the system; on Windows and
macOS emuera-tty finds none by itself, so MIDI there plays only with a
game's own `soundfont.sf2`. Games that come with
**eraMidi** (`eramidi.exe`, as eratohoReverse does) get their music
without it: eraMidi watches Emuera-Anchor's clipboard for a line with a ♪
(the song title the game shows) and plays `media/reverse/<title>.mid`;
emuera-tty reads its own printed lines the same way, so no clipboard
setting is needed. The song loops until another one starts; one that is
already playing isn't restarted.

## Saving and loading

Saving uses the game's own save menu, and the files are Emuera's own
(binary) format: `save00.sav`, `save01.sav`, … and `global.sav`, plus
`dat/chara_*.dat` for SAVECHARA.

- **Both ways**: saves move between emuera-tty
  and Emuera on Windows: loading an Emuera save and saving it again gives
  the same bytes Emuera writes (the variables in Emuera's order, character
  names never set left out as Emuera does), checked on AnonTW saves of
  day 43 and day 1336.
- **Location**: where Emuera puts them: in `sav/` when
  the game's config says so (`セーブデータをsavフォルダ内に作成する`, as
  AnonTW's does), else next to the game; compressed when it asks for that
  (`セーブデータを圧縮して保存する`).
- **Also saved**: Emuera-EM maps, XML documents and data
  tables named in `VarExt.csv`; the autosave runs on
  entering the shop, like Emuera's.
- **Older and text saves**: older emuera-tty saves (`sav/*.rsav`)
  still load, and the next save of that slot writes a `.sav`. Saves in
  Emuera's text format (games whose config turns the binary option off)
  load too; saving always writes the binary format, which Emuera reads
  whatever its config says.
- **After a load**: `@SYSTEM_LOADEND` runs and then
  `@EVENTLOAD`, as in Emuera.

## Managing saves

Ctrl-O → *Manage saves…* lists the game's saves with their date and the
comment Emuera's save menu shows. Pick one to **copy** or **move** it to
another slot, **export** it to a file (by default
`~/emuera-saves/<game>-saveNN-<date>.sav`) or **delete** it. For the whole
folder: **back up all saves** (into
`~/.local/share/emuera-tty/backups/<game>/<date>/`), **restore a backup**
(the current saves are backed up first; *Another folder…* restores from
any folder of saves, such as Emuera's `sav/` on another computer) and
**import a save file** into a slot. Nothing is lost by mistake: a deleted
or overwritten save is copied to the backups first. Load a restored or
imported save from the game's own load menu.

Export, import and *Another folder…* open a file browser, as *Add a game*
does in the launcher: it lists the folders and the `.sav` files with
their comments, and opens where it was last used (else `~/emuera-saves`).
Enter opens a folder or picks a file, ← or Backspace goes up, `~` goes
home, `.` shows hidden folders, `/` types a path instead, Esc goes back;
the mouse works too. The first row, **▶**, picks the folder shown: export
then asks for the file name. From a shell: `emuera-tty GAME --saves` lists the saves
and `emuera-tty GAME --backup-saves` backs them up.

### Save history

Saves the game writes (save slots, autosaves,
`global.sav`, SAVECHARA files) are protected too, with no setting to turn
on:

- **The version replaced is kept**, in the same backup folder, as
  `<date>-auto-save01/`, so *restore a backup* can bring back any of them.
  Copies are thinned: the last 10 of each file, then one a day for 30 days,
  then one a week for 26 weeks. Saves are small (AnonTW's are under 1 MB),
  so this stays at a few MB per game.
- **A save emuera-tty didn't write** (one from Emuera.exe, or from before
  this history) is kept as `<date>-original-save01/` the first time it is
  replaced, and never thinned.
- **Each save is written safely**: to a temporary file, flushed to disk,
  read back and checked to load, and only then put in place. A crash,
  power cut or full disk leaves the previous save; a save that wouldn't
  load is reported at once and the previous one kept.
- **`save-log.tsv`** in the backup folder records each save written: when,
  which file, a fingerprint, its size, and the emuera-tty version and
  commit that wrote it. If a bug turns up in some build, the log tells
  which saves it wrote and which backup came before.

`EMUERA_SAVE_HISTORY=0` turns the history and the log off (saves are still
written safely). Nothing is added inside the saves: Emuera reads them as
before.

## Copying text to the clipboard

For translation tools and text hookers, as Emuera-EE's clipboard feature
(from Emuera-Anchor): with `表示したテキストをクリップボードにコピーする:YES`
in `emuera.config` (or *Copy to clipboard: on*), the lines the game printed
since the last copy go to the clipboard when the game waits for input (and,
as configured, on WAIT, a left, middle or double click). The other
emuera.config options apply too: new lines only or the last *N*
(`クリップボードに貼り付ける行数`), the buffer size, the minimum time between
copies, and replacing pictures with a string (`テキスト中の<>タグを無視する`,
`<>を次の文で置き換える`). The copy is an OSC 52 escape, which kitty, WezTerm,
foot, Windows Terminal, iTerm2 and tmux (`set -g set-clipboard on`) pass to
the system clipboard; Konsole and some others need it allowed in their
settings.
