A big game's scripts are tens of thousands of functions (AnonTW 90000,
pops-tw 100000), and a session runs a few percent of them. So at load
emuera-tty reads only each function's `@` line and attributes (`#FUNCTION`,
`#PRI`…) and where it is in its file; the statements are compiled the
first time the function is called, read again from the file (nothing is
kept in memory meanwhile). A parse error in a function body is reported
when the function first runs; `--check` compiles everything and lists
them all (`EMUERA_PARSE=all` compiles everything at load for a normal
run too). A script file edited while the game runs is read as it is
then, for functions not run yet; the reload key (Ctrl-E) reloads everything.

**Arrays hold only what was written.** An array keeps its elements up to
the last one written to; the rest read as 0 / "" and cost nothing. eraAL-K
gives each of its 743 characters CSTR:100000 and CFLAG:100000: allocated
in full, they took 2 GB and the game ran out of memory at "new game";
now they take 25 MB.

Three things are kept within a share of the machine's memory, so a 512 MB
board copes and a PC with memory to spare does exactly what it did:

- **Compiled functions** beyond 1/16 of the memory (`EMUERA_CODE_MB=n`
  sets it): the bodies called longest ago are dropped and compiled again
  if needed. A huge function (4 MB of code or more) lets its code go as
  soon as it first returns: pops-tw runs a 170,000-line one once at the
  title, about 50 MB compiled. If it is called again, it is kept like
  any other.
- **Graphics buffers** (`GCREATE`) beyond 1/8 (`EMUERA_GBUF_MB=n`): the
  least recently used are kept packed (run-length coded, exactly) until
  drawn or shown again. pops-tw keeps three portraits per character in
  buffers, 270 MB unpacked, 40 MB packed.
- **Decoded picture files** beyond 1/8 (`EMUERA_PICTURES_MB=n`): the
  least recently used whole files are dropped; the parts cut from them
  stay. AnonTW's whereabouts listing decodes 32 portrait sheets, 242 MB,
  to cut 33 MB of portraits from them.

**Pictures warm up while the game starts.** emuera-tty notes which picture
files a game decoded (in `~/.local/share/emuera-tty/pictures/`) and, the
next time it starts, decodes them again in the background on the spare
cores, within three quarters of that budget. The first screen full of
portraits then doesn't wait for them: AnonTW's whereabouts listing (166
characters) went from 1.5 s to 0.4 s on its first showing. The *Decode last
used pictures* setting controls this (default: *auto* — on unless headless);
`EMUERA_WARMUP=1` (or `0`) forces it on (or off). Disabling it can greatly
reduce peak memory use at launch on small machines.

| | before | now (PC) | with a 512 MB machine's budgets | lowest known recorded usage (**on idle/in background**) |
|---|---|---|---|---|
| AnonTW, 800-input session | 654 MB | 157 MB | 157 MB | ~2-4MB |
| pops-tw, a new game | 1204 MB | 636 MB | 346 MB | unrecorded |

`EMUERA_MEMSTATS=<file>` writes what the memory holds (functions
compiled, the biggest variables, characters and their biggest
variables, picture buffers…) to
`<file>` at every input.

Leaving the game on idle/in the background flushes out any
unneeded/unused memory and uses basically no CPU; memory use in this
state can go down to anywhere between ~2-50MB, letting you leave the game
in the background without any detrimental effect on memory use for other
apps. It may also drastically go down in real-time on a machine currently under
memory stress. Note that this is tracked from an AnonTW session, and other Era
games might have higher idle memory usage depending on their content.