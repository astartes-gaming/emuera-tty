#!/usr/bin/env bash
# Builds the APK on ARM Linux, with no Gradle, Android Studio or aapt2.
#
#   nix shell nixpkgs#rustup nixpkgs#gcc nixpkgs#kotlin nixpkgs#jdk21 nixpkgs#apksigner nixpkgs#zip nixpkgs#unzip -c ./build.sh [--fast] [--arm64-only]
#
# --fast skips link-time optimisation: much quicker, but don't benchmark with it.
#
# Needs from Google, in .toolchain/ (./fetch-toolchain.sh gets them; see README):
# libraries from the NDK. Only those files are used: no x86-64 program is run.
set -euo pipefail
A=$(cd "$(dirname "$0")" && pwd)
T=$A/.toolchain
B=$A/build
# The engine: the folder above, or engine/ in here when this folder is checked out on its own
E=$A/engine; [ -d "$E" ] || E=$(dirname "$A")
PACKAGE=io.github.astartes_gaming.emuera
MIN_SDK=${MIN_SDK:-24}
TARGET_SDK=35
PROFILE=release
ABIS="arm64-v8a armeabi-v7a"
for o in "$@"; do
  case "$o" in
    --fast) PROFILE=fastbuild ;;
    --arm64-only) ABIS=arm64-v8a ;;
    *) echo "unknown option $o" >&2; exit 2 ;;
  esac
done

NDK=$T/ndk/android-ndk-r30/toolchains/llvm/prebuilt/linux-x86_64
SYSROOT=$NDK/sysroot/usr/lib
CLANG_LIB=$(echo "$NDK"/lib/clang/*/lib/linux)
NDK_BIN=$NDK/bin
ANDROID_JAR=$T/sdk/android.jar
R8=$(echo "$T"/dl/r8-*.jar)
APKSIGNER=$T/sdk/build-tools/36.1.0/apksigner
for f in "$ANDROID_JAR" "$R8" "$SYSROOT" "$APKSIGNER" "$NDK_BIN/lld"; do [ -e "$f" ] || { echo "missing $f: run ./fetch-toolchain.sh" >&2; exit 1; }; done
export RUSTUP_HOME=${RUSTUP_HOME:-$T/rustup} CARGO_HOME=${CARGO_HOME:-$T/cargo}
mkdir -p "$B"

# --- the engine, as a shared library for each CPU type
build_lib() {
  local abi=$1 target=$2 sys=$3 rtdir=$4 clang_target=$5
  local dir="$SYSROOT/$sys"
  rustup target add --toolchain stable "$target" >/dev/null 2>&1
  local env_target; env_target=$(echo "$target" | tr 'a-z-' 'A-Z_')
  # Use lld directly for linking (accepts all lld flags); use NDK clang wrapper as C compiler.
  local clang="$NDK_BIN/${clang_target}${MIN_SDK}-clang"
  [ -x "$clang" ] || { echo "missing NDK clang: $clang" >&2; exit 1; }
  export "CARGO_TARGET_${env_target}_LINKER=rust-lld"
  export "CARGO_TARGET_${env_target}_RUSTFLAGS=-C linker-flavor=ld.lld -L native=$dir/$MIN_SDK -L native=$dir -L native=$CLANG_LIB/$rtdir -C link-arg=$dir/$MIN_SDK/crtbegin_so.o -C link-arg=$dir/$MIN_SDK/crtend_so.o -C link-arg=-z -C link-arg=max-page-size=16384 -C link-arg=--hash-style=gnu"
  # cc-rs needs CC_<target> or CC env var to find the C compiler for building sqlite
  export "CC_${env_target}=$clang"
  export "CC=$clang"
  (cd "$E" && CARGO_TARGET_DIR="$B/target" cargo +stable rustc --lib --crate-type cdylib --profile "$PROFILE" --target "$target" 2>&1)
  local so="$B/target/$target/$PROFILE/libemuera.so"
  [ -f "$so" ] || { echo "no library for $abi" >&2; exit 1; }
  mkdir -p "$B/apk/lib/$abi"
  cp "$so" "$B/apk/lib/$abi/libemuera.so"
}
rm -rf "$B/apk"; mkdir -p "$B/apk"
for abi in $ABIS; do
  case $abi in
    arm64-v8a) build_lib "$abi" aarch64-linux-android aarch64-linux-android aarch64 aarch64-linux-android ;;
    armeabi-v7a) build_lib "$abi" armv7-linux-androideabi arm-linux-androideabi arm armv7a-linux-androideabi ;;
  esac
done

# --- the manifest
rustup run stable rustc --edition 2021 -O -A warnings -o "$B/mkmanifest" "$A/tools/mkmanifest.rs"
"$B/mkmanifest" "$B/apk/AndroidManifest.xml" "$PACKAGE" 1 0.1 "$MIN_SDK" "$TARGET_SDK" "$PACKAGE.MainActivity" "Emuera-Android"

# --- the Kotlin UI
KOTLINC=$(command -v kotlinc)
STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/lib/kotlin-stdlib.jar
# alt fallback
[ -f $STDLIB ] || { STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/share/kotlin/lib/kotlin-stdlib.jar; }
rm -rf "$B/classes"
kotlinc -no-reflect -nowarn -jvm-target 1.8 -cp "$ANDROID_JAR" -d "$B/classes" "$A"/kotlin/*.kt
find "$B/classes" -name '*.class' > "$B/classes.txt"
# (kept quiet: this d8 prints a long harmless warning per class about the newer Kotlin's metadata)
java -cp "$R8" com.android.tools.r8.D8 --release --min-api "$MIN_SDK" --lib "$ANDROID_JAR" --output "$B/apk" @"$B/classes.txt" "$STDLIB" >"$B/d8.log" 2>&1 \
  || { tail -30 "$B/d8.log" >&2; exit 1; }

# --- package and sign
UNSIGNED=$B/unsigned.apk
rm -f "$UNSIGNED"
(cd "$B/apk" && zip -q -X -r "$UNSIGNED" AndroidManifest.xml classes*.dex lib)
APKSIGNER=$T/sdk/build-tools/36.1.0/apksigner
if [ -n "${KEYSTORE:-}" ]; then
  # A release key: apksigner asks for its password, so it never lands in the shell history
  "$APKSIGNER" sign --ks "$KEYSTORE" --min-sdk-version "$MIN_SDK" --out "$B/erb-player.apk" "$UNSIGNED"
else
  KEY=$T/debug.keystore
  [ -f "$KEY" ] || keytool -genkeypair -keystore "$KEY" -storepass android -keypass android -alias debug -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=emuera-tty debug" >/dev/null 2>&1
  "$APKSIGNER" sign --ks "$KEY" --ks-pass pass:android --key-pass pass:android --min-sdk-version "$MIN_SDK" --out "$B/erb-player.apk" "$UNSIGNED"
fi
rm -f "$B/erb-player.apk.idsig"
"$APKSIGNER" verify "$B/erb-player.apk"
ls -la "$B/erb-player.apk" | awk '{print "built", $NF, int($5/1048576) " MB"}'
