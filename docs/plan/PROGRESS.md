# Progress log

Newest first. Each session adds an entry: date, tasks touched, what changed, what's left, anything the
next session must know.

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
