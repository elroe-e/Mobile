# Loss master plan

**Goal:** make Loss a complete, pure-Rust equivalent of Mornye 5.4 (every feature in
[PARITY.md](PARITY.md)), fix the problems found in the audit, and go further. Above all, Loss should be the
best app for lossless/FLAC playback, starting with the owner's setup: **Xiaomi 14 Ultra + Beats Studio
Pro over USB-C**.

This file is the anchor between sessions. Read **Status** and **Next up** first, work the next unchecked
tasks in order, and update this file before the session ends (see [CLAUDE.md](../../CLAUDE.md)).

## Scope

| Bucket | Count | Where |
|---|---|---|
| Features only in Mornye | 102 | PARITY.md, state `todo` |
| Features simpler in Loss (upgrade to full) | 42 | PARITY.md, state `todo` |
| Different approach (converge where it matters) | 11 | PARITY.md |
| Problems found in Loss | 7 (+ "never tested on a phone") | M1 below |
| Every Mornye setting | 174 | PARITY.md Appendix A |
| Loss+ extras (better than Mornye) | 12 | marked **Loss+** below |
| Kept as is (equal or Loss-only; protected by tests) | 23 | PARITY.md, state `keep` |
| Left out on purpose | 2 | streaming-service downloads and catalogue search |

## Status

| Milestone | What it delivers | Tasks | Done | Rough size |
|---|---|---|---|---|
| M0 | Foundations: CI, glue module, bridge, store, settings, i18n, UI kit, diagnostics | 13 | 11 | 4–5 sessions |
| M1 | The 7 problems fixed + phone verification | 8 | 7 | 2–3 sessions |
| M2 | Audio engine v2, hi-fi output, USB, DSP | 24 | 12 | 8–10 sessions |
| M3 | Player and queue | 16 | 0 | 3–4 sessions |
| M4 | Library v2 | 25 | 0 | 6–8 sessions |
| M5 | Lyrics v2 | 12 | 0 | 4–5 sessions |
| M6 | Downloads v2 and extensions | 15 | 0 | 4–5 sessions |
| M7 | Servers and network | 7 | 0 | 3 sessions |
| M8 | Home, discovery, stats, social, audio tools | 13 | 0 | 5–6 sessions |
| M9 | Devices and platform integrations | 8 | 0 | 5–6 sessions |
| M10 | Look, app shell, desktop, iOS, languages, release | 14 | 0 | 6–7 sessions |
| **Total** | | **155** | **28** | **≈ 50–60 sessions** |

A "session" here means one working session of the length used so far. Numbers are estimates.

## Next up

1. **M1-08** Phone round on the Xiaomi with the Beats (DEVICE-TESTS "Round M1"). Fix whatever the
   owner reports first. The newest APK also carries the glue module, the event bridge and the database.
2. **M0-04** release workflow when the owner decides D3 (signing key). **M0-13** HyperOS reliability
   (built; waiting for the owner's 30-minute screen-off check in DEVICE-TESTS round M1), **M0-04** release workflow (needs decision D3, the signing key).
3. Then **M2** (audio engine, hi-fi output for the Beats). The device report is in
   (`docs/plan/reports/2026-11-21-xiaomi-beats.txt`): the Beats take 32/48 kHz only and this phone has
   no bit-perfect mixer. So the order is ~~M2-01~~, ~~M2-07~~ (our own 44.1 → 48 kHz resampling, which affects most of
   the library), ~~M2-03~~ (the Beats' real rates, so M2-07 takes effect), ~~M2-02~~ (native AAudio output), **M2-06** (built; waiting for the owner's Beats report), then **M2-05**
   (Direct USB, the only bit-perfect route here; stream planning done, I/O waits for the report),
   ~~M2-04~~ (output modes). ~~M2-09~~ (ReplayGain). M2-08 is done but APE (needs a sample file). ~~M2-10~~ (equalizer). ~~M2-11~~ (AutoEq). ~~M2-12~~ (crossfeed). ~~M2-13~~ (speed and pitch; the in-player menu is M3-07). ~~M2-14~~ (skip silence). ~~M2-15~~ (AutoMix). Without the phone, continue with M2-16 (spatial audio).

## Open decisions (defaults apply until the owner decides)

| # | Decision | Default |
|---|---|---|
| D1 | Licence for Loss (e.g. GPL-3.0, MPL-2.0, MIT/Apache-2.0) | none yet; ask before 1.0 |
| D2 | Public GitHub releases (needed by the in-app updater) | CI artifacts only; updater reads releases once public |
| D3 | Release signing key | CI generates instructions; owner stores it as a GitHub secret |
| D4 | API keys: Last.fm, AcoustID, Discord app id, AudD/ACRCloud, concerts provider, Musixmatch | features ship with a "connect your key" field |
| D5 | GitHub Actions minutes (emulator and macOS runners cost minutes on private repos) | emulator on main-branch pushes only |
| D6 | iOS builds (need macOS runners; AltStore/SideStore distribution) | scheduled last (M10-11) |
| D7 | Translations: Claude drafts 20 languages; native-speaker review optional | drafts, marked for review |
| D8 | Unofficial lyrics providers (NetEase, QQ, Apple via proxy) | shipped but off until enabled |

## Global definition of done

A task is done when: the code is merged on the working branch; tests exist and pass (`cargo test`, plus
the UI smoke test for UI work); the Android build succeeds; new strings go through i18n (after M0-09);
user-visible behaviour is described in DEVICE-TESTS.md if it needs a phone check; PARITY.md states are
updated; this file's checkbox is ticked. Commit messages start with the task ID (`M2-07: …`).

---

## M0 Foundations

- [x] **M0-01 Plan and session anchor.** CLAUDE.md, docs/plan/*, setup scripts, parity matrix.
- [x] **M0-02 CI.** GitHub Actions: `cargo fmt --check`, `clippy -D warnings`, `cargo test` for all
  crates; Android APK build with `dx` for arm64 and x86_64 (cache cargo, Gradle, SDK); upload APKs as
  artifacts. *Done when:* the workflow is green on push and the APK artifact installs.
- [x] **M0-03 Emulator smoke test.** CI job on ubuntu with KVM: API 34 x86_64 emulator; install, launch,
  wait 20 s, screenshot, `dumpsys media_session`, logcat; fail on `FATAL EXCEPTION` or Rust panic.
  *Done when:* the job uploads a screenshot showing Home.
- [ ] **M0-04 Release workflow.** Tag `v*` → signed release APK + GitHub Release with notes from
  CHANGELOG.md. Signing key from secrets (D3). *Done when:* a test tag produces an installable signed APK.
- [x] **M0-05 Android glue module.** Move Kotlin into `android/glue/` (Gradle library: several Kotlin
  files, `res/`, manifest fragment merged by Gradle). Bundle through Dioxus' Android artifact mechanism,
  with `scripts/build-android.sh` patching as fallback. *Done when:* the APK contains glue classes and a
  glue resource (the notification icon), and the existing media service still works (Round M1).
- [x] **M0-06 Rust↔Kotlin bridge (`loss-platform`).** Typed JNI calls with cached classes loaded through
  the activity class loader. Events from Kotlin through a native `LossNative.onEvent(kind, json)`
  (registered with `RegisterNatives`), replacing command polling. Migrate media.rs. *Done when:* media
  buttons reach the engine with no polling loop.
- [x] **M0-07 Database (`loss-store`, redb).** Schema from ARCHITECTURE §5, versioned migrations, import
  of `library.json`/`settings.json`, typed accessors. *Done when:* a 50k-track synthetic library loads in
  under 1 s on desktop (under 2 s on the Xiaomi) and migration tests pass.
- [x] **M0-08 Settings registry v2.** Typed registry (key, type, default, section, i18n title and
  description, search keywords, platforms); persisted in the store; migration from current settings;
  generic settings-screen builder; settings search. *Done when:* every existing setting comes from the
  registry and search finds settings by keyword.
- [x] **M0-09 Internationalisation.** `loss-i18n` with Fluent; `t!()` in the UI; all current strings in
  `en.ftl`; locale from Android (JNI) or the OS; language picker (system default first); RTL direction.
  *Done when:* a script finds no hard-coded UI strings and a pseudo-locale renders everywhere.
- [x] **M0-10 UI kit.**
  - Router with per-tab stacks and modal layers; Android Back and predictive back.
  - Bottom sheet, dialog, context menu (long-press and right-click), snackbar with undo.
  - Virtualized list and grid (10k rows at 60 fps), swipe rows, pinch-zoom grid, fast scroller.
  - Marquee text; haptics bridge (`HapticFeedbackConstants`, `Vibrator`).
  - Material Symbols Rounded as an embedded subset font; focus management for TV and keyboard.

  *Done when:* a component gallery screen (debug builds) shows each one, and the library uses the
  virtual list.
- [x] **M0-11 Diagnostics.** `tracing` into a ring buffer and a rotating log file. Log viewer with
  search, level filter, auto-scroll, copy, share and clear. A panic hook saves a crash report, and the
  next launch offers to view or copy it. "Copy diagnostics" collects device, Android version, audio
  routes and settings without secrets. *Done when:* a forced panic in a debug build shows the report on
  next launch.
- [x] **M0-12 Test harness.**
  - `scripts/ui-smoke.sh`: Xvfb, PulseAudio null sink, desktop build, scripted tour, screenshots into
    `target/ui-smoke/`.
  - `scripts/make-fixtures.sh` (ffmpeg): FLAC 16/44.1, 24/96, 24/192; gapless MP3/AAC/Opus split-sine
    sets; ALAC; APE; DSF; CUE+FLAC; tagged covers; LRC files.

  *Done when:* both run in CI.
- [ ] **M0-13 HyperOS reliability.** Ask for the battery-optimisation exemption with an explanation;
  guide to Xiaomi "Autostart" and "No restrictions"; correct foreground-service types; a check screen
  showing each setting's state. *Done when:* the owner confirms playback and downloads survive 30 min
  with the screen off (DEVICE-TESTS M0).

## M1 Fix the problems found in the audit

- [x] **M1-01 (PRB-1) Audio focus.** Request focus on play (usage media, content music). Pause on loss,
  pause and resume on transient loss, duck or pause (setting) on "can duck", abandon on stop. Calls
  pause playback. *Done when:* the emulator test (another player takes focus) pauses Loss, and the
  owner confirms a phone call pauses and resumes.
- [x] **M1-02 (PRB-2) Becoming noisy.** Pause on `ACTION_AUDIO_BECOMING_NOISY` and when the active
  output device disappears (AudioDeviceCallback): USB unplug, Bluetooth off, wired unplug. *Done when:*
  unplugging the Beats' cable pauses playback instead of switching to the speaker.
- [x] **M1-03 (PRB-3) Opus.** `opus-decoder` (pure Rust) behind Symphonia's Ogg and MKV/WebM demuxers;
  pre-skip and gapless; R128 gain tags; seeking. *Done when:* fixture `.opus` and `.webm` files play,
  seek and join gaplessly in tests.
- [x] **M1-04 (PRB-4) Back navigation.** Minimal router: Back closes sheets and the player, then
  sub-pages, then returns to the previous tab, and only then moves the app to the background. *Done
  when:* album page → Back returns to the library on the phone.
- [x] **M1-05 (PRB-5) Background downloads.** `DownloadService` (foreground type dataSync) runs while
  downloads are active. Progress notification (overall plus current item, with pause and cancel
  actions), wake lock, stops when idle, survives swiping the app away. *Done when:* a 20-file album
  finishes with the screen off on the Xiaomi.
- [x] **M1-06 (PRB-6) Gapless playback.** Scheduler pre-opens the next track; continuous ring buffer
  when the format matches; trims encoder delay and padding (LAME/Xing, iTunSMPB, Opus pre-skip); fast
  sink reopen when rates differ. *Done when:* the split-sine fixtures join with no sample discontinuity
  (test checks the waveform) and the owner hears no gap on a live album.
- [x] **M1-07 (PRB-7) Encrypted secrets.** Vault: AES-GCM file whose key is wrapped by Android Keystore
  (Kotlin: create, wrap, unwrap only); `keyring` on desktop. Migrate the Subsonic password; future keys
  and tokens go here. *Done when:* settings files contain no secrets (test) and the server still
  connects after an app restart.
- [ ] **M1-08 (PRB-8) Phone verification round.** The owner runs DEVICE-TESTS "M1 round" on the Xiaomi
  with the Beats (USB-C): notification, lock screen, Bluetooth and USB buttons, volume, focus, unplug,
  downloads, permissions. Fix what fails. *Done when:* all M1 device checks pass.

## M2 Audio engine v2 and hi-fi output

- [x] **M2-01 Engine refactor.** Source → scheduler → DSP graph (f64) → sink. Capability negotiation
  (rates, formats, channels). Sample-accurate position (frames minus latency). A fake sink for tests.
  The integer pass-through path for bit-perfect. *Done when:* the engine test suite runs against the
  fake sink with sample-exact positions.
- [x] **M2-02 Native AAudio sink.** `ndk::audio` replaces cpal on Android:
  - formats F32, I16, I24 packed, I32
  - shared or exclusive (MMAP); performance mode (low-latency or power-saving)
  - usage and content type; target a specific device id
  - spatialization behaviour; disconnect callback → re-route

  *Done when:* the same tracks play on the Xiaomi with the reported stream format matching the request.
- [x] **M2-03 Route and device monitor.** AudioDeviceCallback events: USB, Bluetooth A2DP/LE, wired,
  speaker. Device info: name, type, sample rates, encodings, channel masks; Bluetooth codec where the
  API allows. Feeds the output picker and per-device profiles. *Done when:* plugging the Beats shows
  "Beats Studio Pro (USB)" with its rates in diagnostics.
- [x] **M2-04 Output modes.** Standard, Hi-Res, Bit-perfect (Android 14+:
  `setPreferredMixerAttributes(MIXER_BEHAVIOR_BIT_PERFECT)` for USB, verified with
  `getPreferredMixerAttributes`), Direct USB (M2-05), Exclusive for DAPs (AAudio MMAP). Mode picker
  with clear explanations; covers Mornye's playback-engine choice. *Done when:* each available mode
  reports its real path in the audio-info sheet on the Xiaomi.
- [ ] **M2-05 Direct USB driver (`loss-usb`, pure Rust).**
  - Permission flow (Kotlin); claim the streaming interface (detaching the kernel driver).
  - UAC 1/2/3 descriptor parsing; clock source ranges; set sample rate.
  - Isochronous OUT through usbdevfs URBs (`rustix`); async feedback endpoint; 44.1 kHz-family packet
    sizing.
  - Feature-unit hardware volume and mute, with a fixed-volume safety prompt; DoP for DSD.
  - Hot-unplug recovery; release on stop. Fallback: `libusb1-sys`.

  *Done when:* the Beats plays 16/48 and 24/48 bit-perfect through Direct USB, 44.1 kHz files reach
  them as 24/48 from Loss' resampler, and unplugging and replugging recovers. (The Beats have no
  44.1 kHz rate, measured 2026-11-21; a DAC that has one plays 44.1 bit-perfect.)
- [ ] **M2-06 DAC probe screen.** Lists each USB DAC's interfaces, formats, bit depths, rates and volume
  range; "Copy report" shares raw descriptors. Used to tune the Beats profile and to add descriptor
  fixtures to tests. *Done when:* the owner shares the Beats report and it's committed as a test fixture.
- [x] **M2-07 Resampling and dither.** rubato sinc presets (Fast, Medium, High, Best). Integer-ratio
  paths (96→48, 192→48, 88.2→44.1, 176.4→44.1) and rational 44.1-family → 48 kHz (160/147) for DACs
  without 44.1, like the Beats. Target rate picked from the sink's reported rates. Maximum-sample-rate
  setting. TPDF and high-pass TPDF dither when bit depth drops. *Done when:* tests show passband
  ripple under 0.01 dB to 20 kHz and aliasing under −120 dB for Best, a 24/96 file reaches the Beats
  as 24/48, and a 16/44.1 file reaches them as 24/48 from Loss (not Android's resampler).
- [ ] **M2-08 More formats.**
  - APE (`ape-decoder`); DSD: DSF and DFF readers → DoP or high-quality DSD→PCM.
  - WavPack: decide between a pure-Rust port and the binding, document it in ARCHITECTURE §12.
  - CUE virtual tracks (external `.cue` and embedded CUESHEET).
  - 32-bit float WAV/AIFF; multichannel → stereo downmix.
  - FLAC up to 32-bit/768 kHz.
  - Gapless AAC/ALAC in MP4: apply the edit list / iTunSMPB delay and padding (today ≈45 ms gap per
    join, measured by `generated_gapless_sets`); report ALAC bit depth.

  *Done when:* each format's fixture plays, seeks and shows correct info.
- [x] **M2-09 ReplayGain and normalisation.** Read RG and R128 tags; track, album or automatic mode
  (album when playing an album in order); pre-amp; clip prevention from the true peak; Mornye's "only
  turns down" rule. Off in bit-perfect paths, and the UI says why. *Done when:* the gain is applied
  sample-accurately in tests and shown in the audio-info sheet.
- [x] **M2-10 Equalizer.**
  - Graphic EQ with 5, 10, 15 or 31 ISO bands, plus parametric bands (peak, shelf, high-pass,
    low-pass, Q).
  - Bass and treble shelves, balance, pre-amp, auto headroom, true-peak limiter (−0.5 dBFS),
    compressor (threshold, ratio, attack, release).
  - Presets: built-ins, save, import and share JSON (≤ 64 KB).
  - Response-curve view; reset; bypass notice for bit-perfect.

  *Done when:* measured responses match the designed curves in tests and the UI matches Mornye's
  equalizer screens.
- [x] **M2-11 Loss+ AutoEq.** Import AutoEq parametric profiles (file or paste, plus a searchable
  built-in index of headphone models, e.g. Beats Studio Pro), applied as a parametric EQ and
  auto-applied per device. Also the editor for parametric bands by hand (the engine has them
  since M2-10). *Done when:* picking "Beats Studio Pro" loads its correction curve.
- [x] **M2-12 Loss+ Crossfeed.** BS2B-style crossfeed with three strengths, for headphones only.
- [x] **M2-13 Speed and pitch.** Pure-Rust time-stretch (WSOLA, with a phase vocoder for the best
  quality), keep-pitch toggle, ±12 semitones, presets. Unavailable on bit-perfect and Cast outputs,
  with explanations. *Done when:* tempo and pitch are accurate within 0.5% in tests.
- [x] **M2-14 Skip silence.** Detect silent runs (threshold and minimum length) and skip them, with a
  smooth join. Off while AutoMix is on, as in Mornye.
- [x] **M2-15 Crossfade and AutoMix.** Two-deck mixer; automatic or manual length; effects (crossfade,
  muffled low-pass, echo, pitch, custom); beat and tempo detection with phase-aligned, pitch-preserving
  tempo match; status badge. Skips mixing between consecutive tracks of the same album. *Done when:*
  transitions follow the selected effect and beat alignment is within 20 ms on test material.
- [ ] **M2-16 Spatial audio.** Read Spatializer availability and head-tracking support; set the AAudio
  spatialization behaviour; show state per output; never used with Direct USB or exclusive modes.
- [ ] **M2-17 Volume behaviours.** Loud-volume confirmation above a chosen level; hide-volume option;
  pause on mute and resume when the volume comes back; resume when headphones or a car connect (off,
  after a disconnect, always); fixed-volume USB warning.
- [ ] **M2-18 Audio-info sheet.** Source file (codec, container, rate, depth, channels, bitrate,
  compression), signal path stage by stage, bit-perfect yes/no with the reason, output device and its
  formats, Bluetooth codec, latency, low-latency and pro-audio support, spatial state, copy report.
- [ ] **M2-19 Music haptics.** Beat and onset detection on decoded audio drives vibration amplitude;
  sensitivity and strength settings; local and downloaded songs only.
- [ ] **M2-20 Loss+ Per-device profiles.** Remember output mode, EQ preset, AutoEq, ReplayGain,
  crossfeed and volume per output device; apply automatically on connect (e.g. "Beats Studio Pro
  (USB)" vs Bluetooth vs speaker).
- [ ] **M2-21 Desktop outputs.** WASAPI exclusive (windows crate), ALSA direct to a hw device,
  CoreAudio hog and integer mode, device picker.
- [ ] **M2-22 Loss+ Power.** Larger buffers and power-saving mode with the screen off, low latency when
  the UI is visible; measure battery drain per hour on the Xiaomi (DEVICE-TESTS).
- [ ] **M2-23 Resume after restart.** Persist queue, index, position, shuffle and repeat; restore
  paused on launch; "Continue listening" resumes the real queue.
- [ ] **M2-24 Listening-time accounting.** Play events with milliseconds actually heard (pauses,
  buffering and seeks excluded), source and completion. Feeds statistics, Rewind, scrobbling and
  autoplay learning.

## M3 Player and queue

- [ ] **M3-01 Player layouts.** Artwork style setting (default, card, full screen); compact player bar;
  landscape two-pane player; album art beside lyrics in landscape.
- [ ] **M3-02 Wavy seek bar.** Animated wave while playing, flat when paused; precise dragging (keeps
  the current jitter-free behaviour); haptic ticks.
- [ ] **M3-03 Swipe to change song.** Artwork and mini player, peeking at the next and previous cover,
  haptic commit, undo on accidental swipe.
- [ ] **M3-04 Love button** in the player, mini player menu and notification (uses M4-10).
- [ ] **M3-05 Player song menu.** Details, share (file, link, text), go to album or artist, open in
  another app, add to playlist, sleep timer, speed and pitch, audio info.
- [ ] **M3-06 Sleep timer.** Presets and custom minutes, end of song, optional fade-out, countdown in the
  notification, cancel.
- [ ] **M3-07 Speed and pitch sheet** (UI for M2-13), with an explanation when the output can't do it.
- [ ] **M3-08 Output picker.** Current output, system routes, Cast devices (M9-03), Link devices
  (M9-04), button for Android's output switcher.
- [ ] **M3-09 Lyric line under the title.** Current line preview; tap opens lyrics; setting.
- [ ] **M3-10 Queue v2.** Drag to reorder (handle and long-press), swipe to remove, lock queue,
  no-duplicates option (moves instead of adding twice), clear, save as playlist; sections: Now playing,
  Next in queue, Autoplay.
- [ ] **M3-11 Mini player v2.** Swipe gestures, marquee title, progress, love and next.
- [ ] **M3-12 Volume UI.** Show or hide the slider; loud-volume dialog; DAC hardware-volume indicator.
- [ ] **M3-13 Cover fly-in.** Shared-element (FLIP) transition from lists and cards into the player and
  album pages; setting to turn it off.
- [ ] **M3-14 Lyrics on the lock screen.** MediaSession title shows the current line while locked
  (setting); restored afterwards.
- [ ] **M3-15 Autoplay.** When the queue ends, continue with related songs from the library (artist,
  genre, era, song-analysis similarity). Personalised ranking learns from keeps and skips on the device.
  Randomness slider; reset learning; accuracy status; background song analysis (only while charging,
  optional).
- [ ] **M3-16 Loss+ Visualizer.** Optional real-time spectrum on Now Playing (off by default, GPU-cheap
  canvas).

## M4 Library v2

- [ ] **M4-01 Library on the database.** Incremental scanner with checkpoints (pause, resume, cancel),
  progress notification through DownloadService, force full scan, enable/disable local library.
- [ ] **M4-02 Folder picker and storage.** SAF tree picker with persisted permissions; files opened as
  fds from `ParcelFileDescriptor` (fast reads in Rust); SD cards and USB drives; offline state
  ("Reconnect the storage"); optional "All files access".
- [ ] **M4-03 Automatic scanning.** Each app open, daily or weekly (WorkManager); optional MediaStore
  observer.
- [ ] **M4-04 Scan problems.** Unreadable-files report with reasons; clean up missing entries; relink
  moved downloads; find moved files.
- [ ] **M4-05 Views.** Folders browser, Genres, grid/list per view, pinch to resize, fast scroller,
  default view (last used).
- [ ] **M4-06 Sorting and filters.** All of Mornye's sorts. Filters for quality (Hi-Res, CD, lossy),
  format, source (downloaded, local, network) and metadata gaps (year, genre, album artist,
  track/disc number, artist, ISRC, label, lyrics, ReplayGain), plus incomplete albums. Quality-label
  mode setting.
- [ ] **M4-07 Search v2.** Songs, albums, artists, playlists and folders; full-text search inside
  lyrics; recent searches; result sorting.
- [ ] **M4-08 Multi-select.** Play, queue, add to playlist, love, delete, convert, add/remove
  ReplayGain, fix tags, set common tags, export M3U8, save offline/upload.
- [ ] **M4-09 Song gestures.** Swipe one way to play next and the other to add to queue (with
  haptics); "tapping a song" setting (play or details); swipe toggle.
- [ ] **M4-10 Collections.** Loved, Wishlist and Favourite artists, with screens, hearts on
  songs/artists, and Home shortcuts.
- [ ] **M4-11 Playlists.**
  - Create, rename, delete (also in batch); custom cover (pick or remove); drag to reorder.
  - Create from a folder.
  - Import M3U/M3U8/CSV into a playlist with a match report; export M3U8 with relative paths.
  - "Suggested songs" under a playlist.
- [ ] **M4-12 Album page v2.** Description (MusicBrainz or Wikipedia, or an extension), disc headings,
  song sort (album order, most played, custom), missing-track count, find artwork, release type, label,
  year, per-track quality.
- [ ] **M4-13 Artist page v2.**
  - Online info from metadata providers: popular songs, albums, singles and EPs, compilations, latest
    release.
  - Listener counts where a provider offers them.
  - Concerts (M8-05), favourite button.
  - Offline snapshot ("as you last saw it"); setting to show songs on artist pages.
- [ ] **M4-14 Duplicate finder.** Group by acoustic fingerprint (`rusty-chromaprint`, **Loss+**) plus
  tags and duration. Quality ranking; keep the best per group or for all; confirm deletions; duplicate
  indicator.
- [ ] **M4-15 Delete songs.** Device files (MediaStore delete request, SAF, direct), network files where
  supported, history clean-up, "remove empty folders" option.
- [ ] **M4-16 Song details.** File info (path, size, format, codec, container, bitrate, compression),
  audio info, every tag, ReplayGain values, lyrics, platform links (M8-07), copy actions.
- [ ] **M4-17 Tag editor.**
  - Every field: title, artists, album, album artist, date, track and disc numbers and totals, genre,
    composer, comment, ISRC, label, copyright, UPC, explicit, BPM, mood, release type, lyrics.
  - Cover: replace, remove, resize.
  - Artist separators and tag modes; batch "set common values"; writes through lofty for every
    supported container.
- [ ] **M4-18 Fix tags from online.** MusicBrainz (ISRC, recording and release matching), Cover Art
  Archive, optional public metadata, extension providers. Review screen with per-field diffs and
  selection; modes "fill missing", "update selected", "ISRC only"; apply; undo from a snapshot.
- [ ] **M4-19 Find missing covers.** Library-wide job; option to include animated covers; "remove found
  artwork".
- [ ] **M4-20 Convert format.**
  - FLAC (`flacenc`, compression levels), ALAC (encoder research), WAV/AIFF.
  - AAC and Opus through Android MediaCodec (NDK); MP3 encoder decision documented in
    ARCHITECTURE §12.
  - Bit-depth and sample-rate caps using the M2-07 resampler and dither.
  - Keep the original, or add as a separate entry; batch conversion with progress.
- [ ] **M4-21 CUE.** Split a CUE album into FLAC tracks; virtual-track playback (M2-08).
- [ ] **M4-22 Reorganise files.** Apply folder and filename templates to existing files, with a preview
  ("for example"), move with progress, and reports of skipped files.
- [ ] **M4-23 External player.** Setting to open songs in another app (built-in or external).
- [ ] **M4-24 Scale.** 50k-track library: virtualized views, cover-cache limits, startup under 1.5 s
  warm on the Xiaomi, scans under 10 min for 50k local files.
- [ ] **M4-25 Loss+ Smart playlists.** Rule-based playlists (genre, year, rating/plays, added date,
  quality, format, path), live updating.

## M5 Lyrics v2

- [ ] **M5-01 Lyrics model and parsers.** Lines and words with timing, translations, romanization,
  speakers and duet parts, background vocals. Parsers for LRC, enhanced LRC (word timing), TTML,
  ID3 SYLT/USLT and Vorbis LYRICS. A sidecar JSON for translations, keeping plain `.lrc` compatible.
- [ ] **M5-02 Providers.**
  - LRCLIB (exists); NetEase (with word-synced YRC, translation, romanization); QQ Music (QRC);
    Musixmatch (user key).
  - Apple Music TTML and Zarz-LRC via community proxies: optional, off by default (D8).
  - Extension providers.
  - Priority screen (enable, disable, reorder, unsaved-changes guard); "best timing first" or "order
    only".
- [ ] **M5-03 Choose a result.** Search with an editable title and artist; results show their type
  (word-synced, synced, plain, instrumental); preview.
- [ ] **M5-04 Karaoke view.**
  - Word sweep, background vocals, duet alignment, gap dots, smooth scroll.
  - Blur and text-size settings.
  - Full-screen mode with controls hiding after 5 s (respecting accessibility).
  - Keep-screen-on (`FLAG_KEEP_SCREEN_ON` through the bridge).
- [ ] **M5-05 Timing offset.** Per song and global, ±0.1 s steps, "In sync" reset.
- [ ] **M5-06 Romanization.** Korean (Revised Romanization), Japanese (`kakasi`: kana and kanji),
  Chinese (`pinyin`), Cyrillic (ISO 9 tables), Devanagari and Gurmukhi (ISO 15919). "Romanized as main
  line"; show/hide pronunciation.
- [ ] **M5-07 AI translation.**
  - OpenAI-compatible client (OpenAI or a custom server such as Ollama or LM Studio): model list, test
    connection, key in the vault.
  - Fast and synced (word-aligned) modes; second language; extra instruction.
  - Auto-translate; AI romanization for scripts done offline nowhere else.
  - Save to the song file or the sidecar; auto-save; translate downloads; clear saved translations.
- [ ] **M5-08 Lyrics editor.** Write or paste; tap to set time from playback; ±0.1 s nudges;
  insert/delete lines; remove time; play from a line; warnings for out-of-order lines; save.
- [ ] **M5-09 Saving lyrics.** Embed, `.lrc` or both; lyrics on downloads; NetEase translation and
  romanization options; multi-person lyrics; preserve Apple eLRC word timing; Musixmatch language.
- [ ] **M5-10 Share lyrics as an image.** Pick up to 5 lines. Card styles: artwork, cover colour, dark
  and light. Rendered to PNG in Rust (`tiny-skia` + `cosmic-text`), then the share sheet.
- [ ] **M5-11 Instrumental detection.** From provider flags and metadata, with clear empty states.
- [ ] **M5-12 Lyrics search index.** Full-text index over saved and embedded lyrics (with M4-07).

## M6 Downloads v2 and extensions

- [ ] **M6-01 Download history.** Database-backed history with album, single and playlist sections;
  filters and sort; clear; turn history off; export failed downloads.
- [ ] **M6-02 Network rules.** Wi-Fi only (pause on mobile data, resume on Wi-Fi), with connectivity
  events through the bridge.
- [ ] **M6-03 Quality choice.** Picker when a source offers several formats (Internet Archive formats,
  Jamendo MP3 or FLAC, Subsonic original or transcode); "ask before download"; default quality.
- [ ] **M6-04 Naming templates.**
  - Tags with padding and date patterns.
  - Singles folder and singles filename format.
  - Album folder layouts: Artist/Album, Artist/[Year] Album, Album, [Year] Album, Artist/Album with
    Singles, singles flat.
  - Album artist or track artist; primary artist only; contributing-artist filter; playlist folders.
  - Live preview.
- [ ] **M6-05 Post-processing.** ReplayGain scan and embed; artist-tag modes (joined, split, primary);
  metadata separators; embedded cover size limit; save `folder.jpg` with a size limit; embed lyrics or
  write `.lrc`.
- [ ] **M6-06 Convert after download** (uses M4-20); re-download files that fail the fake hi-res check
  (uses M8-08).
- [ ] **M6-07 Skip what you have.** Match by ISRC, tags or fingerprint; "allow quality variants" option.
- [ ] **M6-08 Save to a NAS.** Upload downloads to an SMB or WebDAV folder (uses M7); network download
  destination setting.
- [ ] **M6-09 Download folder picker.** SAF picker, recovery when access is lost, optional "All files
  access".
- [ ] **M6-10 Albums, discographies, podcasts.** Whole albums and artist discographies from Internet
  Archive, Jamendo and Subsonic; **Loss+** podcast subscriptions that download new episodes
  automatically.
- [ ] **M6-11 Downloads widget and notifications** (with M1-05 and M9-01).
- [ ] **M6-12 Extension runtime.** `wasmi` sandbox with fuel and memory limits. Manifest declares
  capabilities (metadata, lyrics, artwork, home feed, search, URL handler, legal download catalogue)
  and allowed domains. Host API: fetch, storage, settings and account forms, browser sign-in callback
  (in-app or external browser setting), logging. No crypto or decryption host functions.
- [ ] **M6-13 Extension store.** Add repositories (GitHub URL with a signed `registry.json`); browse,
  search and filter by capability; install, update, uninstall; auto-update; health status; "no longer
  in its repository" state; "show Repo tab" setting.
- [ ] **M6-14 Provider priority.** Metadata priority, lyrics priority, search source, home-feed source
  and country, fallback order for downloads (legal sources only).
- [ ] **M6-15 Extension SDK.** Docs plus three example extensions (MusicBrainz metadata, LRCLIB lyrics,
  Internet Archive catalogue) that exercise the API.

## M7 Servers and network

- [ ] **M7-01 Connections.** Subsonic/OpenSubsonic, SMB 2/3 (`smb` crate, encryption), WebDAV, HTTP(S)
  file or listing URLs. Add, edit, remove and test; credentials in the vault.
- [ ] **M7-02 Streaming reader.** HTTP Range and SMB reads behind the engine's media source, with
  read-ahead cache, seeking, buffering state, reconnect, and online/offline badges.
- [ ] **M7-03 Subsonic v2.** Browse artists, albums, playlists and genres; search; stream original or
  transcoded; artwork; scrobble to the server; sync the catalogue into the library; three sign-in
  methods (salted token, password, API key); "more albums".
- [ ] **M7-04 Folder browsing.** SMB, WebDAV and HTTP folders: play folder, add folder to the library
  (remote tag and cover reads), upload (M6-08), delete where supported.
- [ ] **M7-05 Offline copies.** Save offline, with badges (server only, server and device, device only),
  remove the offline copy, delete the server copy.
- [ ] **M7-06 Self-signed HTTPS.** Show the certificate fingerprint, trust it per connection, warn when
  it changes.
- [ ] **M7-07 Network policy.** Cleartext only for approved hosts and the local network (replaces
  today's allow-all); local-network access switch; compatibility mode.

## M8 Home, discovery, stats, social, audio tools

- [ ] **M8-01 Home v2.** Greeting sets for each time of day with your name; search bar (library,
  sources, pasted links); shortcut tiles (4 slots chosen from 9, swap, reset); "From your library";
  import-playlist button.
- [ ] **M8-02 Mixes.** Daily mixes (Favourites, per genre, per artist, per decade, New additions,
  Rediscover, Discover) with generated covers; save as playlist.
- [ ] **M8-03 Explore feed** from extension home-feed providers, with country choice and caching.
- [ ] **M8-04 New releases** for favourite artists (MusicBrainz and ListenBrainz), period choice,
  partial-failure handling.
- [ ] **M8-05 Concerts.** Upcoming shows on artist pages and in a list (Bandsintown or Ticketmaster
  key, or an extension). Details with venue, date and set list (setlist.fm), ticket link, add to
  calendar, open maps.
- [ ] **M8-06 Identify a song.** Microphone capture (foreground service type microphone); AudD or
  ACRCloud with the user's key; result actions (play or search in Loss, open elsewhere); recently
  identified list; clear error states.
- [ ] **M8-07 Open in other services.** Song.link with a region setting, from details and menus.
- [ ] **M8-08 Audio tools.**
  - **Analysis:** rate, codec, container, depth, channels, Nyquist, size, DR, peak, RMS, LUFS, true
    peak, clipping, spectral cutoff, per-channel stats, spectrogram export.
  - **Hi-res check:** cutoff vs rate, mirrored upsampling, effective bits, lossy frame patterns, FLAC
    MD5 integrity, damaged files; album and batch runs; share report.
  - **Null test:** file picker, FFT alignment, level and polarity match, drift and length checks,
    difference over time and difference spectrogram, explanations.
  - **ReplayGain:** add or remove for selections and albums.
  - **Loss+ library-wide FLAC integrity check** in the background.
- [ ] **M8-09 Listening statistics.** Record-listening switch; time, songs, artists, listening days and
  streak; last-30-days and weekday charts; top artists, songs and albums; music taste by genre; year
  navigation; clear.
- [ ] **M8-10 Rewind.** Period choice (last weeks or months, year so far, year-end); story slides (time,
  top artist, rotation, top song, repeats, album, discoveries, rhythm, biggest and favourite day,
  longest streak, month by month, artist of the month, where your year began); share cards as PNG;
  "ready" notification.
- [ ] **M8-11 Profile.** Name and photo, listening since, summary.
- [ ] **M8-12 Scrobbling.** Last.fm (browser auth, now playing, scrobble rules, offline queue, error
  states) and ListenBrainz (token, custom server, https rule).
- [ ] **M8-13 Discord status.** Desktop through IPC (`discord-rich-presence`); Android through Discord's
  Social SDK (D4 application id), with artwork and progress, plus link and unlink.

## M9 Devices and platform integrations

- [ ] **M9-01 Home-screen widgets.** Player widget in 4 layouts (compact, controls, wide, large) with a
  configuration screen, Material You colours, artwork and progress. Download-queue widget in two sizes.
  Updates from Rust, throttled.
- [ ] **M9-02 Android Auto.** Media browser tree (Recent, Playlists, Albums, Artists, Songs), search,
  MediaSession playback, artwork through CoverProvider.
- [ ] **M9-03 Google Cast (pure Rust).** mDNS discovery, CASTV2 sender (`rust_cast`), Default Media
  Receiver. Local HTTP server (`axum`) with Range support and transcoding for formats Cast can't play.
  Position and volume sync, device picker, stop casting.
- [ ] **M9-04 Loss Link.**
  - Pairing (6-digit code, pinned TLS); mDNS discovery; device list.
  - Browse and stream another device's library; remote control; play together (clock sync, "phone
    timing" offset).
  - Send and save songs, keep on this phone; manage files on the computer from the phone.
  - Subsonic-compatible server with app keys; host settings (name, address, security code, paired
    phones); hide disconnected songs.
- [ ] **M9-05 Open files and share targets.** VIEW `audio/*` (play, play folder), SEND and SEND_MULTIPLE
  (audio → add or play; links → Discover, podcast or direct link), PROCESS_TEXT → search, "Open audio"
  button.
- [ ] **M9-06 Android TV / Google TV.** Leanback launcher entry and banner, TV shell (navigation rail,
  D-pad focus, large controls), landscape player, remote media keys. (From M0-10: D-pad focus
  order across the kit's components; keyboard focus rings and Enter-to-play exist already.)
- [ ] **M9-07 Tablets and large screens.** Navigation rail, two-pane list/detail, landscape player,
  multi-window and resizable windows.
- [ ] **M9-08 Desktop system integration.** OS media controls (`souvlaki`: MPRIS, SMTC, Now Playing),
  Windows taskbar buttons, notifications.

## M10 Look, app shell, desktop, iOS, languages, release

- [ ] **M10-01 Look v2.** (From M0-10: Android 14+ predictive back animation, via
  `OnBackInvokedCallback` in the glue forwarding to `LossNative.back`.) Wallpaper colours (Material You system palette, Android 12+) next to
  album-art colours (keep Loss' whole-app colouring as an option); accent picker; AMOLED black; import
  a TTF/OTF font (≤ 10 MB) or use the system font; interface size; animation speed; blur options
  ("always use blur"); cover fly-in toggle; expressive shapes.
- [ ] **M10-02 Glass style.** Loss' own second style: frosted translucent surfaces with backdrop blur,
  accent set, clarity slider, outline toggle, high-contrast fallback.
- [ ] **M10-03 Animated covers.** Video covers from metadata providers and extensions; save offline
  option; data-saver aware.
- [ ] **M10-04 First run.** Setup wizard (language, permissions: audio, notifications, nearby devices,
  storage folder), skippable tutorial, "What's new".
- [ ] **M10-05 In-app updater.** GitHub Releases with stable and preview channels; download with a
  progress notification; PackageInstaller install; "don't remind"; required-update policy (needs D2).
- [ ] **M10-06 Backup and restore.** Pick categories (settings, history, collections with playlist
  covers, extensions and their settings, optional credentials); preview; restore; restart prompt.
- [ ] **M10-07 Storage and cache page.** Sizes for each cache (covers, library covers, analysis,
  explore, lookups, temp, app cache); clear each or all; clean up orphaned history and library entries.
- [ ] **M10-08 Settings completeness.** Every setting in PARITY Appendix A works; settings search;
  About page with contributors, translators, third-party licences (generated with `cargo-about`) and
  links.
- [ ] **M10-09 Languages.** 21 locales (ar, de, en, es, es-ES, fr, hi, id, it, ja, ko, nl, pt, pt-PT, ru,
  tr, uk, vi, zh, zh-CN, zh-TW); RTL for Arabic; plural rules; per-app language setting (Android 13+).
- [ ] **M10-10 Desktop apps.**
  - Linux AppImage/deb, Windows MSI, macOS DMG, built in CI.
  - Desktop UI: sortable song tables, context menus, drag-and-drop import, windowed, full-screen and
    borderless modes, mini-player window.
  - Keyboard shortcuts page, desktop updater, exclusive outputs (M2-21).
- [ ] **M10-11 iPhone and iPad.** Dioxus iOS build on macOS CI; background audio (AVAudioSession via
  `objc2`); Now Playing and remote commands; document-picker import; AirPlay picker; AltStore and
  SideStore source. WidgetKit widgets need Swift (optional).
- [ ] **M10-12 Licence and notices.** Apply D1; NOTICE and third-party licences in the app.
- [ ] **M10-13 Accessibility.** TalkBack labels through ARIA, contrast checks, focus order, text scaling.
- [ ] **M10-14 1.0 audit.** Re-run the Mornye comparison against its latest version (diff the string
  file), update PARITY.md, check performance budgets, crash-free soak test.

## Loss+ extras (better than Mornye)

| Extra | Task |
|---|---|
| Pure-Rust direct USB driver with live bit-perfect verification and a DAC capability viewer | M2-05, M2-06 |
| Best-quality sinc resampling to the DAC's own rates (44.1 → 48 kHz for the Beats, integer ratios for hi-res) + TPDF dither, so Android's resampler never runs | M2-07 |
| Parametric EQ + AutoEq headphone profiles | M2-10, M2-11 |
| Per-output-device profiles | M2-20 |
| Crossfeed for headphones | M2-12 |
| Battery-aware buffering | M2-22 |
| Spectrum visualizer | M3-16 |
| Acoustic-fingerprint duplicates and tag matching | M4-14, M4-18 |
| Smart playlists | M4-25 |
| Podcast subscriptions with auto-download | M6-10 |
| Library-wide FLAC integrity check | M8-08 |
| Whole-app album-art colours (already in Loss, kept) | M10-01 |
