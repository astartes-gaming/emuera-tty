#!/usr/bin/env bash
# Builds the APK on Linux, with no Gradle or Android Studio.
#
#   nix shell nixpkgs#rustup nixpkgs#gcc nixpkgs#kotlin nixpkgs#jdk21 nixpkgs#apksigner nixpkgs#zip nixpkgs#unzip -c ./build.sh [--fast] [--arm64-only]
#
# --fast skips link-time optimisation: much quicker, but don't benchmark with it.
#
# Needs from Google, in .toolchain/ (./fetch-toolchain.sh gets them; see README):
# libraries from the NDK. Only those files are used: other programs are used minimally.
set -euo pipefail
A=$(cd "$(dirname "$0")" && pwd)
T=$A/.toolchain
B=$A/build
# The engine: the folder above, or engine/ in here when this folder is checked out on its own
E=$A/engine; [ -d "$E" ] || E=$(dirname "$A")
PACKAGE=io.github.astartes_gaming.emuera
MIN_SDK=${MIN_SDK:-24}
TARGET_SDK=35
VERSION_CODE=${VERSION_CODE:-1}
VERSION_NAME=${VERSION_NAME:-0.1}
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
AAPT2=$T/sdk/build-tools/36.1.0/aapt2
ZIPALIGN=$T/sdk/build-tools/36.1.0/zipalign
APKSIGNER=$T/sdk/build-tools/36.1.0/apksigner
MAGICK=$(command -v magick)
for f in "$ANDROID_JAR" "$R8" "$SYSROOT" "$AAPT2" "$ZIPALIGN" "$APKSIGNER" "$NDK_BIN/lld"; do [ -e "$f" ] || { echo "missing $f: run ./fetch-toolchain.sh" >&2; exit 1; }; done
[ -n "$MAGICK" ] || { echo "missing imagemagick (magick): install imagemagick to build the app icon" >&2; exit 1; }
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
  # cc-rs needs AR_<target> or AR env var for the archiver
  export "AR_${env_target}=$NDK_BIN/${clang_target}-ar"
  export "AR=$NDK_BIN/${clang_target}-ar"
  (cd "$E" && CARGO_TARGET_DIR="$B/target" cargo +stable rustc --lib --crate-type cdylib --profile "$PROFILE" --target "$target" 2>&1)
  local so="$B/target/$target/$PROFILE/libemuera.so"
  [ -f "$so" ] || { echo "no library for $abi" >&2; exit 1; }
  mkdir -p "$B/apk/lib/$abi"
  cp "$so" "$B/apk/lib/$abi/libemuera.so"
}
rm -rf "$B/apk"; mkdir -p "$B/apk"
echo "[ANDROID] building libraries"
for abi in $ABIS; do
  case $abi in
    arm64-v8a) build_lib "$abi" aarch64-linux-android aarch64-linux-android aarch64 aarch64-linux-android ;;
    armeabi-v7a) build_lib "$abi" armv7-linux-androideabi arm-linux-androideabi arm armv7a-linux-androideabi ;;
  esac
done

# --- the app icon and resources, compiled by aapt2 into the manifest's resource table
# discover icons at build time: icon.png first (the default), then the rest
# alphabetically. Each PNG becomes a launcher-alias option — drop a new file
# into icons/ and rebuild, nothing else needs editing.
ICONS=()
[ -f "$A/icons/icon.png" ] && ICONS+=("icon.png")
for f in "$A"/icons/*.png; do
  [ -e "$f" ] || continue
  b=$(basename "$f")
  [ "$b" = "icon.png" ] && continue
  case "$b" in
    [A-Za-z0-9_]*.png) ICONS+=("$b") ;;
    *) echo "skipping $b: icon names must be alphanumeric" >&2 ;;
  esac
done
[ ${#ICONS[@]} -gt 0 ] || { echo "no icons in icons/: add at least one PNG" >&2; exit 1; }

# one icon set: source png + suffix ("" for the default, "_<name>" otherwise).
# Each is drawn into the adaptive icon's safe zone (66 dp inside a 108×108 dp
# canvas) and at the correct size per density, so nothing overshoots the mask.
make_icon() {
  local src=$1 suffix=$2
  magick "$src" -resize 66x66 -background transparent -gravity center -extent 108x108 \
    PNG32:"$B/res/drawable/ic_launcher_foreground${suffix}.png"
  for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
    case $d in
      mdpi)    s=48 ;;
      hdpi)    s=72 ;;
      xhdpi)   s=96 ;;
      xxhdpi)  s=144 ;;
      xxxhdpi) s=192 ;;
    esac
    magick "$src" -resize ${s}x${s} -background transparent \
      PNG32:"$B/res/mipmap-$d/ic_launcher${suffix}.png"
    magick "$src" -resize ${s}x${s} -background transparent \
      PNG32:"$B/res/mipmap-$d/ic_launcher_round${suffix}.png"
  done
  cat > "$B/res/mipmap-anydpi-v26/ic_launcher${suffix}.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
  <foreground android:drawable="@drawable/ic_launcher_foreground${suffix}"/>
</adaptive-icon>
EOF
}

echo "[ANDROID] building app (L1, resources)"
rm -rf "$B/res"; mkdir -p "$B/res/drawable"
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do mkdir -p "$B/res/mipmap-$d"; done
mkdir -p "$B/res/mipmap-anydpi-v26"
# adaptive icon background: none — foreground-only, fully transparent canvas.
# A transparent <shape> background can make some launchers (e.g. Lawnchair)
# paint black behind the adaptive icon instead of compositing on transparency.
# Dropping it keeps every icon clear.
# one launcher-alias per icon; exactly one is enabled at a time so there is
# always exactly one app icon. MainActivity.kt toggles them at runtime.
: > "$B/aliases.inc"
first=1
for b in "${ICONS[@]}"; do
  [ "$b" = "icon.png" ] && key=default || key=${b%.png}
  case $key in default) suffix="" ;; *) suffix="_$key" ;; esac
  echo "[ANDROID] adding icon $key ($b)"
  make_icon "$A/icons/$b" "$suffix"
  cap=$(printf '%s' "$key" | sed 's/\(.\)/\U\1/')
  enabled=$([ $first -eq 1 ] && echo true || echo false); first=0
  cat >> "$B/aliases.inc" <<EOF
    <activity-alias android:name=".Icon${cap}"
        android:enabled="$enabled"
        android:icon="@mipmap/ic_launcher${suffix}"
        android:roundIcon="@mipmap/ic_launcher_round${suffix}"
        android:targetActivity=".MainActivity">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity-alias>
EOF
done
# generated Kotlin list of icons, consumed by MainActivity.kt
{
  echo 'package io.github.astartes_gaming.emuera'
  echo
  echo 'data class AppIcon(val key: String, val label: String, val iconName: String)'
  echo
  printf 'val appIcons: List<AppIcon> = listOf(\n'
  first=1
  for b in "${ICONS[@]}"; do
    [ "$b" = "icon.png" ] && key=default || key=${b%.png}
    cap=$(printf '%s' "$key" | sed 's/\(.\)/\U\1/')
    [ $first -eq 1 ] || printf ','
    case $key in default) suffix="" ;; *) suffix="_$key" ;; esac
    printf '\n  AppIcon("%s", "%s", "ic_launcher_foreground%s")' "$key" "$cap" "$suffix"
    first=0
  done
  echo
  echo ')'
} > "$B/Icons.g.kt"
# --- the manifest (text; aapt2 compiles it together with the app icon)
echo "[ANDROID] building app (L2, manifest)"
{
cat <<EOF
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="$PACKAGE"
    android:versionCode="$VERSION_CODE"
    android:versionName="$VERSION_NAME">
  <uses-sdk android:minSdkVersion="$MIN_SDK" android:targetSdkVersion="$TARGET_SDK" />
  <uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
  <uses-permission android:name="android.permission.FOREGROUND_SERVICE_SPECIAL_USE" />
  <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />
  <application
      android:label="Emuera-Android"
      android:icon="@mipmap/ic_launcher"
      android:roundIcon="@mipmap/ic_launcher_round"
      android:debuggable="true"
      android:hasCode="true"
      android:hardwareAccelerated="true"
      android:extractNativeLibs="true">
    <!-- the game list: launched through an activity alias so alternate icons work -->
    <activity android:name=".MainActivity"
        android:exported="true"
        android:configChanges="mcc|mnc|locale|touchscreen|keyboard|keyboardHidden|navigation|screenLayout|uiMode|orientation|screenSize|smallestScreenSize|layoutDirection|density|colorMode|grammaticalGender|fontWeightAdjustment|fontScale"
        android:alwaysRetainTaskState="true" />
EOF
cat "$B/aliases.inc"
cat <<EOF
    <!-- a running game -->
    <activity android:name=".GameActivity"
        android:exported="true"
        android:permission="android.permission.DUMP"
        android:process=":game"
        android:screenOrientation="sensorLandscape"
        android:launchMode="singleTask"
        android:configChanges="mcc|mnc|locale|touchscreen|keyboard|keyboardHidden|navigation|screenLayout|uiMode|orientation|screenSize|smallestScreenSize|layoutDirection|density|colorMode|grammaticalGender|fontWeightAdjustment|fontScale" />
    <!-- the engine's settings: same screen whether opened from the list or a game -->
    <activity android:name=".SettingsActivity"
        android:configChanges="mcc|mnc|locale|touchscreen|keyboard|keyboardHidden|navigation|screenLayout|uiMode|orientation|screenSize|smallestScreenSize|layoutDirection|density|colorMode|grammaticalGender|fontWeightAdjustment|fontScale" />
    <!-- keeps a game alive in the background, so in the game's process -->
    <service android:name=".GameService"
        android:exported="false"
        android:process=":game"
        android:foregroundServiceType="specialUse" />
    <!-- the games in the system's file manager: only it holds this permission -->
    <provider android:name=".GamesProvider"
        android:permission="android.permission.MANAGE_DOCUMENTS"
        android:exported="true"
        android:authorities="$PACKAGE.games"
        android:grantUriPermissions="true">
      <intent-filter>
        <action android:name="android.content.action.DOCUMENTS_PROVIDER" />
      </intent-filter>
    </provider>
  </application>
</manifest>
EOF
} > "$B/apk/AndroidManifest.xml"

# compile each resource file into a flat (PNGs emit a zip holding the flat),
# then link them all together
FLATS=""
i=0
for f in $(find "$B/res" -type f); do
  i=$((i + 1))
  "$AAPT2" compile -o "$B/res.zip.$i" "$f"
  unzip -o -q -d "$B/res.flat.$i.d" "$B/res.zip.$i"
  FLATS="$FLATS $(find "$B/res.flat.$i.d" -name '*.flat')"
done
# shellcheck disable=SC2086
"$AAPT2" link -o "$B/res.apk" --manifest "$B/apk/AndroidManifest.xml" -I "$ANDROID_JAR" $FLATS
(cd "$B/apk" && unzip -q -o "$B/res.apk")
rm -rf "$B/res" "$B"/res.flat.* "$B/res.apk"
# --- the Kotlin UI
echo "[ANDROID] building app (L3, vars)"
KOTLINC=$(command -v kotlinc)
STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/lib/kotlin-stdlib.jar
# alt fallback
echo "[ANDROID] building app (L4, vars)"
[ -f $STDLIB ] || { echo "first STDLIB not valid, using alt"; STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/share/kotlin/lib/kotlin-stdlib.jar; }
rm -rf "$B/classes"
echo "[ANDROID] building app (L5, compile)"
kotlinc -no-reflect -nowarn -jvm-target 1.8 -cp "$ANDROID_JAR" -d "$B/classes" "$A"/kotlin/*.kt "$B/Icons.g.kt"
find "$B/classes" -name '*.class' > "$B/classes.txt"
# (kept quiet: this d8 prints a long harmless warning per class about the newer Kotlin's metadata)
echo "[ANDROID] building app (L6, d8)"
java -cp "$R8" com.android.tools.r8.D8 --release --min-api "$MIN_SDK" --lib "$ANDROID_JAR" --output "$B/apk" @"$B/classes.txt" "$STDLIB" >"$B/d8.log" 2>&1 \
  || { tail -30 "$B/d8.log" >&2; exit 1; }

# --- package: resources.arsc stays uncompressed (required on API 30+)
UNSIGNED=$B/unsigned.apk
rm -f "$UNSIGNED"
(cd "$B/apk" && zip -q -X -n .arsc -r "$UNSIGNED" AndroidManifest.xml classes*.dex lib res resources.arsc)

# --- align on 4-byte boundaries (required on API 30+)
"$ZIPALIGN" -p 4 "$UNSIGNED" "$B/aligned.apk"
mv "$B/aligned.apk" "$UNSIGNED"

# --- sign
echo "[ANDROID] building app (L7, signing)"
if [ -n "${KEYSTORE:-}" ]; then
  echo "[ANDROID] signing with release key"
  # A release key: apksigner asks for its password, so it never lands in the shell history
  "$APKSIGNER" sign --ks "$KEYSTORE" --min-sdk-version "$MIN_SDK" --out "$B/emuera-tty-android.apk" "$UNSIGNED"
else
  echo "[ANDROID] signing with debug key"
  KEY=$T/debug.keystore
  [ -f "$KEY" ] || keytool -genkeypair -keystore "$KEY" -storepass android -keypass android -alias debug -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=emuera-tty debug" >/dev/null 2>&1
  "$APKSIGNER" sign --ks "$KEY" --ks-pass pass:android --key-pass pass:android --min-sdk-version "$MIN_SDK" --out "$B/emuera-tty-android.apk" "$UNSIGNED"
fi
rm -f "$B/emuera-tty-android.apk.idsig"
echo "[ANDROID] building app (L8, finalization/verification)"
"$APKSIGNER" verify "$B/emuera-tty-android.apk"
ls -la "$B/emuera-tty-android.apk" | awk '{print "built", $NF, int($5/1048576) " MB"}'
