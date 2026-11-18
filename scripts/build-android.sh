#!/usr/bin/env bash
# Builds the Loss APK for 64-bit ARM Android phones.
# Needs: Rust + the aarch64-linux-android target, the Dioxus CLI 0.7.10,
# Android SDK (platform 35, build-tools 35) + NDK 27, and JDK 17+.
set -euo pipefail
cd "$(dirname "$0")/../app"

export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.2.12479018}"

dx bundle --platform android --release --target aarch64-linux-android

APK=../target/dx/loss/release/android/app/app/build/outputs/apk/debug/app-debug.apk
mkdir -p ../dist
cp "$APK" ../dist/Loss-arm64.apk
echo "APK: $(cd ../dist && pwd)/Loss-arm64.apk"
