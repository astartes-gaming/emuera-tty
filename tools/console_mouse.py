#!/usr/bin/env python3
"""Test the Linux-console mouse pointer without a real VT: run emuera-tty
in a pseudo-terminal with TERM=linux, connected to a fake gpm socket, send
it pointer moves and show what ends up on screen.

    tools/console_mouse.py BIN GAME_DIR X,Y [X,Y …]

For each move (1-based cells, as gpm sends them) it prints the screen with
reversed cells as '#', and every reversed cell's colours: a pointer drawn
black on black is "reversed" yet invisible, so check fg/bg, not just the
flag. EMUERA_MOUSE_LOG goes to ./mouse.log. Needs python3-pyte. The real
console differs in what a pty can't show: its 16 colours, the font's
glyph set (GIO_UNIMAP only works on the VT itself) and framebuffer
pictures drawn over the text. Use a scratch copy of the game.
"""
import os, pty, socket, struct, sys, time, select, fcntl, termios, tempfile
import pyte

BIN, GAME = sys.argv[1:3]
moves = [tuple(int(v) for v in m.split(",")) for m in sys.argv[3:]]
# (a short path: unix socket paths are limited to ~108 bytes)
sock = os.path.join(tempfile.mkdtemp(prefix="gpm"), "gpmctl")
srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock)
srv.listen(1)
cols, rows = 100, 30
pid, fd = pty.fork()
if pid == 0:
    # EMUERA_GPMCTL: the socket to connect to; EMUERA_GPM_VC: the VT number
    # a pty doesn't have
    os.environ.update(TERM="linux", EMUERA_GPMCTL=sock, EMUERA_GPM_VC="7", EMUERA_MOUSE_LOG=os.path.abspath("mouse.log"))
    os.execv(BIN, [BIN, GAME])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
scr = pyte.Screen(cols, rows)
st = pyte.ByteStream(scr)


def pump(t):
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.05)
        if r:
            try:
                st.feed(os.read(fd, 65536))
            except OSError:
                return


def show(tag):
    print("==", tag)
    for y in range(rows):
        line = "".join("#" if scr.buffer[y][x].reverse else (scr.buffer[y][x].data or " ") for x in range(cols))
        if line.strip():
            print(f"{y:2d}|{line.rstrip()}")
    for y in range(rows):
        for x in range(cols):
            c = scr.buffer[y][x]
            if c.reverse:
                print(f"   reversed cell ({x},{y}) {c.data!r} fg={c.fg} bg={c.bg}")


srv.settimeout(10)
pump(0.5)
conn, _ = srv.accept()
print("Gpm_Connect:", conn.recv(16).hex())
pump(3)
for x, y in moves:
    # Gpm_Event (28 bytes): buttons, modifiers, vc, dx, dy, x, y, type
    # (1 = GPM_MOVE), clicks, margin, wdx, wdy
    ev = struct.pack("=BBHhhhhiiihh", 0, 0, 0, 0, 0, x, y, 1, 0, 0, 0, 0)
    conn.send(ev.ljust(28, b"\0"))
    pump(0.3)
    show(f"move {x},{y}")
os.kill(pid, 9)
os.unlink(sock)
os.rmdir(os.path.dirname(sock))
