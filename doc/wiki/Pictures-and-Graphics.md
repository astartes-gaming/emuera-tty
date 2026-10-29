## Graphics backends

`--gfx auto` picks, in order:

| Environment | Backend | How |
|---|---|---|
| `TERM=linux` / `fbterm*` (VT) | **fb** | mmaps `/dev/fb0`, alpha-blends sprites onto the console after each frame. Needs read/write access to the device (`video` group). Cell size = fb resolution ÷ cols/rows. |
| `TERM_PROGRAM=iTerm.app` / `WezTerm`, `LC_TERMINAL=iTerm2` | **iterm** | PNG per sprite in `OSC 1337 File=`, sized in whole cells (padded, so never stretched), cached. |
| kitty graphics reply `_Gi=31;OK` | **kitty** | zlib-compressed RGBA (`o=z`) transmitted once per sprite+size, `a=p` placements (a replaced picture is placed before the old one is taken off), source-rect crop at the screen edge. |
| inside tmux with `allow-passthrough on`, and a kitty graphics reply or `GHOSTTY_RESOURCES_DIR` / `KITTY_WINDOW_ID` | **kitty-tmux** | the same through tmux's passthrough, placed with kitty's Unicode placeholders (U+10EEEE, the image id as the 256-colour foreground, row/column diacritics), which tmux handles as text. |
| DA1 reply contains `4` | **sixel** | median-cut 255-colour quantiser, transparent background, cached per sprite. |
| otherwise | none | images shown as `░` placeholders of the right size. |

### KMSCON limitations

KMSCON (the Linux KMS/DRM userspace console) has no terminal image
protocol (sixel, kitty graphics, iTerm2 images), so pictures show as
placeholders. The `fb` backend can't draw there either: KMSCON itself owns
the framebuffer (or the DRM device), and writing to it would fight with
KMSCON's own rendering.

KMSCON sets `TERM=xterm-256color` by default, so emuera-tty can't tell it
apart from xterm; with `TERM=kmscon` (its `--term` option) the prompt line
shows a reminder of these limits.

For its mouse, see [Mouse](Terminals#mouse) on the Terminals page.

### Canvas games and speed

Games that draw everything into a window-sized canvas every frame
(eraFlappyTengu, eraSort) are limited by how fast that canvas reaches the
terminal. Each frame is sent as one synchronized update (DEC mode 2026);
kitty pictures go compressed (a mostly empty canvas shrinks a
hundredfold), and scaling (`GDRAWSPRITE`, the canvas to the screen) uses
a fixed-point version of the Triangle filter on several cores. Off-screen
pictures stay in the terminal only up to 24 MB, and freed pictures'
ids are given to new ones: Konsole (25.04) never frees a picture's data
when told to, only when another picture comes with the same id, so
fresh ids for every frame would fill its memory within a minute. At a
1440×900 window, eraFlappyTengu runs at about 85 frames per second, using
about 4 ms of CPU per frame.

### Picture size and layers

Images are sized in Emuera's coordinate space (font-size–relative, as in
`HTML_PRINT <img height=…>`) and scaled by `terminal cell height / Emuera
line height`, so layouts keep their proportions. `emuera.config` font size /
line height / colours are honoured.

Pictures that overlap, like a portrait's layers (body, face, blush, a
frame, the tint while time is stopped, each printed on its own line and
moved onto the first with `ypos`), are blended into one picture before
they are sent, back to front as Emuera draws them. Terminals stack
separate pictures by rules of their own (kitty by image id, Konsole by
which was placed last), so a re-sent layer could land on top of the
others; one picture can't. It also helps sixel and iTerm2, which can't
stack transparent pictures at all.

### Text over pictures

In WezTerm, Windows Terminal and other terminals where sixel and iTerm2
pictures belong to the cells under them, and inside tmux, where kitty
pictures are placeholder text, writing text there erases the picture:
after redrawing a hovered button over a picture, emuera-tty sends that
picture again. Inside tmux a tooltip simply covers the placeholders,
rather than being cut out of the picture (which would mean sending a new
picture through tmux).

### Linux console caveat

The in-kernel console can't draw CJK glyphs. For
eratohoTW's Japanese/Chinese text on a bare VT, run under **fbterm** (still
uses the fb backend) or **yaft** (sixel).
