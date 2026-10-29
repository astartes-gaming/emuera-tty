Some games will hit something emuera-tty doesn't handle yet. Your saves
are safe either way: emuera-tty keeps a history of every save it
overwrites (see [Save history](Playing#save-history)).

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
  day change takes that long). Set the time with *Milliseconds for
  infinite loop warning* in *Game settings → Environment*, or the game's
  `emuera.config` (`無限ループ警告までのミリ秒数`).
- **Silent misparsing:** emuera-tty may parse or run part of a script
  differently from Emuera without any error: a wrong branch taken, a
  variable not set, an event not firing. These can be small and easy to
  miss. If something feels off, run the same save in the `Emuera.exe` the
  game comes with and compare; a report with both outputs is the most
  useful kind.
- **Reporting a bug:** include the error text and, if you can, a trace:
  `EMUERA_TRACE=FUNCTIONNAME emuera-tty 2> trace.txt` logs every call to
  functions whose name contains `FUNCTIONNAME`, with their results.
