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
