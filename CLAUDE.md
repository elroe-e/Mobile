# Loss

Loss is a music player and downloader for Android, written in Rust (Dioxus UI). The goal is full parity
with the Mornye app (v5.4.0) and beyond, and above all the best lossless/FLAC player for the owner's
setup: **Xiaomi 14 Ultra (HyperOS) + Beats Studio Pro over USB-C** (headphone DAC: 32 or 48 kHz only, no 44.1; 16/24-bit;
no bit-perfect mixer on this phone, see `docs/plan/reports/`).

## Start of every session

1. `python3 scripts/check-plan.py`: shows progress and the next unchecked tasks.
2. Read `docs/plan/PLAN.md` (Status, Next up, the next tasks) and the newest entry in
   `docs/plan/PROGRESS.md`.
3. If tools are missing (fresh container): `scripts/setup-env.sh`, then export the variables it prints.
4. Work the next tasks in order. For a task, read its PARITY.md rows (search the task ID). For
   Mornye's exact behaviour, run `scripts/fetch-mornye.sh` and read the files listed in
   `docs/plan/ARCHITECTURE.md` §15.

## End of every session

1. `cargo test -p loss-core -p loss-audio -p loss-download` (plus new crates) and an Android build pass.
2. Tick finished tasks in PLAN.md; update the Status table counts; update **Next up**.
3. `python3 scripts/check-plan.py --sync` updates PARITY.md states from the ticked tasks.
4. Add a PROGRESS.md entry (what changed, what's left, gotchas).
5. Add phone checks to DEVICE-TESTS.md when behaviour can only be verified on the device.
6. `python3 scripts/check-plan.py` must pass. Commit (message starts with the task ID, e.g.
   `M1-03: Opus playback`) and push to the branch this session was given.

## Rules

* **Rust for all logic.** Kotlin only for Android entry points: the activity in
  `app/android/MainActivity.kt`, everything else in `android/glue/` (package `app.loss.glue`).
  Kotlin forwards to Rust and decides nothing. Prefer pure-Rust crates;
  any C dependency goes in ARCHITECTURE §12 with a reason.
* **Legal sources only.** Never add downloading from streaming services, DRM circumvention, stream
  decryption, or extension host functions for that. Mornye's streaming downloads are out of scope.
* **Don't copy Mornye.** Re-implement behaviour. Use Loss' own names, design and theme. Mornye is
  LGPL-3.0.
* Privacy: no telemetry; keep stats local; secrets in the vault (after M1-07), never in settings files.
* External requests use a generic user agent (`Loss/<version>`) and never include personal data.
* Write UI text plainly, from the user's side. Every new string goes through i18n once M0-09 lands.

## Layout

```
crates/loss-core       library model, tags (lofty), LRC, playlists, settings
crates/loss-audio      engine: Symphonia decode → resample → cpal/AAudio; queue, gapless (M1-06)
crates/loss-download   download manager + legal sources + post-processing
app/src/ui/            Dioxus screens (home, library, now_playing, discover, downloads, settings), theme.rs
app/src/{services,media,platform}.rs   app services, Android media bridge, JNI helpers
app/src/{diag,nav,notify}.rs           logging/crash reports, Back plumbing, downloads notification
app/android/           AndroidManifest.xml + MainActivity.kt (used verbatim by dx, see Dioxus.toml)
android/glue/          Kotlin glue as a Gradle library: services, bridge, manifest fragment, res/
app/assets/            style.css (embedded with include_str!), fonts
docs/plan/             PLAN, PARITY, ARCHITECTURE, DEVICE-TESTS, PROGRESS
scripts/               setup-env, build-android, check-plan, fetch-mornye
```

## Build and test

```sh
cargo test -p loss-core -p loss-audio -p loss-download
cargo test -p loss --no-default-features --features desktop          # app unit tests (theme…)
cargo build -p loss --no-default-features --features desktop         # desktop window for UI checks
scripts/build-android.sh                                             # APK → dist/Loss-arm64.apk
```

The installable APK is `target/dx/loss/release/android/app/app/build/outputs/apk/debug/app-debug.apk`
(release-optimised code, debug-signed). Send it to the owner with SendUserFile.

**UI checks without a phone** (no KVM here, so no emulator): run the desktop build under Xvfb
(`Xvfb :99 -screen 0 412x880x24`). Use a PulseAudio null sink for real-time audio: the ALSA null device
plays instantly. Set `HOME`/`LOSS_DATA_DIR` to a scratch dir, drive it with `xdotool`, and capture with
`import -window root`. M0-12 turns this into `scripts/ui-smoke.sh`.

## Gotchas (learned the hard way)

* Run `cargo fmt --all` right before every commit: CI fails on any formatting diff.
* CI (`.github/workflows/ci.yml`) runs fmt, clippy `-D warnings` and tests on every push. Code
  that only Android uses must still compile without warnings on desktop
  (`#[cfg_attr(not(target_os = "android"), allow(dead_code))]` or a desktop use).

* Dioxus `rsx!` format strings can't hold `if` or complex expressions; compute a variable first.
* Text inputs must use `initial_value`, not `value`: controlled inputs lose keystrokes on re-render.
* Sliders: use `ui::Slider` (holds the dragged value until the real value catches up). The animated
  fill needs `@property --pct { inherits: true }`.
* `spawn` is scoped to its component, so timers that must outlive it go through the App tick (see
  toasts).
* `asset!()` doesn't resolve under plain `cargo run`; CSS and fonts are embedded with
  `include_str!`/`include_bytes!`.
* JNI from Rust threads: load app classes through the activity's class loader; run loops inside
  `with_local_frame`.
* Gradle sometimes gets HTTP 429 from Maven Central: rerun `./gradlew assembleRelease` inside
  `target/dx/loss/release/android/app`.
* The glue module is bundled by `dx` because `app/src/glue.rs` has a `manganis::ffi` block pointing at
  `android/glue` (it becomes `:plugins:lossglue`). Permissions and services go in the glue's manifest;
  Rust looks classes up as `app.loss.glue.*` and JNI exports are `Java_app_loss_glue_*`.
* `scripts/emulator-smoke.sh <apk>` checks a build on an emulator or a phone over adb (M0-03).
* cpal 0.16 uses AAudio on Android (0.15 needed C++ Oboe). Symphonia 0.5/0.6 have no Opus decoder.
* The desktop test environment's WebKitGTK reports a light theme; set the theme in settings for
  dark-mode screenshots.
