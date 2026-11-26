# CLAUDE.md

Guidance for working in the Android app (`android/`): the same engine
(`../src/android.rs`) with a Kotlin UI drawn by the app. `README.md` here has
the layout, the build, the install and the on-phone behaviour; the root
`CLAUDE.md` has the engine's rules.

- **Run `./build.sh` first, `./fetch-toolchain.sh` second.** `build.sh` names
  what is missing (`missing …: run ./fetch-toolchain.sh`). Only then run
  `./fetch-toolchain.sh`: it downloads about 820 MB (270 MB kept) from
  `dl.google.com`, so don't fetch the toolchain speculatively. Toolchains are
  stored in `.toolchain`.

- **Build with `./build.sh`, never with `cargo build`.** The Android side
  compiles the engine as a shared library for `aarch64-linux-android` and
  `armv7-linux-androideabi`, with the NDK's clang as the C compiler and
  archiver, and needs `kotlinc`, a JDK and `apksigner`. `build.sh` does all
  of that setup itself (targets, linkers, `RUSTFLAGS`, `CC_*` / `AR_*`, the
  manifest, d8, zip, signing), so don't try to reproduce it by hand. Any
  change to the Kotlin code (`kotlin/*.kt`) or the Android-side Rust code
  (`../src/android.rs` and what it needs) is compiled through it; the APK
  lands in `build/emuera-tty-android.apk`.

- **Use `./build.sh --fast` while testing.** It skips link-time optimisation
  and cuts the wait for the compiler; `--arm64-only` builds one ABI instead
  of two. Don't measure performance with `--fast`.

- **On a connected phone:** `test-game.sh` (what works on a phone),
  `test-import.sh` (importing a game, saves surviving), `test-files.sh` (the
  file manager), `push-game.sh` (a game from the PC, faster than importing
  it on the phone). They need `adb` and a phone.

## System packages and binaries

To build (`./build.sh`):

| Need | Debian/Ubuntu | Fedora | Arch | Why |
|---|---|---|---|---|
| rustup (with Rust) | `rustup` | `rustup` | `rustup` | `build.sh` runs `rustup target add --toolchain stable` and `rustup run stable rustc`, so rustup itself is needed, not just cargo |
| A C compiler | `gcc` | `gcc` | `gcc` | links the host build scripts; the target C compiler is the NDK's clang, which `build.sh` sets |
| Kotlin | `kotlin` | `kotlin` | `kotlin` | `kotlinc`, for the app |
| A JDK | `openjdk-21-jdk-headless` | `java-21-openjdk-devel` | `jdk21-openjdk` | `java` runs d8 and the apksigner, `keytool` makes the debug key |
| zip | `zip` | `zip` | `zip` | packaging the APK |
| curl, unzip, sha256sum | `curl`, `unzip`, `coreutils` | `curl`, `unzip`, `coreutils` | `curl`, `unzip`, `coreutils` | only for `./fetch-toolchain.sh` |

`apksigner` itself comes from Google's build-tools in `.toolchain/`; it is a
shell script, so it needs `java` on the PATH. `bash`, `find`, `awk`, `sed`
and the coreutils (`od`, `stat`, `sort`, `tr`, `seq`) are needed too, and any
Linux has them. With Nix, the one-liner in the README has them all:
`nix shell nixpkgs#rustup nixpkgs#gcc nixpkgs#kotlin nixpkgs#jdk21
nixpkgs#apksigner nixpkgs#zip nixpkgs#unzip -c ./build.sh`.

Optional, for the tests:

- **On a connected phone:** `adb` (Debian/Ubuntu `adb`, Fedora
  `android-tools`, Arch `android-tools`), a phone with USB debugging on, and
  `zip` and `tar` for `test-import.sh` and `push-game.sh`.
- **On the PC, no phone:** `tools/paths_test.kt` needs `kotlinc` and a JDK:
  `kotlinc -nowarn kotlin/Paths.kt tools/paths_test.kt -include-runtime -d
  /tmp/paths.jar && java -cp /tmp/paths.jar
  io.github.astartes_gaming.emuera.Paths_testKt`.

  General rule: **Don't run `adb` unattended.** Prompt/ask the user
  should you want/attempt to.
