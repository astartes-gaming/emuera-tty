- `EMUERA_TRACE=<part of a function name>` logs matching calls and
  their return values to stderr, with timestamps.
- `EMUERA_PROFILE=<file>`
  samples which ERB function is running every millisecond (time spent waiting
  for the player not counted) and keeps `<file>` updated with the top 60,
  under a first line telling which input the script is waiting for and the
  call stack that asked for it (handy when a game seems stuck).
- `EMUERA_INJECT="STATEMENT||STATEMENT"` with `EMUERA_INJECT_AT=<n>` runs those
  statements as a function `@__INJECT` just before the n-th input.
- `EMUERA_LIST_BUTTONS=1` makes `--headless` print the values of the buttons
  on screen before each input (`[buttons: 1001, 1002]`), hidden ones included,
  for scripted play; a comma inside a value is written `\,`.
- `EMUERA_LINT=1` with `--check` lists compiled statements that look like
  the parser split their line in the wrong place (a function name holding
  `(`, `,` or spaces; FORM text still holding `%`, `{` or `}`), for hunting
  parser bugs across a whole game. It also lists every `>=` and `<=` between
  two strings (and `CASE IS >=` / `IS <=` in a SELECTCASE on a string):
  Emuera evaluates both as `<`, a bug as old as vanilla Emuera, so these are
  the lines a fix would change.

## Emuera's load warnings

`emuera-tty GAME --check` compiles every function and reports what Emuera
reports while loading, in its words (`警告Lv2:ERB/X.ERB:12行目:…`), as the
game's settings ask (*Minimum warning level*, *Function is not found
warning*, *Function not called warning*, *Duplicated functions warning*,
*System function override warning*, *Check duplicate ERD identifier*,
*Display loading report*): calls to functions that don't exist, functions
nothing calls, functions defined twice, built-in functions the game
replaces. A game whose settings leave these at Emuera's defaults shows
none; set them in the game's `emuera.config` (or F3 → game settings) to
see them. The warnings the function headers alone tell (duplicates,
replaced built-ins) are also in the debug panel's console when the game
starts in debug mode.

## The debug panel

Emuera's debug window, for games run in debug mode (`--game-debug`, or
*Restart in debug mode* in the settings). At any input, **Ctrl-D** (or
`@DEBUG` typed at the input) opens it over the game; Esc or Ctrl-D closes
it. Tab moves between its three tabs, Emuera's:

- **Variable watch**: expressions and their values, worked out again
  whenever the panel opens or the list changes. Enter edits the selected
  one (the empty last row adds one), `a` adds, `d` removes, `r` refreshes.
  An expression can be anything the game could compute: `FLAG:3`,
  `CFLAG:TARGET:好感度`, a `#FUNCTION`'s result.
- **Stack trace**: the line the game is waiting at, then the calls that
  led there, newest first, in Emuera's words (`CALL :@FUNC:file:line行目`).
- **Console**: what DEBUGPRINT, DEBUGPRINTL, DEBUGPRINTFORM(L) printed
  (DEBUGCLEAR empties it), and a line to type debug commands in: a
  statement runs (`FLAG:3 = 5`, `ADDCHARA 1`), anything else is an
  expression whose value is printed. As in Emuera, flow control, waiting
  for input, what a `#FUNCTION` can't do and SAVEDATA, PUTFORM, UPCHECK,
  CUPCHECK are refused. ↑↓ recall earlier commands.

The watch list is kept in the game's `debug/watchlist.csv` and the console
in `debug/console.log` (on closing), both as Emuera writes them, so they
carry over to and from Emuera. The same commands can be typed at the
game's own input with `@` in front when the game's *Allow debug commands*
is on (see [Playing](Playing)).

## Interactive Debugger

Enable with `--vm-debug` flag or `EMUERA_VM_DEBUG=1` environment variable.
Game-level debug (`[IF_DEBUG]` blocks, `;#;` lines) is enabled with
`--game-debug` flag or `EMUERA_GAME_DEBUG=1`. When the debugger stops (at a
breakpoint or step), a REPL prompt appears: `(emu-debug)> `. Available
commands:

| Command | Aliases | Description |
|---------|---------|-------------|
| `continue` | `c` | Resume execution |
| `step [n]` | `s`, `si`, `stepi` | Step into next `n` instructions (default 1) |
| `next [n]` | `n` | Step over next `n` instructions in this or caller frame (default 1) |
| `finish [n]` | `f` | Step out until `n` functions return (default 1) |
| `backtrace` | `bt`, `where` | Show call stack |
| `print <var>` | `p` | Print variable value |
| `watch <expr>` | `w` | Add watch expression (evaluated at each stop) |
| `unwatch <n>` | `uw` | Remove watch expression by index |
| `break <spec>` | `b`, `breakpoint` | Set breakpoint: `file:line`, `@function`, or `function:pc` |
| `clear <spec>` | `cb` | Clear breakpoint |
| `listb` | `lb` | List breakpoints |
| `listw` | `lw` | List watch expressions |
| `locals` | `il` | Show local variables in current frame |
| `globals` | `ig` | Show global variables |
| `frame` | `if` | Show current frame info |
| `help` | `h`, `?` | Show this help |
| `quit` | `q` | Quit the game |

Breakpoints can be pre-set via `EMUERA_BREAKPOINTS` env var (comma-separated):
```
EMUERA_BREAKPOINTS="@EVENTFIRST,ERB/TEST.ERB:42,EVENT_SHOP:10"
```

In the TUI, pressing Enter at an empty prompt repeats the last step command (defaults to step into).
