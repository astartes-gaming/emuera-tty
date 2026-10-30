## Other systems

emuera-tty is developed and played on Linux. The whole test suite also
runs on **macOS and Windows** (GitHub's runners, on every push), and in
virtual machines on **FreeBSD 15.1, NetBSD 11.0, OpenBSD 8.0, DragonFly
6.4, illumos (OmniOS r151058), Haiku R1/beta5, GNU/Hurd (Debian
2026-03-14) and Redox (2026-10-14)**, where two long AnonTW sessions also
give output identical to Linux (October 2026). **Windows** and **Android**
have had some play-testing but need more feedback (Android on Termux, and
it even runs under a custom recovery environment, TWRP). **Solaris** is
only compile-checked (`cargo check --target …`): its installer needs an
Oracle account.

- **macOS / BSD**: the same steps. On macOS, `~/.local/bin` is usually not on
  PATH, so `--install` picks a writable PATH folder (Homebrew's
  `/usr/local/bin`) or tells you how to add one. Settings live in
  `~/.config/emuera-tty`. The Linux-console extras (framebuffer pictures,
  gpm / `/dev/input/mice` mouse) are Linux-only; everything that goes
  through the terminal works the same. Fonts for picture text come from
  fontconfig when it's installed, else the system's Japanese fonts
  (Hiragino on macOS).
- **Windows**: install Rust with `rustup-init.exe` (the MSVC toolchain, as
  rustup suggests), then in PowerShell:
  `git clone …`, `cd emuera-tty`, `cargo build --release`,
  `.\target\release\emuera-tty.exe --install`. Run it in **Windows Terminal**
  (mouse, true colour and sixel pictures in recent versions); the old
  console window works too but without pictures. Settings go to
  `%APPDATA%\emuera-tty`, extracted games to `%LOCALAPPDATA%\emuera-tty`;
  game saves stay in the game's `sav\` as with Emuera, so you can switch
  between Emuera.exe and emuera-tty. `--serial COM3` drives a terminal on a
  COM port. Picture text uses MS Gothic / Meiryo / Yu Gothic from
  `C:\Windows\Fonts`.
- **Termux** (Android): `pkg install rust git`, then the same steps as on
  Linux; `--install` puts the program in Termux's `$PREFIX/bin`. Pictures
  show if Termux's terminal answers that it can draw sixels (auto
  detection), else placeholders.
- **Haiku**, **Redox**, **illumos / Solaris**, **GNU/Hurd**, **NetBSD /
  OpenBSD / DragonFly**: the Unix code path (termios, `poll`), as on Linux minus the
  Linux-console extras. Haiku's Terminal shows text only. Redox's own
  console plays in full screen, with its limits: it sends nothing for
  F1–F12 or Esc (Ctrl-O opens the settings, `q` closes menus), has no
  mouse and no pictures, and its font has no Japanese, so translated
  games read best there. (`tcdrain`, called after each frame on other
  systems, isn't used on Redox: its pty daemon answers it by sleeping,
  which froze every console.) illumos' own console (OpenSolaris'
  descendants: OmniOS, OpenIndiana, SmartOS; `TERM=sun-color`) plays in
  full screen too, F1–F12 and the paging keys included, in 24-bit colour.
  It draws Japanese as boxes (its font has none) one cell wide, so
  emuera-tty pads each character to Emuera's two columns and the layout
  stays put. It has no mouse and no pictures, and it doesn't answer
  terminal queries, so the clipboard copy is off there. Redox's test harness needs
  `--test-threads=1`, as tests run in parallel hang there. On Haiku the fastest serial speeds (460800, 921600) aren't
  offered, on Redox there is no RTS/CTS flow control. Haiku gives a thread
  at most 16 MB of stack (Linux: the 512 MB emuera-tty asks for), so there
  scripts may nest about 1,000 calls instead of 4,000 before "call stack
  overflow". `GETMEMORYUSAGE` and the memory counter read the memory in
  use everywhere (Redox: the process's private memory, as its `ps`
  shows, less the stack reserved for scripts). A terminal that doesn't take UTF-8
  (NetBSD's console, xterm in Latin-1 mode) is detected at startup and
  gets ASCII lookalikes or `?` in each character's cells, so layouts stay
  aligned; FreeBSD's console takes UTF-8 but its default font has no
  Japanese. The Hurd's console (`TERM=hurd`) shows Japanese and is
  treated like the Linux console: 16 colours (brightened by bold, as it
  drops 90–97), no terminal queries, no mouse (the keyboard pointer is
  on), and no copying to the clipboard (it would print the OSC 52 escape).

## Other CPUs

The systems above are about x86-64 PCs. emuera-tty is plain Rust with no
CPU-specific code, so it builds for most processors Linux runs on. Saves
are Emuera's little-endian format on every CPU: a save made on a
big-endian machine is byte for byte the one a PC makes. **Tried only
under emulation (`qemu-user`), not on real machines.**

| | CPUs |
|---|---|
| **Run under emulation**: the sample game's self-test, a full 800-input AnonTW session with the same output as on a PC, saves loaded across | x86 32-bit (i586), ARMv5 / v6 / v7, AArch64, RISC-V 64, PowerPC 64 (big- and little-endian), LoongArch 64 |
| **Compile-checked** (no C library here to link against) | RISC-V 32, PowerPC 32, IBM Z (s390x), SPARC 32 / 64, MIPS 32 / 64 (both byte orders), Motorola 68000, Hexagon, C-SKY, big-endian ARM / AArch64, x32 |
| **Not possible**: Rust has no code generator for them | DEC Alpha, PA-RISC (HPPA), SuperH (SH-4), Itanium, OpenRISC, MicroBlaze, Xtensa |

The emulated builds are static musl programs, made on a PC with Rust's own
linker (no cross compiler needed): add the target with `rustup target add`,
then build with `RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes
-C target-feature=+crt-static" cargo build --release --target …` for
`i586-unknown-linux-musl`, `arm-unknown-linux-musleabihf`,
`armv5te-unknown-linux-musleabi`, `armv7-unknown-linux-musleabihf`,
`aarch64-unknown-linux-musl`, `riscv64gc-unknown-linux-musl`,
`powerpc64-unknown-linux-musl`, `powerpc64le-unknown-linux-musl` or
`loongarch64-unknown-linux-musl`. The compile-checked ones need nightly
Rust (`cargo +nightly build -Z build-std=std,panic_abort --target …`) for
most, and the target's C library to link. RISC-V 32 is checked as
`riscv32gc-unknown-linux-musl`: the glibc target's own standard library
doesn't build with current nightly Rust (`SYS_futex` is missing from its
`libc`). On the machine itself, `cargo build --release` works wherever Rust
is installed.

## Small machines (Raspberry Pi Zero and the like)

emuera-tty keeps its memory low enough for a 512 MB board (see
[Memory](Memory)): AnonTW stays under 160 MB, pops-tw under 350 MB.
It runs on 32-bit ARM (Pi Zero / Zero W, ARMv6) and 64-bit ARM (Pi Zero 2,
Pi 3 and later). **Not tried on a real board yet**: the 32-bit ARM build
runs AnonTW under emulation (`qemu-arm`) with the same output as on a PC,
and both ARM builds compile.

Building on the board itself works (`cargo build --release`) but takes a
long time on a Zero; building on a PC and copying the program over is
quicker. For a Pi Zero (no cross compiler needed):

```bash
rustup target add arm-unknown-linux-musleabihf
```

```bash
RUSTFLAGS="-C linker=rust-lld -C link-self-contained=yes -C target-feature=+crt-static" cargo build --release --target arm-unknown-linux-musleabihf
```

then copy `target/arm-unknown-linux-musleabihf/release/emuera-tty` to the
Pi (use `aarch64-unknown-linux-musl` for a 64-bit Pi OS) and run
`./emuera-tty --install` there. On the Linux console with no X, pictures
go to the framebuffer; over SSH, use a terminal with sixel or kitty
graphics, or `--gfx none` for text only. A big game takes some seconds to
load on a Zero (it reads every script once).

## Build status

What GitHub Actions builds, and what has been run where.

| OS/Architecture | Status | Notes |
|---|---|---|
| Linux x86-64 | Build passed | Most common platform, under constant testing |
| Linux x86-64 (static) | Build passed | Untested, but should work the same as above |
| Windows x86-64 | Build passed | Test suite passes on GitHub's runners; tested launching and basic playthrough |
| Windows ARM64 | Build passed | Untested |
| macOS ARM64 (Apple silicon) | Build passed | Test suite passes on GitHub's runners; not played yet |
| macOS x86-64 | Build passed | Untested |
| Linux i586 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux ARMv5 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux ARMv6 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux ARMv7 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux AArch64 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux RISC-V 64 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux PowerPC 64, big-endian (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux PowerPC 64, little-endian (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| Linux LoongArch 64 (static) | Build passed | Test suite passes under qemu-user; game output identical to x86-64 |
| FreeBSD x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own cc: the test suite passes in a FreeBSD 15.1 VM, game output identical to Linux |
| NetBSD x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own cc: the test suite passes in a NetBSD 11.0 VM, game output identical to Linux |
| OpenBSD x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own cc: the test suite passes in a OpenBSD 8.0 VM, game output identical to Linux |
| DragonFly BSD x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own cc: the test suite passes in a DragonFly 6.4.2 VM, game output identical to Linux |
| illumos x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own cc: the test suite passes in a OmniOS r151058 VM, game output identical to Linux; full-screen play works on its console and over SSH |
| Solaris x86-64 | Build failed (under actions), check passed | Compile-checked only (Solaris needs an Oracle account to download); illumos, its open relative, passes |
| Haiku x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with Haiku's own gcc: the test suite passes in a Haiku R1/beta5 VM, game output identical to Linux |
| GNU/Hurd x86-64 | Build failed (under actions), check passed | Cross-compiled and linked with the system's own gcc: the test suite passes in a Debian GNU/Hurd 2026-03-14 VM, game output identical to Linux; played on its console |
| GNU/Hurd i686 | Build failed (under actions), check passed | Compile-checked only |
| Redox x86-64 | Build failed (under actions), check passed | Built with Redox's own toolchain: the test suite passes in a Redox VM (2026-10-14 image, one test thread), game output identical to Linux; full-screen play works on its console |
| Android (Termux, AArch64) | Build success | Tested on actual devices. Sparsely tested, works under Termux, should also work in any terminal environment, and does run under Android Recovery (TWRP)... somehow |
| Linux RISC-V 32 | Build failed (under actions), check failed | Rust's own standard library doesn't build there at the moment (`SYS_futex` missing from libc for that target), nothing in emuera-tty |
| Linux PowerPC 32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux IBM Z (s390x) | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux SPARC 32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux SPARC 64 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 32, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 32, little-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 64, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux MIPS 64, little-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux Motorola 68000 | Build failed (under actions), check passed | Fails under Github Actions; compiles with nightly `-Z build-std` |
| Linux Hexagon | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux C-SKY | Build failed (under actions), check passed | Fails under Github Actions; compiles with nightly `-Z build-std` |
| Linux ARM, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux AArch64, big-endian | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
| Linux x32 | Build failed (under actions), check passed | Fails under Github Actions, needs testing on emulated/real hardware instead |
