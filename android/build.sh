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
for f in "$ANDROID_JAR" "$R8" "$SYSROOT" "$AAPT2" "$ZIPALIGN" "$APKSIGNER" "$NDK_BIN/lld"; do [ -e "$f" ] || { echo "missing $f: run ./fetch-toolchain.sh" >&2; exit 1; }; done
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

# --- the manifest (text; aapt2 compiles it together with the app icon)
echo "[ANDROID] writing manifest"
cat > "$B/apk/AndroidManifest.xml" <<EOF
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
      android:icon="@drawable/ic_launcher"
      android:debuggable="true"
      android:hasCode="true"
      android:hardwareAccelerated="true"
      android:extractNativeLibs="true">
    <!-- the game list -->
    <activity android:name=".MainActivity"
        android:exported="true"
        android:configChanges="mcc|mnc|locale|touchscreen|keyboard|keyboardHidden|navigation|screenLayout|uiMode|orientation|screenSize|smallestScreenSize|layoutDirection|density|colorMode|grammaticalGender|fontWeightAdjustment|fontScale"
        android:alwaysRetainTaskState="true">
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
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

# --- the Kotlin UI
echo "[ANDROID] building app (L1, vars)"
KOTLINC=$(command -v kotlinc)
STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/lib/kotlin-stdlib.jar
# alt fallback
echo "[ANDROID] building app (L2, vars)"
[ -f $STDLIB ] || { echo "first STDLIB not valid, using alt"; STDLIB=$(dirname "$(dirname "$(readlink -f "$KOTLINC")")")/share/kotlin/lib/kotlin-stdlib.jar; }
rm -rf "$B/classes"
echo "[ANDROID] building app (L3, compile)"
kotlinc -no-reflect -nowarn -jvm-target 1.8 -cp "$ANDROID_JAR" -d "$B/classes" "$A"/kotlin/*.kt
find "$B/classes" -name '*.class' > "$B/classes.txt"
# (kept quiet: this d8 prints a long harmless warning per class about the newer Kotlin's metadata)
echo "[ANDROID] building app (L4, d8)"
java -cp "$R8" com.android.tools.r8.D8 --release --min-api "$MIN_SDK" --lib "$ANDROID_JAR" --output "$B/apk" @"$B/classes.txt" "$STDLIB" >"$B/d8.log" 2>&1 \
  || { tail -30 "$B/d8.log" >&2; exit 1; }

# --- the app icon and resources, compiled by aapt2 into the manifest's resource table
echo "[ANDROID] building app (L4b, resources)"
rm -rf "$B/res"; mkdir -p "$B/res/drawable"
cp "$A/icon.png" "$B/res/drawable/ic_launcher.png"
"$AAPT2" compile -o "$B/icon.zip" "$B/res/drawable/ic_launcher.png"
unzip -p "$B/icon.zip" > "$B/icon.flat"
"$AAPT2" link -o "$B/res.apk" --manifest "$B/apk/AndroidManifest.xml" -I "$ANDROID_JAR" "$B/icon.flat"
(cd "$B/apk" && unzip -q -o "$B/res.apk")
rm -rf "$B/res" "$B/icon.zip" "$B/icon.flat" "$B/res.apk"

# --- package: resources.arsc stays uncompressed (required on API 30+)
UNSIGNED=$B/unsigned.apk
rm -f "$UNSIGNED"
(cd "$B/apk" && zip -q -X -n .arsc -r "$UNSIGNED" AndroidManifest.xml classes*.dex lib res resources.arsc)

# --- align on 4-byte boundaries (required on API 30+)
"$ZIPALIGN" -p 4 "$UNSIGNED" "$B/aligned.apk"
mv "$B/aligned.apk" "$UNSIGNED"

# --- sign
echo "[ANDROID] building app (L5, signing)"
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
echo "[ANDROID] building app (L6, finalization/verification)"
"$APKSIGNER" verify "$B/emuera-tty-android.apk"
ls -la "$B/emuera-tty-android.apk" | awk '{print "built", $NF, int($5/1048576) " MB"}'
