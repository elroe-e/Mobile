# Progress log

Newest first. Each session adds an entry: date, tasks touched, what changed, what's left, anything the
next session must know.

## 2026-11-24 (session 5, later): route and device monitor (M2-03)

- **M2-03** `loss_platform::route` (device kinds, rates, encodings, active output), Kotlin
  `LossBridge.outputs()` and an app-level `AudioDeviceCallback` (moved out of the playback
  service so it works before playback starts). `Services::refresh_route` → `Command::SetOutputCaps`;
  the engine reopens at the device's rate where the song was (test: plugging in the Beats mid-song
  reopens at 48 kHz). Settings → Audio shows "Playing through …" and the rates. With this, 44.1 kHz
  files are converted to 48 kHz by Loss (M2-07, Best) instead of the Android mixer.
- Bluetooth codec isn't in the public API; not shown.
- Next: M2-02 (native AAudio sink with I16/I24/F32 and dither), M2-06, M2-05, M2-04.

## 2026-11-24 (session 5): high-quality resampling (M2-07)

- **M2-07** `rubato` 5 resampler with four presets, exact lengths (delay trimmed, tail flushed at end
  of file), Best flat ±0.01 dB to 20 kHz and < −120 dB aliasing (tests), TPDF/high-pass dither
  (ready for M2-02's integer outputs). Settings → Audio: conversion quality and highest sample rate
  (`AudioOptions` → engine). Engine tests: 96 → 48 kHz for the Beats with exact length; a 192 kHz
  DAC capped at 96 kHz.
- On the phone nothing changes yet: cpal's default Android output says it takes every rate, so the
  file's rate is kept and Android's mixer converts. **M2-03** (device monitor: the Beats' real
  rates from AudioDeviceInfo) makes Loss do the 44.1 → 48 kHz conversion itself. Do M2-03 next.
- The worker restarted mid-task; the engine edits had not been written and were redone. The disk
  filled up (12 GB in `target/debug/deps`); cleaned build caches (see CLAUDE.md gotcha).
- Next: M2-03, M2-02, M2-06, M2-05, M2-04.

## 2026-11-22 (session 4): settings, translations, test harness (M0-08, M0-09, M0-12)

- **M0-08** `loss_core::registry`: every user setting with section, kind, keywords, platforms;
  `get`/`set` by key (validated), `search`, `INTERNAL` for app state. Tests fail if a `Settings`
  field is in neither. The settings screen is generated from it, with a search box (results
  grouped by section, editable in place). Database schema 2: one row per setting (migration from
  schema 1's single row; a bad value falls back to its default). New visible setting: retries.
- **M0-09** `crates/loss-i18n` (Fluent): `t!("id", k = v)`, English fallback, plural selectors,
  pseudo-locales `en-XA` and `ar-XB` (RTL). All ~190 UI strings plus the downloads notification
  and the registry are in `locales/en/loss.ftl`. Language follows the phone unless set in
  Settings → Look → Language. `scripts/check-i18n.py` (CI) catches hard-coded text and
  unknown/unused ids. Tab labels now shrink with an ellipsis.
- **M0-12** `scripts/make-fixtures.sh` (ffmpeg → `target/fixtures`) and
  `scripts/ui-smoke.sh` (Xvfb tour in en, en_XA, ar_XB with screenshots), both in CI, plus
  `--ignored generated` tests. Did M0-12 before M0-10 so the UI kit can be reviewed with it.
- Found by the harness and fixed: **Opus left a 6.5 ms gap at each gapless join** (Symphonia's
  Ogg reader doesn't trim the pre-skip; `opus.rs` now does). Found and recorded for M2-08: AAC/ALAC
  in MP4 leave ≈45 ms per join (edit list / iTunSMPB ignored), and ALAC bit depth isn't reported.
- Playback errors now have a kind (`loss_audio::PlayError`: output, missing, unsupported); the
  player shows a plain translated sentence and the technical detail stays in the log (it used to
  show raw cpal text such as "The requested device is no longer available…").
- Gotchas: rustfmt can move a trailing `// i18n: ignore` comment to its own line (then the checker
  flags the string); restructure the code instead. `PULSE_SERVER`/`PULSE_COOKIE` are needed when
  the app runs with its own HOME. Choice settings' labels are `choice-<key>-<value>` messages.
- **M0-13** built, not ticked (needs the owner's 30-minute screen-off check): Kotlin
  `LossReliability` reports battery exemption, background restriction, notifications and whether a
  Xiaomi Autostart screen exists; `loss_platform::reliability` turns that into checks (tested);
  Settings → Help → Background playback shows them with Fix/Open buttons and refreshes every 2 s;
  Home shows a dismissible notice while the phone limits Loss. Manifest: the battery-exemption
  permission and a `<queries>` entry for `com.miui.securitycenter`.
- CI: the UI tour first "passed" with black screenshots (WebKitGTK sandbox blocked on the runners,
  no GPU, 25 s first load). Fixed with test-only env vars and longer waits; the script now fails on
  blank screenshots. The APK build retries and caches Gradle (Maven Central 429s).
- **M0-10** started (`app/src/ui/kit.rs`): layer stack that Back pops first; `Sheet`, `SheetItem`,
  `Dialog`; snackbar with Undo (`toast_undo`; used for removing a music folder); long-press /
  right-click menu on song rows as a bottom sheet with a vibration; `haptic()` through the bridge
  (`LossBridge.haptic`); `VirtualList` (a script reports the list's position on scroll; renders
  ±1 screen, start snapped to 20 rows; tested) now drives Library → Songs, with an `Ids` handle so
  rows share one id list; `Marquee` for player and mini-player titles; a debug-only component
  gallery (also in the UI tour, in every language). The i18n checker now also catches words next to
  placeholders ("{} songs") and text after icons; four missed strings were translated.
- Found while testing the kit: container animations with `fill-mode: both` trapped fixed-position
  sheets (hidden under the bottom bar) and dialogs (off-screen); they now use `backwards`.
- **M0-10 done**: plus `VirtualGrid` (Library → Albums; pinch or Ctrl+wheel zoom saved as
  `album_grid_size`), fast-scroll handle (dragging runs in page JS for smoothness), swipe a song
  towards the end edge to play next (pointer events; `touch-action: pan-y`), keyboard focus rings
  and Enter to play, Material Symbols Rounded as a 7 KB subset font (`scripts/make-icon-font.py`;
  Apache-2.0 notice in `app/assets/fonts/`). Predictive back moved to M10-01, TV D-pad focus to
  M9-06.
- **M2-01 done**: `sink.rs` (Sink trait, CpalSink, FakeSink/FakeClock), `choose_rate` (tested for
  the Beats: everything → 48 kHz), `dsp.rs` (f64 Graph, empty = untouched), positions = samples
  heard minus output latency (cpal callback timestamps). Engine tests now drive the real engine
  against the fake sink: sample-exact positions with latency, pause, bit-exact 16-bit path, 24-bit
  exact in f32, 44.1 → 48 kHz conversion, gapless across 44.1/48 kHz files without reopening.
- Next: M2-07 (high-quality resampling), M2-02, M2-03, M2-06, M2-05, M2-04.
  M0-04 waits for D3; M0-13 and M1-08 wait for the owner's phone round.

## 2026-11-21 (session 3): foundations (M0-03, M0-05, M0-06, M0-07)

- **M0-03** CI's Android job is a matrix (arm64 for the phone, x86_64 for the emulator). The x86_64
  leg boots an API 34 emulator, installs with permissions granted, launches, and runs
  `scripts/emulator-smoke.sh` (fails on Java/Rust/native crashes, a dead process, a missing start
  line or a missing "platform bridge ready" line). Artifact `emulator-smoke` has the screenshot,
  logcat, media sessions. Android jobs run on PRs, main and manual runs: trigger `ci.yml` with
  workflow_dispatch on the branch to get them. The screenshot showed one UI flaw: at 320 px the
  "Discover"/"Downloads" tab labels touch (fix in M0-10/M10).
- **M0-05** Kotlin lives in `android/glue/` (package `app.loss.glue`), bundled by dx via the
  `manganis::ffi` block in `app/src/glue.rs` (becomes `:plugins:lossglue`). The glue manifest holds
  permissions and services; `MainActivity` stays in `app/android/` (it must extend `WryActivity`).
  First glue resource: the music-note notification icon.
- **M0-06** New `crates/loss-platform`: `Event` parsing and `FocusPolicy` (pause/resume decisions for
  calls, other apps and unplugging, 13 tests). Kotlin `LossBridge.emit` → `LossNative.onEvent`,
  registered with `RegisterNatives`; events from before registration are queued and flushed by
  `nativeReady`. The `loss-events` thread blocks on a channel; polling of commands and volume is
  gone. The `loss-media` thread still ticks (300 ms) to push state out, until M2-01 adds engine
  events. New behaviour: pausing during a call cancels the resume afterwards.
- **M0-07** New `crates/loss-store` (redb 5, `loss.redb`): `tracks`, `settings`, `kv`, schema 1,
  JSON values, migrations, newer-schema refusal, reset-aside for unreadable files, one-time import
  of `library.json`/`settings.json` (kept as `*.json.imported`; passwords scrubbed into the vault
  first). The app saves only changed songs. 50k songs load in ~0.1 s (release desktop); CI runs it.
- Device report from the owner is in `docs/plan/reports/`: the Beats take 32/48 kHz only and the phone
  has no bit-perfect mixer; M2 was re-planned around that (Loss resamples 44.1 → 48 itself).
- Not verified on the phone yet: everything Kotlin (Round M1, now including the bridge, storage
  migration and the new notification icon). The owner hasn't sent Round M1 results.
- Gotchas: a push cancels a running CI run on the same branch (concurrency group), so wait for a
  manual run to finish before pushing. `cargo clippy --target aarch64-linux-android` works locally
  with the NDK clang in `CC_aarch64_linux_android` and catches Android-only lints CI doesn't see.
- Next: M0-08 settings registry (with search), then M0-09 i18n, M0-10 UI kit, M0-12, M0-13.

## 2026-11-21 (session 2): CI and the seven problems (M0-02, M0-11, M1-01 … M1-07)

- **Device report received** (`docs/plan/reports/2026-11-21-xiaomi-beats.txt`). Android 16. The Beats
  take only 32/48 kHz (16/24-bit/float), not 44.1 kHz, and the phone offers no bit-perfect mixer for
  them. The M2 plan now resamples 44.1-family music to 48 kHz in Loss itself (M2-07 moved up) and
  treats Direct USB as the only bit-perfect route. ARCHITECTURE §4.2, M2-05/M2-07 and DEVICE-TESTS
  round M2 were updated. Round M1 results are still to come.
- **M0-02** CI on GitHub Actions: fmt, clippy `-D warnings`, tests and the plan check on every push;
  the Android APK job runs on PRs, main and manual runs to save minutes. The workspace was rustfmt'd
  and all clippy findings fixed. Each push cancels the previous run (concurrency group).
- **M1-01/M1-02** Audio focus and unplug, in the Kotlin playback service: pause on loss, pause and
  resume on a temporary loss (calls), no playing over a call, pause on `BECOMING_NOISY` and when a
  USB, wired or Bluetooth output disappears.
- Kotlin now calls Rust directly: `LossNative.command` and `LossNative.back` are `#[no_mangle]` JNI
  exports in `platform.rs`, confirmed present in `libmain.so` with `llvm-nm -D`. Polling stays as a
  fallback.
- **M1-04** Back: `nav.rs` holds a "can go back" flag and a channel. The App pops in this order:
  player, Library history (`LibNav` now keeps a page history), Discover collection, Settings page,
  then Home. Escape = Back on desktop.
- **M0-11** Diagnostics (`diag.rs`): logger to logcat + 3,000-line ring + rotating file; panic hook
  writes `logs/crashes/crash-*.txt`; Home shows a notice; Settings → Logs and diagnostics (search,
  filters, copy, share, clear, device report with every audio output's rates and formats and, on
  Android 14+, the bit-perfect formats the phone offers); "Detailed logging" setting.
- **M1-03** Opus via the pure-Rust `opus-decoder`, as a Symphonia decoder (`loss-audio/src/opus.rs`,
  `codecs()` registry), with Ogg/WebM fixtures in `crates/loss-audio/tests/fixtures/`.
- **M1-06** Gapless: `Session::handover` keeps the ring buffer; `Switch`/`Display` move the shown
  track at the exact join sample. Tested sample-exact with a split sine. Different sample rates still
  reopen the output (a short gap); M2 resamples to keep those gapless too.
- **M1-05** `LossDownloadService` (dataSync) with wake and Wi-Fi locks; the notification comes from
  `notify.rs` (tested).
- **M1-07** Vault (`loss-core/src/vault.rs`, AES-256-GCM); the key is wrapped by an Android Keystore
  key; settings never store the password (migration tested).
- Not verified on a phone yet: everything Kotlin (focus, unplug, Back, notifications, Keystore).
  That's M1-08: DEVICE-TESTS "Round M1".
- Gotchas: CI failed once on formatting, so always `cargo fmt --all` before committing. Code used
  only on Android needs a desktop allowance for clippy. `check-plan.py --sync` updates parity states.

## 2026-11-21: Plan and anchor (task M0-01)

- Compared Loss with Mornye 5.4.0 (commit `c4a1c7b`). Results: 183 items, 102 only in Mornye, 42 simpler
  in Loss, 11 different, 21 equal, 5 only in Loss, 2 left out on purpose. 7 problems were found in Loss.
  The page with the full list: https://claude.ai/artifact/VAKyMfq4LkHThhrNfGEPSM (private to the owner).
- Wrote the plan anchor: `CLAUDE.md`, `docs/plan/PLAN.md` (155 tasks in 11 milestones),
  `PARITY.md` (every item and all 174 Mornye settings mapped to tasks), `ARCHITECTURE.md`,
  `DEVICE-TESTS.md`, plus `scripts/check-plan.py`, `scripts/setup-env.sh`, `scripts/fetch-mornye.sh`.
- Research that shaped the plan:
  - Symphonia 0.6 still has no Opus decoder, so the fix uses the pure-Rust `opus-decoder` crate.
  - `nusb` has no isochronous transfers on Linux/Android, so Direct USB uses our own usbdevfs layer.
  - Pure-Rust crates exist for APE (`ape-decoder`), SMB (`smb`), resampling (`rubato`), fingerprints
    (`rusty-chromaprint`), Cast (`rust_cast`) and the database (`redb`).
  - Dioxus 0.7.10's CLI can bundle an Android Gradle module (manganis `AndroidArtifactMetadata`).
  - Beats Studio Pro over USB-C use their own DAC, up to 24-bit/48 kHz, with ANC off while wired.
- Saved a setup script on the owner's "Music" cloud environment. It runs `scripts/setup-env.sh` when a
  fresh container is built, so new sessions start with the Android SDK/NDK and `dx` ready. Unproven
  until the first fresh session: check that `dx --version` and `/opt/android-sdk/ndk/27.2.12479018`
  exist; if not, run `scripts/setup-env.sh` by hand (it takes about 20 minutes: SDK download plus
  `cargo install dioxus-cli`; prebuilt `dx` downloads from GitHub are blocked here).
- Next: M0-02 (CI), then the M1 problem fixes in the order listed in PLAN.md → Next up.

## Before the plan (sessions 1–5)

- Built the workspace: `loss-core` (library, tags, LRC, playlists, settings), `loss-audio` (Symphonia +
  cpal 0.16/AAudio), `loss-download` (resumable manager; Internet Archive, Jamendo, podcasts, Subsonic,
  direct links; tagging, lyrics, zip unpack) and the Dioxus app.
- Added background playback: Kotlin `MainActivity.kt` with PlaybackService, MediaSession and
  notification; media thread in `app/src/media.rs`.
- Redesigned the UI: dynamic colour from album art, Home tab, new Now Playing, Library chips, badges,
  Google Sans Flex.
- The volume slider now follows the phone's media volume (Android); the Slider component no longer
  jitters; added animations.
