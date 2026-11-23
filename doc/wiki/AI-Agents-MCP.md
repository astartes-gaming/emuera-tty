`emuera-tty GAME_DIR --mcp` runs a game as a
[Model Context Protocol](https://modelcontextprotocol.io) server, so an
AI agent harness can play it and inspect it: Claude Code, Codex,
OpenCode, Cursor and others that speak MCP. It is meant for QA (checking
that a change works by playing to it), for reproducing bug reports, and
for agents that write or translate a game's scripts and want to see the
result.

The game runs as in `--headless` mode: no screen, no sound. It stops at
every input, and the agent answers it with the `input` tool. In between,
it can look at the screen, list the buttons, evaluate expressions, change
variables and see where the script is.

- [Setting it up](#setting-it-up)
- [The tools](#the-tools)
- [How an input works](#how-an-input-works)
- [Tips](#tips)
- [Limits](#limits)

## Setting it up

The server talks over stdin/stdout (MCP's stdio transport): the harness
starts `emuera-tty` itself, so it only needs the command line. Give it
**a copy of the game**, not the folder you play in: the game writes saves
and `global.sav` there, and `run` can change anything. Copying `CSV/` and
`ERB/` is enough; add `resources/` if picture names matter to you.

**Claude Code**, from the folder you work in:

```sh
claude mcp add emuera -- emuera-tty /path/to/game-copy --mcp
```

or in a project's `.mcp.json`:

```json
{
  "mcpServers": {
    "emuera": {
      "command": "emuera-tty",
      "args": ["/path/to/game-copy", "--mcp"],
      "env": { "EMUERA_SEED": "1" }
    }
  }
}
```

**Codex**, in `~/.codex/config.toml`:

```toml
[mcp_servers.emuera]
command = "emuera-tty"
args = ["/path/to/game-copy", "--mcp"]
```

**OpenCode**, in `opencode.json`:

```json
{
  "mcp": {
    "emuera": {
      "type": "local",
      "command": ["emuera-tty", "/path/to/game-copy", "--mcp"],
      "enabled": true
    }
  }
}
```

Other harnesses take the same command and arguments in their own MCP
settings. Harnesses without MCP support (pi, for one) need an MCP
extension or bridge.

Other options work alongside `--mcp`:

- **`EMUERA_SEED=N`:** the same random numbers every run, so a session can
  be replayed.
- **`EMUERA_CLOCK=MS`:** a fixed game clock, for games that read the time.
- **`--game-debug`:** runs the game's `[IF_DEBUG]` blocks and `;#;` lines,
  as Emuera's `-Debug` does. Many games have debug menus behind them.
- **`--launch-flags FILE`:** the usual options from a file.

## The tools

| Tool | Arguments | What it does |
|---|---|---|
| `screen` | `lines` (default 40) | The last lines of the console, then what the game waits for and the live buttons. What it showed isn't repeated in the next `input`'s answer |
| `buttons` | — | The buttons the current input accepts: each one's value and the text it shows |
| `input` | `value`; `stop_at_wait`, `timeout` (optional) | Answers the input and runs until the next one. Returns everything printed meanwhile, then what the game waits for and its buttons |
| `eval` | `expression` | The value of an ERB expression: `FLAG:3`, `CFLAG:MASTER:10`, `NAME:TARGET`, `LOCAL@FUNC`, arithmetic, built-in functions, the game's `#FUNCTION`s |
| `run` | `statement` | Runs one ERB statement as Emuera's debug console does (`FLAG:3 = 5`, `PRINTL text`, `ADDCHARA 5`), and also `CALL`, `CALLF`, `CALLFORM`, `TRYCALL` (which that console refuses), so a game's setup functions can run. Returns what it printed |
| `state` | — | The line running and the call stack (Emuera's debug window format, with Japanese labels), the game's phase (`TITLE`, `FIRST`, `SHOP`, `TRAIN`…), what the game waits for, and the buttons. At an input of the engine's own menus (the SHOP loop's command input) no script function is running, so the trace says なし (none) and the phase tells where the game is |

Answers end with a status block, for example:

```
--- waiting for: a number (INPUT)
--- buttons:
100 → [100] Train
101 → [101] Next day
"SORT" → Sort
```

A button's value is what `input` takes: a number, or a string in quotes
(send it without the quotes).

## How an input works

- **Number and text inputs** (`INPUT`, `INPUTS`, `TINPUT`, `ONEINPUT`…):
  `value` is what you would type. It may hold Emuera's input macros
  (`0\n1`, `(0\n)*3`), which answer several inputs in a row, and `@`
  system commands (`@REBOOT`, `@OUTPUTLOG`).
- **WAITs** pass by themselves while `input` runs on to the next input
  that needs a value, and the answer says how many it passed. With
  `"stop_at_wait": true`, `input` stops at each WAIT instead; send any
  value to go on. (When the game starts at a WAIT, the agent's first
  `input` answers it.) After 500 WAITs in a row, `input` answers anyway.
- **Timed inputs** (`TINPUT`, `TWAIT`, a timed `INPUTMOUSEKEY`) wait for
  the agent, not the clock. `"timeout": true` lets one run out instead.
- **Mouse and key inputs** (`INPUTMOUSEKEY`, used by picture menus such as
  pops-tw's): a button's value clicks that button. `key:Enter`, `key:Esc`,
  `key:Up`, `key:F1`, `key:Space` press keys. `key:65` sends a Windows key
  code, and a single character presses that key.
- **When the game ends** (`QUIT`, the window closed): the answer is
  everything it printed, then `--- the game has ended`. The server then
  exits.

## Tips

- **Find the code behind a screen:** call `state` at the screen you are
  looking at. It names the ERB file and line where the game waits.
- **Skip to the part to test:** set up the situation with `run` (time,
  money, a character's flags), as players do with Emuera's debug console.
  Then use `input` to play through it.
- **Test a function directly:** `eval` calls a `#FUNCTION` with any
  arguments (`MY_CHECK(TARGET, 3)`). `run` with `CALL` runs an ordinary
  function, and the answer includes what it printed.
- **Replay a session:** keep `EMUERA_SEED` and the values the agent sent.
  `--headless` with the same answers (one a line on stdin) plays it again.

## Limits

- **Debug-console rules:** `eval` and `run` follow the same rules as
  Emuera's debug console, except that `run` may call a function. Other
  flow control (`GOTO`, `JUMP`, `RETURN`, `BEGIN`…), inputs, WAITs and
  `SAVEDATA` are refused with Emuera's message, and an error comes back
  with `isError` set.
- **Inside a called function:** if a function called with `run` waits
  for input, the `run` answer arrives at that input, ending with what the
  game waits for and its buttons, and `input` goes on from there. A `BEGIN` or `QUIT` inside it ends only the call, not the
  game.
- **Saving and loading:** go through the game's own save and load menus
  with `input`. There is no separate tool for them yet.
- **No pictures in answers:** answers are text. A picture shows as
  `[img:NAME]`, and colours and layout are left out. To check graphics,
  play in a terminal (see [Pictures and graphics](Pictures-and-Graphics)).
- **Plain-text drawings:** a text-art map is still sent as text, but an
  agent may not read it well.
- **One game per server:** start one server per game. Each is a separate
  process (and memory: see [Memory](Memory)).

See also [Debugging](Debugging) for the debug panel, which the `eval`,
`run` and `state` tools share their engine with.
