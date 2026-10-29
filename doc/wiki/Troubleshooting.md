Some games will hit something emuera-tty doesn't handle yet.

- **A red `*** script error ***` screen** means the game hit something
  emuera-tty doesn't support yet (or a bug of the game's). As in Emuera
  it shows the file and line, the line itself, the function, and the
  call stack that led there. Press Enter to quit; your last save is safe.
- **`warning: … not implemented (ignored)`** lines in the terminal after
  quitting are commands emuera-tty skipped; the game usually carries on.
- **Check a game without playing it:** `emuera-tty /path/to/game --check`
  lists every line that fails to parse, and, as Emuera does, every
  statement a `#FUNCTION(S)` may not use (input, CALL/JUMP, BEGIN, QUIT,
  RETURN, PRINTW…: it is an error if it runs).
- **Screen looks broken:** Ctrl-L redraws; try `--gfx none` to rule out
  image problems; make the window wider.
- **Infinite loop detection:** if a script runs too long without input
  (default 5 s), a prompt appears (TUI); headless runs go on, with a note
  on stderr (no one is there to answer, and on a slow machine a big game's
  day change takes that long);
  configure via `Milliseconds for infinite loop warning` in *Game settings → Environment* or the game's `emuera.config` (`無限ループ警告までのミリ秒数`).
- **Silent misparsing:** this fork could potentially parse or handle parts
  of a script incorrectly, leading to unintended behavior without an error
  message. If something feels off (wrong branch taken, variable not set,
  event not firing), compare with the original `Emuera.exe` the game comes
  with — run both on the same save and check for divergence.
  **These discrepancies can be very minor/almost unnoticeable, so keep a close eye on mishaps you might see.**
- **Reporting a bug:** include the error text and, if you can, a trace:
  `EMUERA_TRACE=FUNCTIONNAME emuera-tty 2> trace.txt` logs every call to
  functions whose name contains `FUNCTIONNAME`, with their results.
