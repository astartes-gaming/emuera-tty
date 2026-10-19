#!/usr/bin/env python3
"""Keep a game running in a pseudo-terminal and drive it from a FIFO.

    tools/gamed.py BIN GAME_DIR WORKDIR &      # starts the game
    tools/g.sh WORKDIR type 1                  # send a command, print the screen

Commands (one per line into WORKDIR/game.fifo): `type TEXT` (text + Enter),
`enter`, `key HEX` (raw bytes, e.g. `key 1b5b41` for Up), `click LABEL`
(SGR mouse move + click on the last screen occurrence of LABEL),
`wait SECS`, `quit`. Once the game is idle again (low CPU over a window, so
animated screens still settle) the screen is written to WORKDIR/screen.txt,
first line `status: idle|BUSY|gone`.

The game runs with TERM=xterm-256color, `--gfx none`, EMUERA_SEED=5, its
settings in WORKDIR/config (XDG_CONFIG_HOME), stderr in WORKDIR/stderr.txt
and a 3.5 GB address-space cap. Extra variables: GAMEENV="K=V K2=V2".
Needs python3-pyte. Run it on a scratch copy of the game: games write
saves and global.sav. Requires Linux (/proc for the idle check).
"""
import os, pty, sys, time, select, fcntl, termios, struct, resource
import pyte

BIN, GAME, WORK = sys.argv[1:4]
FIFO, SCREEN = os.path.join(WORK, "game.fifo"), os.path.join(WORK, "screen.txt")
COLS, ROWS = int(os.environ.get("COLS", 140)), int(os.environ.get("ROWS", 45))
os.makedirs(WORK, exist_ok=True)
screen = pyte.Screen(COLS, ROWS)
stream = pyte.ByteStream(screen)
pid, fd = pty.fork()
if pid == 0:
    resource.setrlimit(resource.RLIMIT_AS, (3500 << 20, 3500 << 20))
    os.environ.update(TERM="xterm-256color", EMUERA_MEASURE="0", XDG_CONFIG_HOME=os.path.join(WORK, "config"), EMUERA_SEED="5")
    os.environ.update(dict(kv.split("=", 1) for kv in os.environ.get("GAMEENV", "").split()))
    os.dup2(os.open(os.path.join(WORK, "stderr.txt"), os.O_WRONLY | os.O_CREAT | os.O_TRUNC), 2)
    os.execv(BIN, [BIN, GAME, "--gfx", "none"])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))


def cpu():
    f = open("/proc/%d/stat" % pid).read().split(")")[1].split()
    return int(f[11]) + int(f[12])


def settle(maxt=240, quiet=1.5):
    end = time.time() + maxt
    c0, t0, busy = cpu(), time.time(), 1.0
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                stream.feed(os.read(fd, 1 << 16))
            except OSError:
                return "gone"
        if time.time() - t0 > quiet:
            c = cpu()
            busy = (c - c0) / max(0.01, (time.time() - t0) * 100)
            if busy < 0.35:
                return "idle (%.0f%% cpu)" % (busy * 100)
            c0, t0 = c, time.time()
    return "BUSY after %ds (%.0f%% cpu)" % (maxt, busy * 100)


def rows():
    return ["".join(screen.buffer[y][x].data or " " for x in range(COLS)) for y in range(ROWS)]


def dump(status):
    with open(SCREEN + ".tmp", "w") as f:
        f.write("status: %s\n" % status)
        for i, r in enumerate(rows()):
            if r.strip():
                f.write("%2d|%s\n" % (i, r.rstrip()))
    os.replace(SCREEN + ".tmp", SCREEN)


dump(settle(90))
if not os.path.exists(FIFO):
    os.mkfifo(FIFO)
while True:
    with open(FIFO) as f:
        for line in f:
            cmd, _, arg = line.strip().partition(" ")
            if cmd == "quit":
                os.kill(pid, 9)
                sys.exit()
            elif cmd == "enter":
                os.write(fd, b"\r")
            elif cmd == "key":
                os.write(fd, bytes.fromhex(arg))
            elif cmd == "type":
                os.write(fd, arg.encode() + b"\r")
            elif cmd == "click":
                hit = [(y, r.index(arg)) for y, r in enumerate(rows()) if arg in r]
                if hit:
                    y, x = hit[-1]
                    os.write(fd, ("\x1b[<35;%d;%dM" % (x + 2, y + 1)).encode())
                    time.sleep(0.4)
                    os.write(fd, ("\x1b[<0;%d;%dM\x1b[<0;%d;%dm" % (x + 2, y + 1, x + 2, y + 1)).encode())
            elif cmd == "wait":
                time.sleep(float(arg))
            dump(settle(float(os.environ.get("MAXT", "120"))))
