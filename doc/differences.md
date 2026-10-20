# Deliberate differences from Emuera-EM/EE

Where emuera-tty does something other than Emuera-EM/EE
(`../emuera.em`) on purpose, and why, for future reference. Differences
from Emuera-Anchor's translation system are in
[anchor.md](anchor.md#deliberate-differences).

The rule: copy Emuera, bugs included, where not doing so would make a
game not work, or not work as expected, because the games are written
against it. Differ to fix a bug or do the right thing, as long as saves
stay compatible both ways and nothing the games rely on in Emuera breaks.
Things that differ only because they aren't implemented yet aren't
listed here; they're gaps (README, *Implemented* and *Roadmap*).

Each entry: what, what Emuera does (with the C# file and line), what
emuera-tty does (with the Rust function), and why.

No entries have been written yet. Differences made before this file was
started were never collected; add them here when you come across one.

| What | Emuera-EM/EE | emuera-tty | Why |
|---|---|---|---|
