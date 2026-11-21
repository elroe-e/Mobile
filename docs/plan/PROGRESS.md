# Progress log

Newest first. Each session adds an entry: date, tasks touched, what changed, what's left, anything the
next session must know.

## 2026-11-21 (session 2): CI and the seven problems (M0-02, M0-11, M1-01 … M1-07)

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
