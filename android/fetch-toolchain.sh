#!/usr/bin/env bash
# Fills .toolchain/ with what build.sh needs from Google, checked against these SHA-256 sums:
# android.jar, the r8 jar, and only the NDK's target libraries (no x86-64 program is kept).
# Downloading means accepting Google's Android SDK terms. Needs curl, unzip and sha256sum.
set -euo pipefail
T=$(cd "$(dirname "$0")" && pwd)/.toolchain
R=https://dl.google.com/android
mkdir -p "$T/dl" "$T/sdk" "$T/ndk"

get() { # url, file, sha256
  [ -f "$2" ] && echo "$3  $2" | sha256sum -c --quiet 2>/dev/null && return
  echo "fetching $1"
  curl -fL --retry 3 -o "$2.part" "$1"
  echo "$3  $2.part" | sha256sum -c --quiet || { echo "checksum mismatch: $1" >&2; rm -f "$2.part"; exit 1; }
  mv "$2.part" "$2"
}

get $R/maven2/com/android/tools/r8/8.13.25/r8-8.13.25.jar "$T/dl/r8-8.13.25.jar" 0d4f2f68c4fcbc9434eff122f41feb6fb1d98efd2faa530cec1abacab1cea03d

if [ ! -f "$T/sdk/build-tools/36.1.0/apksigner.jar" ]; then
  get $R/repository/build-tools_r36.1_linux.zip "$T/dl/build-tools_r36.1_linux.zip" a7b5889e4a79fcf3b0976bef40d401f4240fb1eed891d9d91169da1111e11d78
  unzip -q -o "$T/dl/build-tools_r36.1_linux.zip" "android-16/*" -d "$T/sdk/build-tools"
  mv "$T/sdk/build-tools/android-16" "$T/sdk/build-tools/36.1.0"
  rm "$T/dl/build-tools_r36.1_linux.zip"
fi

if [ ! -f "$T/sdk/android.jar" ]; then
  get $R/repository/platform-35_r02.zip "$T/dl/platform-35_r02.zip" 0988cacad01b38a18a47bac14a0695f246bc76c1b06c0eeb8eb0dc825ab0c8e0
  unzip -q -j -o "$T/dl/platform-35_r02.zip" android-35/android.jar -d "$T/sdk"
  rm "$T/dl/platform-35_r02.zip"
fi

if [ ! -d "$T/ndk/android-ndk-r30/toolchains" ]; then
  get $R/repository/android-ndk-r30-linux.zip "$T/dl/android-ndk-r30-linux.zip" 753611f410d002cfcd3f3dc2ef49aad532089d3180b436c060a90bf0fcb64df2
  P=android-ndk-r30/toolchains/llvm/prebuilt/linux-x86_64
  unzip -q -o "$T/dl/android-ndk-r30-linux.zip" -d "$T/ndk" android-ndk-r30/NOTICE android-ndk-r30/meta/platforms.json \
    "$P/lib/clang/*/lib/linux/aarch64/*" "$P/lib/clang/*/lib/linux/arm/*" \
    "$P/lib/clang/*/lib/linux/*-aarch64-android.*" "$P/lib/clang/*/lib/linux/*-arm-android.*" \
    "$P/sysroot/usr/lib/aarch64-linux-android/*" "$P/sysroot/usr/lib/arm-linux-androideabi/*"
  rm "$T/dl/android-ndk-r30-linux.zip"
fi
echo "toolchain ready in $T"
