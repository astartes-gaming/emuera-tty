1. Play on macOS and on a real Raspberry Pi Zero: the test suite passes
   on macOS (CI) and the Pi build runs under emulation, but nobody has
   played on them yet. Windows and Android have had some testing and need
   more feedback.
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
3. Dialog translation and sound with real translators and players on
   other systems: tested here with stand-ins and mpv only.
4. Speed of script-heavy games: pops-tw takes about 13 seconds of CPU
   from the title to the first menu and 25 more for the first Wake Up.
   Its own object system runs 19 million function calls there and 6
   million FINDELEMENT searches through a table of a few hundred names
   (those now compare hashes before strings, 5% of the time saved).
   What is left is spread over every variable access, so compiling
   variables to slots ahead of time would be the next step.
5. Feature parity with other Emuera forks: various forks of Emuera
   (such as EM and Anchor) can have their own entirely unique features
   and scripting logic that emuera-tty can't load unless implemented.
   Replicating these features from their sources or thorough
   reverse-engineering can let emuera-tty run Era games dependent on
   those features.
6. Behavior testing/comparisons: run emuera-tty and a fork of Emuera
   that accepts set-seed runs with a single seed and a single shared save
   file. With both sides using SFMT, using the same seed and save on both
   Emuera and emuera-tty should produce the same outputs. If emuera-tty
   diverges, then some handling/parsing/interpreting is very likely being
   done incorrectly.
   - Some games also read the clock (pops-tw picks tips and shop staff
     by it): `EMUERA_CLOCK` fixes it on emuera-tty's side, but Emuera's
     side has no such setting.
   - **NOTE that both sides should be run with the exact same chain of commands.**
     Not doing this will obviously cause divergence.
     **Every single command you send to the game matters.**
   - In AnonTW, there are several points available for side-by-side 
     comparisons (some are not listed for obvious reasons):
     - Wake-up events
     - Outputs of `Whereabouts Sense` (character locations)
     - Character events
     - Character PALAMs and PALAM gains
     - Scores/number outputs (Danmaku matches, EXP gains, etc.)
     - And other things...
