On Linux, emuera-tty reads gamepads itself: the **Steam Deck**'s
controller (when Steam hands it to the game, see below), Xbox,
PlayStation, 8BitDo and other pads, wired or Bluetooth. Plug one in at any
time; the first touch shows a pointer on the first button.

| Pad | In a game | In menus (launcher, settings, saves) |
|---|---|---|
| D-pad | jump to the next button that way (held: repeats) | ↑ ↓ ← → |
| Left stick | move the pointer freely (picture menus, maps) | — |
| A | click under the pointer | Enter |
| B | right click: skip messages, as in Emuera | Esc / back |
| X, Y | Enter (continue, send what is typed) | Enter |
| Start | settings | settings |
| Select / Back | Esc | Esc |
| LB / RB | page up / down through the text | page up / down |
| Right stick | scroll a line at a time | ↑ ↓ |
| LT / RT | top / bottom of the text (Home / End) | — |

*Gamepad* in the settings turns it off (it also shows which pads were
found); `EMUERA_GAMEPAD=/dev/input/eventN` reads just one device. Pads are
readable without setup by whoever is logged in at the computer. Over SSH
they aren't yours to read, and nothing happens.

**On the Steam Deck** (SteamOS):

1. **Desktop Mode** (Steam button → Power → Switch to Desktop) for setting
   up.
2. **Get the program**, as under [SteamOS](Installation#steamos-steam-deck) (a
   Distrobox build, or a static build copied from another PC; the
   `emuera-tty-linux-x86_64-static` artifact of the *Build all platforms*
   workflow on GitHub is one).
3. **Copy your game** somewhere in your home folder (`~/Games/…`), with
   its saves if you have some.
4. **Add it to Steam**, so Steam hands the controller over as a gamepad:
   Steam → *Games* → *Add a Non-Steam Game to My Library* → **Konsole**.
   In its *Properties*, set *Launch Options* to
   `--fullscreen -e /home/deck/.local/bin/emuera-tty` and the name to
   something like *emuera-tty*. In *Controller* settings pick the
   **Gamepad** layout.
5. Back in **Game Mode**, start it from the library: the launcher opens,
   the d-pad and A choose the game. **Steam + X** brings up the on-screen
   keyboard for typing names or numbers.

Konsole shows the pictures (kitty graphics). If the text is too big for
games laid out for wide windows, make the font smaller in Konsole's
profile (or Ctrl+−). Started from Desktop Mode without Steam, the Deck's
controller works as Steam's desktop layout (trackpad as mouse, triggers as
clicks), which emuera-tty takes like any mouse.

The gamepad code is tested with a simulated pad; it hasn't been tried on
a real Deck yet, so reports are welcome.
