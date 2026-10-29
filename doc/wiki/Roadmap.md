1. Run it on macOS, DragonFly and Redox and on a real Raspberry Pi Zero:
   they compile (the Pi build also runs under emulation; Haiku, the other
   BSDs, illumos and GNU/Hurd pass the tests in virtual machines), nobody has
   played on them yet. Windows and Android have
   had some testing, and need more feedback.
2. A good way to play on Android phones, through Termux: players keep
   asking for an up-to-date eraBASIC player there, and saves would move
   between phone and PC. Nothing is known to be missing, and it has had
   some testing on Termux, but it needs more feedback. What needs checking: a phone's narrow screen
   (games are laid out for a wide window), taps arriving as clicks for
   buttons and picture menus, function keys and Ctrl keys from Termux's extra-keys
   row, Japanese text widths in Termux's font, whether its terminal draws
   sixel pictures, and games kept in shared storage (`/sdcard`). A
   separate Android app would come later, if at all: the engine never
   talks to the terminal directly, so another frontend could be added.
3. Play deeper into AnonTW, pops-tw and eraTohoK, and more games.
   AnonTW's 5 remaining parse errors and pops-tw's 16 are mistakes in the
   games' own scripts (Emuera reports them too); eraTohoK has none.
4. Dialog translation and sound with real translators and players on
   other systems: tested here with stand-ins and mpv only.
5. Speed of script-heavy games: pops-tw's new game takes about 14
   seconds of CPU (its own object system looks names up in 1000-entry
   arrays hundreds of thousands of times, through a million statements);
   compiling variables to slots ahead of time would be the next step.
