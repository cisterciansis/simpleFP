#!/bin/sh
# Builds the Rust engine, regenerates Swift bindings, and assembles SimpleFP.app.
# Usage: apps/macos/build.sh [debug|release]   (default: release)
set -eu
CONFIG="${1:-release}"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
LIB="$ROOT/target/release/libsimplefp_ffi.a"
GEN="$HERE/.build/uniffi"

export MACOSX_DEPLOYMENT_TARGET=15.0
cargo build --manifest-path "$ROOT/Cargo.toml" --release -p simplefp-ffi
rm -rf "$GEN" && mkdir -p "$GEN/headers"
cargo run -q --manifest-path "$ROOT/Cargo.toml" --release -p simplefp-ffi --bin uniffi-bindgen -- \
  generate --library "$LIB" --language swift --out-dir "$GEN"
mv "$GEN/simplefp_ffiFFI.h" "$GEN/headers/"
mv "$GEN/simplefp_ffiFFI.modulemap" "$GEN/headers/module.modulemap"
mkdir -p "$HERE/Sources/SimpleFP/Generated"
mv "$GEN/simplefp_ffi.swift" "$HERE/Sources/SimpleFP/Generated/"

rm -rf "$HERE/Frameworks/SimpleFPCore.xcframework"
xcodebuild -create-xcframework -library "$LIB" -headers "$GEN/headers" \
  -output "$HERE/Frameworks/SimpleFPCore.xcframework" >/dev/null

swift build --package-path "$HERE" -c "$CONFIG"
BIN="$(swift build --package-path "$HERE" -c "$CONFIG" --show-bin-path)/SimpleFP"

APP="$HERE/build/SimpleFP.app"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/SimpleFP"
cp "$HERE/Resources/Info.plist" "$APP/Contents/Info.plist"
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "built $APP"
