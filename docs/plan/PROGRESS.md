# Progress log

Newest first. Each session adds an entry: date, tasks touched, what changed, what's left, anything the
next session must know.

## 2026-11-26 (session 5, later): volume behaviours (M2-17)

- `FocusPolicy` (loss-platform, pure and tested) now also decides: pause when the volume reaches
  zero and resume when it comes back (only if the mute caused the pause; a manual play/pause
  cancels it); resume when headphones/DAC/Bluetooth connect (Never / After a disconnect, i.e. only
  when unplugging paused it / Always, never over music that already plays). Outputs Android lists
  at app start look like "added": a resume decided in the first 4 s is dropped.
- Loud-volume confirmation (`needs_loud_confirmation`: only when raising across the limit,
  default 80%, 0 = off) in the player's volume slider, with a dialog. Hide-volume setting removes
  the slider from the player. The glue reports `AudioManager.isVolumeFixed`; with a USB output the
  Audio card warns that the phone's volume won't change it.
- Not covered: the phone's own volume buttons (Android's safe-volume prompt handles those), and
  the volume control in the notification or lock screen.
- Local Android lint: `CC_aarch64_linux_android=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang
  AR_aarch64_linux_android=…/llvm-ar cargo clippy -p loss --target aarch64-linux-android
  --no-default-features -- -D warnings`. Don't `export $(scripts/setup-env.sh)`: it prints the whole
  environment.

## 2026-11-26 (session 5, later): spatial audio (M2-16)

- The glue reads the system `Spatializer` (Android 12+) into the mixer JSON: immersive level,
  enabled in the system settings, available, head tracker, and whether the current media route
  (stereo float PCM) can be spatialized. `loss_platform::modes::spatial_state` turns that plus the
  setting and the output mode into one of: no spatializer, off, blocked by the mode, off in the
  phone's settings, not this output, on (with or without head tracking). Settings → Audio shows it
  under the output row.
- The AAudio stream already carried the spatialization behaviour (AUTO or NEVER); now it is NEVER
  in Exclusive, Bit-perfect and Direct USB modes whatever the setting says.
- Nothing here can be checked without the phone: see DEVICE-TESTS. The Kotlin is compiled by the
  CI APK build only.

## 2026-11-26 (session 5, later): AutoMix (M2-15)

- **Held-back tail.** With AutoMix on, a track keeps its last N seconds (N = the AutoMix length,
  6 s when automatic, at most half the track) out of the ring buffer; the point where holding
  starts comes from the track's known duration, so an inexact duration just holds a little more
  or less. At the handover the tail is mixed into the first N seconds of the next track
  (`mixer::Blend`): the outgoing side gets an effect and a fade-out, the new track is faded in
  (equal power, soft clip over 0.95 so two loud masters can't clip). The Display switches to the
  next track where the mix starts.
- **Effects:** crossfade, muffled (a low-pass sweeping 18 kHz → 250 Hz), echo (0.3 s, 55% feedback,
  the dry signal fades in the first half), slowing down (reads the tail at a rate sliding to 0.5),
  custom (speed, pitch, echo, muffle), automatic (a loud ending gets the muffled exit).
- **Beat matching** (`beat.rs`, `mixer::align`): onset envelope (5 ms hop) → autocorrelation (70–180
  BPM, weighted towards 120) → a comb search over period (±2%) and phase on the smoothed envelope.
  The new track's first seconds are decoded separately (`decode_head`, same trim as playback).
  If the tempos are within 8% (after allowing for double/half time) the tail is stretched with WSOLA
  to the new tempo, and the new track comes in on a beat (the first few beats of the old track
  play alone). Tests: tempo within 0.5%, beats on the grid within 10 ms, and after the stretch
  both tracks' beats within 20 ms (120→126, 100→96, 128→128, 90→178 double time).
- **Never mixes:** same-album neighbours played in order (`QueueItem.album` + `album_run`),
  Bit-perfect, tracks of unknown length. Skip silence is off while AutoMix is on. Settings → Audio
  says so.
- **Not done:** the in-player badge (M3 player UI; for now the Audio card lists "AutoMix (effect,
  beat match 120→126 BPM)"), bar/downbeat alignment, smooth tempo ramps (the stretch is constant),
  and previewing a transition. A user skip cuts the held tail (a manual skip doesn't mix).
- **Lesson:** a hop of `(rate × 5 ms) as usize` samples isn't 5 ms; at 22.05 kHz it was 0.23%
  short, which looked like a tempo bias until the envelope peaks were printed.

## 2026-11-26 (session 5, later): skip silence (M2-14), emulator crash fix

- **M2-14** `loss_audio::silence::Skipper`: a frame is silent when every channel is under −55 dBFS;
  a run of 0.8 s or more is cut to its first 0.25 s, shorter runs and a short tail play as they
  were. Streaming, exact, chunk-size independent; the join is quiet-to-quiet so no fade. Runs
  before the stretch in `run_graph`; the position adds the skipped time (`SkipLog`), so the
  seek bar stays right. Setting `skip_silence` (Settings → Audio). Allowed on bit-perfect (it only
  removes whole silent frames). The "off while AutoMix is on" rule comes with M2-15.
- **Fix: app died on the x86_64 emulator** (2 of 3 CI runs). Logcat: the activity was destroyed
  and recreated about 2 s after start (emulator first-boot keyboard/locale change), then Rust
  threads hit `pthread_mutex_lock called on a destroyed mutex` while the native side tore down.
  `configChanges` only listed orientation/screen/keyboardHidden; now it lists every change, so
  Android never recreates the activity for a locale, dark-mode, font-scale or density change
  (which would have hit real phones too).

## 2026-11-26 (session 5, later): speed and pitch (M2-13)

- **M2-13** `loss_audio::stretch::Wsola` (waveform-similarity overlap-add: ~24 ms Hann windows, ±10 ms
  search by normalised cross-correlation, exact output length, no fade-in) + `Tempo { speed,
  keep_pitch, semitones }`. `Tempo::plan` gives (resample, stretch): keep-pitch → stretch
  `speed/shift`, pitch shift by resampling `shift`; pitch-follows-speed → resample `speed·shift`,
  stretch `1/shift`. Engine: the stretch runs at the source rate before the graph; the resample is
  folded into the graph's source rate (`build_graph`, polynomial resampler since the FFT ones need
  whole-number ratios); `Session::speed` / `Display::speed` make the position count source
  seconds; a live change reloads the track at the current position; off on bit-perfect (the
  Audio card says so). Settings → Audio: Speed (%), Keep the pitch, Pitch (semitones). In-player
  menu and presets come with M3-07.
- **Checked at sample level** (lesson from the EQ bug): heard length and frequency within 0.5%
  for speeds 0.75–2× and pitch −5…+12 semitones, with and without keep-pitch; position counts
  source seconds and carries over a live speed change. WSOLA costs 3% of real time (release).
- Not done: the phase vocoder for "best" quality (WSOLA is fine for music with strong
  periodicity, softer for dense mixes); smooth (click-free) speed changes: a live change
  restarts the track from the same place.

## 2026-11-26 (session 5, later): fix, the equalizer and crossfeed weren't connected

- **Bug (M2-10 and M2-12 builds before this commit):** `Session::push_pending` never ran the
  equalizer or the crossfeed, so turning them on changed the "processing" label but not the
  sound. A scripted text replace for that function silently didn't match after rustfmt had
  reshaped it, and the engine tests only checked the label. The DSP itself (`eq.rs`,
  `crossfeed.rs`) was right and tested on its own; the engine just never called it.
- Fixed: gain → crossfeed → equalizer (limiter last) in `push_pending`. New engine tests check
  real samples: a pre-amp halves every sample exactly; a +9 dB band measures within 0.2 dB of its
  design; crossfeed output equals the filter applied by hand and puts left-only bass in the right
  ear. The limiter's look-ahead used to add 5 ms of silence at the start; it's dropped now
  (`Equalizer::process_vec`), so timing is exact and a gapless join carries the last 5 ms into
  the next track (only the last 5 ms of the whole queue is cut).
- **Lesson:** after a scripted edit, check that it applied (`grep`), and test behaviour (the
  samples), not labels.

## 2026-11-26 (session 5, later): crossfeed (M2-12)

- CI on the AutoEq commit: the x86_64 emulator smoke test failed once (the app process aborted
  2.5 s after start with "FORTIFY: pthread_mutex_lock called on a destroyed mutex", right after
  Android relaunched and destroyed the activity) and passed on re-run with no change: a flake.
  If it comes back, look at shutdown order when the activity is destroyed early.
- **M2-12** `loss_audio::crossfeed`: `out_L = L + g·lp(R−L)`, `out_R = R − g·lp(R−L)` with BS2B's
  presets (Light 9.5 dB/650 Hz, Medium 6 dB/700 Hz, Strong 4.5 dB/700 Hz; g = r/(1+r)): low
  frequencies leak across at the preset's level, high ones stay; mono is bit-identical and a peak
  never grows (tests: measured levels, mono, full-scale opposite phase). Engine: before the
  equalizer, carried across gapless joins, live updates, bypassed on bit-perfect paths (test).
  Setting Audio → Crossfeed (Off/Light/Medium/Strong); used only when the output is something you
  wear (`is_personal_output`: wired, Bluetooth, USB headset) or unknown (desktop); shows in the
  processing list as "crossfeed (medium)".

## 2026-11-25 (session 5, later): AutoEq and parametric bands (M2-11)

- **M2-11** `loss_core::autoeq`: parser for AutoEq's ParametricEQ.txt / Equalizer APO lines
  (PK/LSC/HSC/LP/HP, OFF skipped, values clamped); the AutoEq index (8,850 headphones, MIT,
  `data/autoeq-index.tsv` + `data/AUTOEQ-LICENSE`, rebuilt by `scripts/update-autoeq-index.py`)
  searched offline; the chosen profile is downloaded from raw.githubusercontent.com
  (`loss_download::autoeq::fetch`, generic UA, ≤ 64 KB). Corrections are kept per output device
  (`Settings::autoeq`, keyed by the route's device name, "default" without one) and switch with
  the output (`refresh_route` sends the device's correction). Applied as parametric bands with
  the EQ; AutoEq's pre-amp is used only without automatic headroom. Hand-made parametric bands
  (type, Hz, Q, gain; up to 20) on the Equalizer screen. Tested on desktop: searching "beats
  studio pro" and picking it downloads the Rtings profile and the curve shows it (input gain
  −3.7 dB vs AutoEq's −3.8). Fixture: `crates/loss-core/fixtures/autoeq-beats-studio-pro.txt`.
- The correction applies when "Use the equalizer" is on (it's part of the EQ chain).

## 2026-11-25 (session 5, later): equalizer (M2-10)

- **M2-10** `loss_audio::eq`: RBJ biquads (f64, transposed DF-II) for graphic bands (5/10/15/31
  ISO centres, constant-Q from the spacing), parametric bands (peak, shelves, HP/LP), bass
  (150 Hz) and treble (6 kHz) shelves, balance, input gain with automatic headroom from the
  computed curve's peak, linked peak compressor, 5 ms look-ahead limiter at −0.5 dBFS. Tests:
  measured sine responses match the designed curve within 0.1 dB; band peaks; limiter ceiling;
  compressor reduction; flat = bit-identical. Engine: after ReplayGain, carried across gapless
  joins (filter memory kept), live updates without clicks, bypassed for bit-perfect (test).
  `loss_core::equalizer`: settings + presets (JSON with Mornye's field names, 64 KB, clamped),
  band-count changes interpolate the curve, 12 built-in presets with Loss' own curves.
  Settings → Audio → Equalizer: on/off, curve (SVG) with input gain, presets (built-in + saved,
  save/import by paste/share), bands (upright sliders in slots: WebKit's vertical range inputs
  don't move the thumb), tone and stereo, dynamics, reset. Screenshots checked (10 and 31 bands).
- Known: the limiter's 5 ms look-ahead isn't flushed at the very end of a queue (the last 5 ms
  of the last song); positions lag by the same 5 ms while it's on.
- Parametric bands have no editor yet (moved to M2-11 with AutoEq). The EQ isn't reachable from
  Now Playing yet (player actions sheet, M3).

## 2026-11-25 (session 5, later): ReplayGain (M2-09)

- **M2-09** Tags: REPLAYGAIN_TRACK/ALBUM_GAIN and R128 (Q7.8, −23 LUFS → +5 dB) read by
  loss-core; `Track::gain_track/gain_album` and `meta_version` (older tracks are re-read once on
  the next scan). `album_runs` marks queue items that play an album in order. Engine:
  `Normalize { enabled, mode: Auto|Track|Album, preamp_db }` in `AudioOptions`; Mornye's rule
  (gain + pre-amp capped at 0 dB: only turns down, never clips); applied per session after the
  graph, so it switches on the exact sample at gapless joins (test), updates live on a settings
  change (test), and is off on bit-perfect paths (test; the Audio card says why). Shown in
  Settings → Audio as "Loss changes the sound: gain −6.0 dB (track)" until the audio-info sheet
  (M2-18). Settings: Volume levelling (off, like Mornye), Levelling by (Automatic), Pre-amp.
- Peak-based clip prevention isn't needed with "only turns down"; revisit if boosting is added.

## 2026-11-25 (session 5, later): gapless MP4 and ALAC depth (M2-08, started)

- **M2-08** (not ticked) `loss-audio/src/mp4gap.rs` reads encoder priming and length from
  `iTunSMPB` or the first sound track's edit list (boxes only, seeking past media data). Sessions
  cut each decoded packet by its timestamp (`Trim::keep`), seeks skip the priming, and the shown
  duration is the music's own. ffmpeg AAC sets now join like FLAC (0.08 ms, total exactly 3.000 s);
  `generated_gapless_sets` requires it. ALAC reports its bit depth (from the codec config).
- Checked with fixtures: FLAC 24/384 kHz, 5.1 FLAC folded to stereo, 32-bit float WAV play.
  **Limit:** Symphonia 0.5 rejects 32-bit FLAC (sample-size code 7 is "reserved" there, and its
  i32 side-channel maths can't hold 33 bits) and rates above 655,350 Hz; such files fail with an
  error (tested). Revisit when moving to Symphonia 0.6.
- **DSD**: `loss-audio/src/dsd.rs` reads DSF and DFF (DST-compressed DFF fails with an error)
  as Symphonia format/codec (registered in `opus::probe()`/`codecs()`), and decodes with a
  linear-phase Kaiser FIR (flat to 20 kHz, ≥ 100 dB at the new Nyquist) run from byte lookup
  tables, decimating to 88.2 kHz (96 kHz for 48k-family DSD). Unity gain (SACD 0 dB = −6 dBFS).
  0.5 s of DSD64 stereo decodes in 12 ms (release, desktop). Labels show "DSD64"; counts as
  lossless. Library: `loss-core/src/dsd.rs` reads rate/channels/length from the header and tags
  from the ID3v2 block (wrapped in an in-memory WAV for lofty); scanner takes .dsf/.dff.
  DoP output waits for Direct USB (M2-05).
- **WavPack**: decided on `wavicle` (pure Rust, MIT/Apache, a port of the BSD reference; the
  GPL `symphonia-codec-wavpack` is avoided while D1 is open). Checked bit-exact against ffmpeg
  for 16-bit, 32-bit int and float. `wavpack.rs`: a reader indexing block headers (seek by
  block) and a per-block decoder; mono/stereo lossless only (hybrid, DSD-in-WV, >2 ch fail with
  an error). Fixtures: 16/44.1, 24/96, float/48. Every fixture format now also passes a seek test.
- **CUE sheets**: `loss-core/src/cue.rs` parser (FILE/TRACK/TITLE/PERFORMER/INDEX 01/REM
  DATE, GENRE; BOM, CRLF, Windows paths). The scanner turns a file named by a `.cue` in its
  folder, or with an embedded CUESHEET tag (FLAC/WV/APE), into one track per CUE track
  (`Track::start/end`, id `path#n`, play stats kept on rescan). `QueueItem::start/end`; the
  engine treats a range as a trim window on top of MP4 priming, seeks into the file at start
  and at gapless handover, and shows the range's duration. Fixture tests: the album scans into
  First/Second/Third and plays back as one continuous tone (no gap, no jump).
  **Note for M4:** deleting or editing tags of a CUE track must act on the sheet, not the
  shared file.
- Left in M2-08: APE (`ape-decoder` 0.3, MIT/Apache, looks right, but there is no APE encoder
  here and its sample files are in a repository outside this session: needs a sample .ape).

## 2026-11-25 (session 5, later): output modes (M2-04)

- CI: apt now uses only the Ubuntu archive, with timed retries and the main mirror as fallback
  (three runs had stalled 25–40 min on a mirror). Green since.
- **M2-04** Output mode setting (Standard / Hi-Res / Bit-perfect / Exclusive / Direct USB) with
  availability and reasons from `loss_platform::modes` (tests with the Xiaomi's facts: no
  bit-perfect formats → falls back to Hi-Res). Engine: Standard converts to the mixer rate;
  Bit-perfect keeps rate and depth, reopens for a new depth, no dither, no software volume
  (tests). Kotlin `mixerInfo` / `setBitPerfect` (Android 14 preferred mixer attributes, verified),
  `MODIFY_AUDIO_SETTINGS` permission. The Exclusive toggle became a mode.
- Long choices render as a list with explanations (screenshot checked on desktop).
- Not verified on a phone: whether HyperOS offers bit-perfect on another DAC; the Stream line in
  Settings → Audio shows the real path.
- Next without the phone: M2-08 (formats: CUE, APE, DSD, gapless MP4), M2-09 (ReplayGain).

## 2026-11-24 (session 5, later): Direct USB groundwork (M2-05, started)

- CI on the M2-06 code is green, including both APKs and the API 34 emulator smoke run. That run
  only starts the app, so it shows the APK with the AAudio code loads and starts; no stream was
  opened. (The first M2-06 run failed on a hard-coded env-var name; fixed.)
- **M2-05** (not ticked) `loss_usb::stream`: picks the playback setting for a rate and bit depth
  (stereo PCM, exact depth, else deeper), SET_CUR rate requests (UAC1 endpoint 3 bytes, UAC2 clock
  4 bytes), volume/mute requests, `PacketSizer` (exact 44.1 kHz-family spreading: 9×44 + 45;
  high-speed microframes; follows async feedback as Q16.16) and `parse_feedback` (10.14 / 16.16
  with Linux-style format guessing). The parser now records feedback endpoints and the UAC2
  terminal's clock.
- Left for M2-05: claim/release the streaming interface (usbdevfs DISCONNECT/CLAIM/SETINTERFACE),
  isochronous OUT URBs + feedback reads on a real-time thread, a `Sink` in loss-audio that uses it
  (I16/I24/I32 packing via `sink::Encoder`), hardware volume with a safety prompt, unplug recovery.
  Needs the Beats report (M2-06) to confirm UAC version, endpoints and sync type first.

## 2026-11-24 (session 5, later): USB DAC details (M2-06, waiting for the report)

- **M2-06** new crate `loss-usb`: UAC 1/2/3 descriptor parser, DAC report (summary + hex that
  `unhex` reads back as a fixture), usbdevfs control requests for volume ranges and UAC2 clock
  rates. Kotlin glue lists USB devices, asks permission, hands Rust the fd. Settings → Audio →
  USB DAC details: formats, rates, volume range, Copy/Share report. Desktop: sysfs; debug
  `LOSS_DEBUG_PAGE=dac` + `LOSS_DAC_FIXTURE=crates/loss-usb/fixtures/uac1-headset.hex` shows it
  with a made-up headset (screenshot checked).
- Not ticked: done when the owner's Beats report is committed as a fixture
  (`crates/loss-usb/fixtures/beats-studio-pro.hex`, with a test of what it says).
- New icons: usb, copy (icon font regenerated). CI now lints and tests `loss-usb`.
- Next: M2-05 (Direct USB: claim the streaming interface, isochronous OUT) builds on `loss-usb`;
  then M2-04.

## 2026-11-24 (session 5, later): native AAudio output (M2-02)

- **M2-02** `loss-audio/src/aaudio.rs` replaces cpal on Android: chosen format (F32/I16/I24/I32),
  shared or exclusive (MMAP, falls back), power-saving or low-latency, usage media / content
  music, spatializer off by default, device id (for M2-04's picker), disconnect → reopen where
  the song was. `sink::Encoder` with TPDF dither only when samples changed (16-bit stays
  bit-exact). Four new Android-only settings under Audio; Settings → Audio shows the real stream.
  Engine tests: dither decision, format change and device loss reopen at the same position.
- Not verified on a phone yet (no device here); the DEVICE-TESTS round M2 has the checks.
- cpal is still compiled into the Android build but unused there; drop it if size matters.
- Next: M2-06 (DAC probe), M2-05 (Direct USB), M2-04 (output modes + picker).

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
