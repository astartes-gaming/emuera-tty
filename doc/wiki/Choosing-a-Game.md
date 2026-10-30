Start emuera-tty with no arguments. The first time it opens the folder
picker to find a game; once you have played games it opens the launcher,
your library of them.

```sh
emuera-tty
```

## The folder picker

The first time, a **folder picker** opens in your home folder. Game folders
are marked with **★** and their title, archives with **▣** and their size.

| Key | Action |
|---|---|
| ↑ / ↓ (or `k` / `j`), PgUp / PgDn, Home / End | move |
| Enter | play the ★ game under the cursor, extract and play a ▣ archive, or open a folder |
| → / ← (or Backspace) | open a folder / go up |
| `~` | jump to your home folder |
| `.` | show hidden folders |
| `q` / Esc | cancel |

The mouse works too: click an entry to select it, click it again to open
or play it, scroll with the wheel.

emuera-tty remembers the game. From then on `emuera-tty` starts it
straight away.

## The launcher (several games)

**Several games** (eraTW, eraMegaten, eraPoPs, any Emuera game): every game
you play or add goes into your **library**. Once there are two or more,
`emuera-tty` opens the launcher instead:

When run with `--game-debug` (or `EMUERA_GAME_DEBUG=1`), an orange line
appears at the top: **Game debugging enabled; [IF_DEBUG] blocks and ;#; lines WILL run**.

When run with `--vm-debug` (or `EMUERA_VM_DEBUG=1`), an orange line appears
at the top: **VM-level debugging/breakpoints enabled; select this option to show a list of what files/functions have an attached breakpoint**.
Press Enter on that line to open a dialog listing the breakpoints from
`EMUERA_BREAKPOINTS` (format: `ERB/FILE.ERB:42`, `@FUNCTION`, `FUNCTION:10`).

| Key | Action |
|---|---|
| Enter (or click twice) | play the selected game |
| `a` / **＋ Add a game…** | open the folder picker above to add a folder or archive (it only joins the list, marked gold **(NEW)** until you first play it; Enter then plays it) |
| **↓ Clone a game…** | ask for a git repository, the folder to keep it in and (optionally) a branch and a history depth, `git clone` it (Esc cancels) and add it to the list, marked gold **(NEW)** until you first play it; it doesn't start |
| **⇩ Download a game…** | pick a game from the [eraGames wiki](https://wiki.eragames.rip)'s list and clone it (see [Downloading a game](#downloading-a-game)) |
| `u` | update a game kept in a git checkout (`git pull --ff-only`; marked **· git** with its branch and short commit hash) |
| `U` (Shift-U) | update all of them, each checkout once, then sum up what changed |
| `g` | right after an update (`u` / `U`): show git's full output in a scrollable view (`g` is "go to top" otherwise) |
| `d` / Delete | take the game off the list (its files and saves stay) |
| (Enter on a missing game) | if the game's folder no longer exists, a dialog opens: **Remove it from the list** or **Update the folder** to point to the new location (← / → or mouse to pick a button, Enter or click to confirm, Esc to cancel) |
| `s` / **⚙ Settings** | the settings page (see [Settings](Settings#emuera-ttys-settings-f3)) |
| `q` / Esc | quit |

## Cloning a game from the launcher

**↓ Clone a game…** (below *Add a game…*) asks for the repository (URL or
path), the folder to store it in (the repository gets its own subfolder
there, named like `git clone` would; Enter takes the folder next to your
last game) and, if you want them, the branch to check out straight away
(Enter for the repository's default) and a history depth for big
repositories (`1` fetches only the latest commit, Enter the full history; a
shallow clone still updates with `u`). It shows git's progress in a box
over the launcher (see below). Each field starts from what you gave last
time, shown in [brackets]: Enter on an empty field reuses it, and **Tab**
puts it in the field for editing (clear it to leave the field empty, e.g.
to drop a remembered branch). **Esc** or **Ctrl-C** cancels the clone (git
cleans up after itself; if a half-cloned folder is left, it asks before
removing it). Otherwise it adds the game to the list without starting it,
marked **(NEW)** in gold until you first play it (games added with *Add a
game…* get the same mark). `g` shows git's full output of the clone, also
when it failed. A game kept in a subfolder of the repository (up to two
levels down) is found too.

## Downloading a game

**⇩ Download a game (from the eraGames wiki)…** lists the games of the
[eraGames wiki](https://wiki.eragames.rip/index.php/Category:Games) (its
*Games* category) that link a git repository, each with the repository
it names first (`(+2 more)`: other repositories or branches) and, dimmed
under it, what the game is: a notice from its page (no longer
maintained…), its genre and the first sentence of its description (or its
translation status). Enter on one shows its genre, version and
translation status, its description and what to download: each repository or branch
the page links to (a link to a branch's zip counts as that branch), with
the branch a repository checks out by default. Where that default isn't
an English branch but the repository has English ones the page doesn't
name (many era-games repositories default to `game-old/master` or
`game/japanese` beside `game/eng-release`), those are offered first:
releases, then translations, then the rest. Only repositories are
offered, never release archives or file hosts. Enter
on one clones it with only the latest version (`--depth 1`: much smaller
and quicker, and `u` still updates it), into the folder *Clone a game…*
was given last (else next to your last game), named after the repository
and branch (`eraMegaten-game-eng-release`). Then it joins the library,
selected and marked **(NEW)**: Enter plays it.

It needs `curl` (for the wiki) and `git`.

### Logging in to gitgud.io

Many repositories of gitgud.io's *era-games* group are for signed-in
users only. When a game opens, the downloader asks each of its
repositories first and marks those that don't answer *(asks for a
gitgud.io login, or is gone)*: gitgud answers the same for a repository
that no longer exists.

**🔑 Log in to gitgud.io…**, at the top of the list (and on the page of a
game that needs it), asks for your gitgud.io user name and password, or
an [access token](https://gitgud.io/-/user_settings/personal_access_tokens)
with *read_repository* (needed when your account has two-factor
authentication). The password is typed hidden. git checks it right away;
then the launcher offers to remember it. A remembered login is kept in
`~/.config/emuera-tty/git-credentials`, readable only by you, in plain
text as git's own credential store keeps it, and is used in later
sessions and by `u`. Otherwise it is forgotten when you leave the
launcher. Once logged in, the entry says as whom, and Enter on it logs out
(forgetting a remembered login too). Downloading or updating (`u`) a
repository that wants a login when you haven't given one asks for it
there and then, and offers to remember it once it has worked.

The login reaches git through the environment of each git command, read
by a credential helper of emuera-tty's own: never on a command line,
where other users could see it.

## Updating games kept in git

Games cloned with git (`git clone` of a game's repository) update with
`u` in the launcher: it pulls the new commits and says how many came in,
with the commit it moved from and to (`abc1234 -> def5678`). Each such game
is listed with the branch and short commit hash it is on
(`· git master @ abc1234`), and `g` shows git's full output of the update
just done. It only fast-forwards and never asks for a password, so a game
you have edited, or a repository that needs a login, reports why it
couldn't update instead (commit or stash your edits, or run `git pull` yourself).
Saves in the game folder that the repository ignores are left alone.

While a game updates, clones or comes out of its archive, a box over the
launcher shows how far along it is, drawn like the gauges of Debian's
installer: the bar fills in red from git's own progress (receiving the
objects, resolving deltas, writing the files, then the Git LFS pictures),
and a block sweeps along it while git hasn't said how far it is yet (and
for archives, with the count of files written). With `U` the bar covers all
the games, "Updating 2 of 5: …". A terminal too small for the box (under
42 columns or about 10 rows) gets the same news on the status line.

### Games with pictures in Git LFS

Games that keep their pictures in Git LFS (pops-tw) also get `git lfs
pull`. Without [git-lfs](https://git-lfs.com) a clone holds small pointer
files instead of the pictures, and scripts that measure a picture fail
(pops-tw's title: a division by zero in `@TITLE_NAS`). Install git-lfs and
run `git lfs pull` in the game folder (or `u` again); the script error
screen says so when it finds such pointers.

## Other ways to start

Inside a game, **Ctrl-O** → *Switch game…* goes back to the
launcher. Other ways to start:

```sh
emuera-tty --choose                 # open the launcher even with one game
emuera-tty ~/games/eratohoTW        # play a specific folder
cd ~/games/eratohoTW && emuera-tty  # a game in the current folder wins
emuera-tty ~/Downloads/eratohoTW.zip # play straight from the archive
emuera-tty https://gitgud.io/eracorrection/eraflappytengu.git  # clone it in the launcher
```

A git repository's address (or `--clone URL`) opens the launcher and
clones the game straight away, without *Clone a game…*'s questions: into
the folder it would offer first, on the default branch, with the full
history (use *Clone a game…* for a branch or a shallow clone). The game is
then selected, marked **(NEW)**: Enter plays it. If it was cloned there
before, that copy is selected instead (`u` updates it).

## Archives

Archives are extracted once into `~/.local/share/emuera-tty/games/`
(named after the archive) and reused after that. Download a newer version
under the same file name and it is re-extracted automatically, **keeping
your saves**. Zip, tar and tar.gz work out of the box; tar.xz / tar.bz2 /
tar.zst, 7z and rar need `bsdtar`, `tar` or `7z` installed
(`sudo apt install libarchive-tools p7zip-full`). Japanese file names in
zips made on Windows are handled. Delete a game's folder there to remove
it.
