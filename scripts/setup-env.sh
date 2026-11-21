#!/usr/bin/env bash
# Prepares a fresh Ubuntu 24.04 container to build, test and package Loss.
# Safe to re-run: every step skips what is already installed.
# First run downloads the Android SDK/NDK (~2 GB) and builds the Dioxus CLI (~10-15 min).
set -euo pipefail

SDK="${ANDROID_HOME:-/opt/android-sdk}"
NDK_VER="27.2.12479018"
DX_VER="0.7.10"
CMDTOOLS_ZIP="https://dl.google.com/android/repository/commandlinetools-linux-13114758_latest.zip"

echo "== system packages"
pkgs=(libasound2-dev pkg-config libwebkit2gtk-4.1-dev libxdo-dev xvfb imagemagick xdotool
      pulseaudio libasound2-plugins ffmpeg unzip curl)
command -v java >/dev/null || pkgs+=(openjdk-21-jdk-headless)
missing=()
for p in "${pkgs[@]}"; do dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p"); done
if [ ${#missing[@]} -gt 0 ]; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${missing[@]}"
else
  echo "already installed"
fi

echo "== Android command-line tools"
if [ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
  mkdir -p "$SDK"
  (
    cd "$SDK"
    curl -sSfL -o cmdtools.zip "$CMDTOOLS_ZIP"
    unzip -q cmdtools.zip && rm cmdtools.zip
    mkdir -p cmdline-tools/latest
    mv cmdline-tools/bin cmdline-tools/lib cmdline-tools/NOTICE.txt cmdline-tools/source.properties cmdline-tools/latest/
  )
else
  echo "already installed"
fi

echo "== Android SDK platform 35, build-tools 35, NDK $NDK_VER"
if [ ! -d "$SDK/ndk/$NDK_VER" ] || [ ! -d "$SDK/platforms/android-35" ] || [ ! -d "$SDK/build-tools/35.0.0" ]; then
  yes | "$SDK/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$SDK" --licenses >/dev/null || true
  "$SDK/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$SDK" \
    "platform-tools" "platforms;android-35" "build-tools;35.0.0" "ndk;$NDK_VER"
else
  echo "already installed"
fi

echo "== Rust Android targets"
rustup target add aarch64-linux-android x86_64-linux-android

echo "== Dioxus CLI $DX_VER"
if command -v dx >/dev/null && dx --version | grep -q "$DX_VER"; then
  echo "already installed"
else
  cargo install dioxus-cli --version "$DX_VER" --locked
fi

JAVA_HOME_GUESS="$(dirname "$(dirname "$(readlink -f "$(command -v java)")")")"
echo
echo "Done. For Android builds, export:"
echo "  export ANDROID_HOME=$SDK ANDROID_NDK_HOME=$SDK/ndk/$NDK_VER JAVA_HOME=$JAVA_HOME_GUESS"
