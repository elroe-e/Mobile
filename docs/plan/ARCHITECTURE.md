# Loss target architecture

How Loss is built once the plan in [PLAN.md](PLAN.md) is complete. Each milestone moves the code toward
this shape; when a decision here changes, update this file in the same commit.

## 1. Principles

1. **Rust for everything that is logic.** Player, DSP, USB driver, library, tags, lyrics, downloads,
   networking, analysis, stats, sync, UI state: all Rust.
2. **Platform glue only where the OS demands it.** Android entry points (activities, services,
   broadcast receivers, widgets, Android Auto browser, Keystore) are Kotlin in `android/glue/`. Glue
   classes only marshal data to and from Rust, with no decisions in Kotlin. Budget is about 1,500 lines total.
   iOS uses Rust through `objc2` bindings. The only Swift exception is iOS WidgetKit, which is optional.
3. **Pure-Rust libraries first.** C libraries are acceptable only when no pure-Rust option of
   comparable quality exists, and each one is listed in §12.
4. **Legal sources only.** No downloading from streaming services, no DRM or decryption code, and no
   extension host functions meant for that. Free and Creative Commons catalogues, podcasts, your own
   servers, and files you bought.
5. **Private by default.** No telemetry. Statistics, learning and analysis stay on the device. Secrets
   live in the OS keystore.
6. **Testable without a phone.** Every crate has unit tests. The engine runs against a fake sink.
   The UI runs as a desktop window under Xvfb. Android runs on a CI emulator. The owner's phone is the
   final check, not the first.

## 2. Workspace layout (target)

```
Cargo.toml                 workspace
crates/
  loss-core/               shared types, text utils, ids, formatting (exists)
  loss-store/              redb database: schema, migrations, typed tables, settings storage
  loss-audio/              engine: sources, scheduler (gapless/crossfade), DSP graph, sinks
  loss-usb/                USB Audio Class descriptors + isochronous streaming over usbdevfs
  loss-dsp/                resampler (rubato), EQ, dynamics, crossfeed, time-stretch, dither, meters
  loss-library/            scanner, index, search, collections, playlists, smart playlists, duplicates
  loss-tags/               tag read/write (lofty), cover ops, CUE parsing, templates
  loss-lyrics/             lyrics model, parsers (LRC/eLRC/TTML/SYLT), providers, romanization, translation
  loss-download/           download manager, legal sources, post-processing (exists, grows)
  loss-net/                Subsonic, SMB (smb crate), WebDAV, HTTP sources, cert pinning, streaming reader
  loss-meta/               MusicBrainz, Cover Art Archive, AcoustID, Song.link, Last.fm, ListenBrainz,
                           concerts, new releases
  loss-analysis/           spectrum, loudness (ebur128), DR, hi-res check, null test, tempo/beat, song features
  loss-ext/                extension runtime (wasmi), manifests, repositories, store
  loss-link/               device pairing, mDNS, TLS, sync protocol, Subsonic-compatible server
  loss-cast/               Google Cast sender (CASTV2), discovery, local media server
  loss-stats/              listening events, aggregations, Rewind
  loss-i18n/               Fluent bundles, locale negotiation
  loss-platform/           platform abstraction: Android JNI bridge, desktop, iOS
app/                       Dioxus UI: screens, components, theme, routing
android/glue/              Kotlin + res + AndroidManifest fragment (Gradle library module)
docs/plan/                 this plan
scripts/                   setup, build, fixtures, UI smoke tests, reference fetch
.github/workflows/         CI, emulator smoke test, releases
```

New crates are created when their milestone starts; until then code stays where it is.

## 3. Runtime and threads

| Thread or runtime | Owns | Rules |
|---|---|---|
| UI (Dioxus main) | rendering, signals | never blocks; reads snapshots; sends commands |
| Engine control | queue, scheduler, decoders, DSP | single owner of playback state |
| Audio callback (AAudio / USB / cpal) | pulls from lock-free ring buffer | no allocation, no locks, no syscalls |
| USB I/O thread | isochronous URB submit/reap | real-time priority where allowed |
| Tokio runtime (shared) | HTTP, SMB, WebDAV, Link, Cast, providers | one runtime for the whole app |
| Job runner | scans, analysis, conversion, tagging, ReplayGain | priorities, cancellation, "only while charging" |
| JNI event thread | Kotlin → Rust events | attached permanently; forwards into channels |

UI state moves from 250 ms polling to **push events**: the engine and services publish changes on a
`tokio::sync::broadcast` bus, and a single UI task coalesces them into signals (max 30 updates/s for
position).

## 4. Audio engine

### 4.1 Pipeline

```
Source (decoder) ─► Scheduler ─► DSP graph (f64) ─────────────────────────────► Sink
  Symphonia            gapless      ReplayGain → EQ / parametric / AutoEq →       AAudio (shared / exclusive)
  opus-decoder         preroll      crossfeed → speed/pitch → crossfade mixer →   Android bit-perfect mixer
  ape-decoder          crossfade    volume (only when no system/hw volume) →      Direct USB (loss-usb)
  DSF/DFF reader       AutoMix      limiter → resample to sink rate →             cpal (desktop shared)
  network readers      decks        dither to sink bit depth                      WASAPI excl. / ALSA hw / CoreAudio
                                                                                  Cast / Link (network)
```

* **Bit-perfect path**: when no DSP is active and the sink accepts the source rate and depth, samples
  pass as integers without float conversion. Any active stage marks the path "not bit-perfect", with
  the reason shown in the audio-info sheet.
* **Sample-accurate position**: position = frames delivered to the sink minus sink latency, so seek,
  lyrics and gapless joins line up exactly.
* **Gapless**: the scheduler opens the next track early, applies encoder delay and padding (LAME/Xing,
  iTunSMPB, Opus pre-skip), and writes it into the same ring buffer. When rates differ it reopens the
  sink in under 100 ms.
* **Formats**: MP3, AAC/M4A, ALAC, FLAC (up to 32-bit/768 kHz), Ogg Vorbis, Opus (Ogg/WebM/MKV),
  WAV/AIFF/CAF (int and float), APE, DSD (DSF/DFF → DoP or PCM), and WavPack (see §12). CUE sheets,
  external and embedded, become virtual tracks.

### 4.2 Output selection and the owner's setup

Phone: **Xiaomi 14 Ultra** (HyperOS, **Android 16**). Headphones: **Beats Studio Pro over USB-C**, noise
cancelling off while wired.

Measured on 2026-11-21 (`docs/plan/reports/2026-11-21-xiaomi-beats.txt`):

* The Beats' DAC takes **only 32 kHz and 48 kHz** (16-bit, 24-bit, float; stereo). There is no
  44.1 kHz, so CD-rate music (most FLAC) is always resampled before it reaches the headphones. The
  question is only who does it: Android's mixer, or Loss with its best filter.
* The phone **does not offer the Android bit-perfect mixer** for this output. Direct USB is the only
  way past the system mixer.
* The system mixer runs at 48 kHz (144-frame bursts), with low-latency and pro-audio support. Media
  volume has 150 steps.
* To confirm in M2-06: the raw USB descriptors (12 interfaces, including the microphone), in case
  Android hides a 44.1 kHz alt setting.

Output modes, chosen per device and remembered (M2-20):

| Mode | How | Bit-perfect |
|---|---|---|
| Standard | AAudio shared through the system mixer | no (Android mixes and resamples) |
| Hi-Res | AAudio at the source rate and 24/32-bit; Android may still resample | sometimes |
| Bit-perfect (Android 14+) | `AudioManager.setPreferredMixerAttributes(MIXER_BEHAVIOR_BIT_PERFECT)` for the USB device, then AAudio at the track's rate | yes, if the phone supports it (checked at runtime) |
| Direct USB | Loss claims the USB audio interface and streams over usbdevfs itself | yes |
| Exclusive (DAPs) | AAudio MMAP exclusive at the track's rate | yes, where supported |

Rate negotiation (the sink reports its rates; Loss picks the target):

* A source rate the DAC accepts plays at that rate (bit-perfect where the path allows).
* Otherwise the nearest accepted rate, preferring the same family (88.2/176.4 → 44.1, 96/192 → 48,
  integer ratios) and then the higher rate.
* **The Beats: everything goes to 48 kHz.** 48/96/192 kHz are exact or integer-ratio; 44.1/88.2/176.4
  kHz use the rational 160/147 ratio with the Best sinc filter (rubato), in f64, then 24-bit with TPDF
  dither. Loss does this itself in every mode, so Android's resampler never touches the music: in
  Standard mode Loss hands the mixer 48 kHz float, which it passes on without resampling.
* DSD → PCM at 44.1 kHz family (DoP needs a DAC that supports it; the Beats doesn't).
* Volume: Direct USB uses the DAC's hardware volume (UAC feature unit) when present. Otherwise the
  phone's media volume. Digital volume is only a fallback, and then with dither.

Status (M2-01): `loss-audio` has `sink.rs` (`Sink` trait: `caps()` and `open(rate)`;
`CpalSink`; `FakeSink` + `FakeClock` for tests), `choose_rate` (own rate, else an integer ratio,
else the highest; Beats → 48 kHz), `dsp.rs` (`Graph` of f64 `Stage`s, empty when nothing needs
changing: `StreamInfo::processing` lists the stages, `untouched()` is true for an empty graph) and
latency-aware positions (`Stream::heard_frames` = taken minus output latency). Gapless joins now
need only the same output rate, not the same source rate. Sample formats (I16/I24/F32) are
negotiated in M2-02 with the native AAudio sink. On Android, cpal's default output reports every
rate, so the file's rate is kept until M2-03/M2-07 use the device's real rates.

Status (M2-03): `loss_platform::route` parses the output list that `LossBridge.outputs()` builds
from `AudioManager.getDevices(OUTPUTS)` (id, type, name, rates, channel counts, encodings) plus the
devices media is routed to (`getAudioDevicesForAttributes`, API 33+). `Outputs::active()` takes the
routed device, else the best plugged-in one (USB > wired > Bluetooth > HDMI > speaker). An
app-level `AudioDeviceCallback` emits `outputs_added`/`outputs_removed`; `Services::refresh_route`
sends `Command::SetOutputCaps` (the device's rates; none → any rate) and the engine reopens the
output at the right rate where the song was if the current rate no longer fits. Settings → Audio
shows the device and its rates. Bluetooth codec is not exposed by public APIs (left for M2-06/M9).

Status (M2-02): on Android the engine plays through `aaudio.rs`, a thin wrapper over the NDK's
AAudio C API (`ndk-sys`, a system library, no bundled C code); cpal remains the desktop output.
Settings → Audio (Android only): output format (Automatic = float when shared, the device's own
format when exclusive; or F32/I16/I24 packed/I32), exclusive (MMAP; falls back to shared when
refused), low latency (else power saving; exclusive forces low latency), spatial audio (off =
`SPATIALIZATION_BEHAVIOR_NEVER`). Usage media, content type music; `OutputPrefs::device_id` can
target a device (the picker comes with M2-04). Functions newer than API 26 (usage, content type,
spatializer, hardware format) are looked up with `dlsym`. `sink::Encoder` writes the callback's
bytes: integer formats get TPDF dither (high-pass at 16 bits) only when the samples changed
(resampled, volume below 100 %) or carry more bits than the output, so 16-bit content on a 16-bit
stream stays bit-exact (tests). Latency comes from `AAudioStream_getTimestamp`. The error
callback (disconnect) sets `Stream::lost`; the engine reopens where the song was.
`StreamInfo::output` / the log line "AAudio asked …; opened …" report the real stream
(format, sharing, performance mode, buffer, and the hardware format on Android 14+).

Status (M2-06): `crates/loss-usb` (pure Rust, no dependencies but `libc` on Linux/Android)
parses device/configuration/interface/endpoint descriptors and the UAC 1/2/3 class records
(AudioControl header, terminals, feature units with mute/volume, UAC2 clock sources;
AudioStreaming AS_GENERAL and FORMAT_TYPE I with UAC1 rate lists or ranges; the data endpoint's
sync type, skipping feedback endpoints). `devfs` reads descriptors from a usbdevfs fd and sends
class GET requests through `USBDEVFS_CONTROL` (UAC1 GET_MIN/MAX/RES volume, UAC2 RANGE for volume
and clock rates); refusals (EBUSY while the kernel driver holds the interface) are reported, not
fatal. The glue only lists devices (`usbDevices`), asks permission and hands over
`UsbDeviceConnection.getFileDescriptor()` (`usbOpen`/`usbClose`); the answer arrives as the
`usb_permission` event. Settings → Audio → USB DAC details shows the formats, rates and volume
range and copies/shares the report (parsed summary + hex). On desktop Linux the same page reads
`/sys/bus/usb/devices/*/descriptors`. Fixtures: `crates/loss-usb/fixtures/*.hex`.

Status (M2-04): Settings → Audio → Output mode: Standard (Loss converts to the mixer's
`PROPERTY_OUTPUT_SAMPLE_RATE`, `OutputPrefs::fixed_rate`), Hi-Res (default; the song's rate when
the device lists it), Bit-perfect, Exclusive (MMAP; replaces the M2-02 toggle), Direct USB (shown
as not yet available). `loss_platform::modes` decides availability from the glue's `mixerInfo`
JSON (SDK, mixer rate, the USB DAC's `getSupportedMixerAttributes` with
`MIXER_BEHAVIOR_BIT_PERFECT`) and falls back to Hi-Res with a reason shown in the Audio card.
Bit-perfect: caps become the bit-perfect rates; per stream the AAudio sink calls the app's hook
(`sink::set_bit_perfect_hook`), which picks the encoding for the music's depth
(`Mixer::bit_perfect_encoding`, smallest container that holds it) and asks Android
(`setPreferredMixerAttributes`, verified with `getPreferredMixerAttributes`); the stream then
targets the DAC, ignores software volume, and reopens when the depth changes (gapless only within
one depth). Rate 0 clears the preferred mixer when leaving the mode. The Xiaomi reports no
bit-perfect formats for the Beats (expected: "This phone doesn't offer it for this DAC").
Settings with more than four choices render as a list with per-choice descriptions
(`choice-<key>-<value>-desc`).

Status (M2-08, partial): MP4 gapless via `mp4gap.rs` (iTunSMPB or edit list → `Trim`, applied
per packet timestamp; seeks offset by the priming). ALAC depth from its codec config. Format
limits of Symphonia 0.5: no 32-bit FLAC, FLAC rates ≤ 655,350 Hz (both fail cleanly; tested).
DSD: `dsd.rs` (DSF/DFF reader + FIR decimating decoder → 88.2/96 kHz PCM), registered through
`opus::probe()` and `opus::codecs()`; library facts and tags in `loss-core/src/dsd.rs`.
WavPack: `wavpack.rs` over `wavicle` (pure Rust, MIT/Apache), one block per packet.
CUE: `loss_core::cue` + `Track::from_cue`; tracks carry `start`/`end` (seconds in the file), and
the engine plays them as a `Trim` window with a seek to the start (also at gapless handover).

Status (M2-07): `resample.rs` uses `rubato` 5 (pure Rust): Fast = cubic polynomial, Medium/High/Best =
synchronous FFT with longer filters. Measured (tests): Best is flat within ±0.01 dB to 20 kHz and
keeps aliasing/imaging below −120 dB; lengths are exact (start-up delay trimmed, tail flushed at
end of file through `Graph::flush`). `Dither` (TPDF, high-pass TPDF) is ready for integer outputs
(M2-02). Settings → Audio: conversion quality and highest sample rate (`AudioOptions`, sent with
`Command::SetAudioOptions`). Gapless joins measured after the change: FLAC/MP3/Opus still clean.

### 4.3 DSP blocks (loss-dsp)

Biquad EQ (graphic 5/10/15/31 ISO bands plus parametric peaking/shelf/HP/LP), bass and treble shelves,
balance, pre-amp, auto headroom, true-peak limiter (−0.5 dBFS), compressor, BS2B crossfeed, WSOLA and
phase-vocoder time-stretch, rubato resampler presets (Fast/Medium/High/Best), TPDF and high-pass TPDF
dither, silence detector, beat/onset tracker. Every block has tests for frequency response, gain and
latency, using generated sine sweeps.

### 4.4 Direct USB (loss-usb)

1. Kotlin asks `UsbManager` for permission, then passes the opened file descriptor to Rust.
2. Rust parses descriptors (UAC 1/2/3: interfaces, alt settings, formats, bit resolutions, clock
   sources and ranges, feature units, endpoints and sync type).
3. It claims the streaming interface (detaching the kernel driver), selects the alt setting, and sets
   the sample rate.
4. It streams isochronous OUT URBs through `USBDEVFS_SUBMITURB`/`REAPURB` ioctls (`rustix`). It
   honours the asynchronous feedback endpoint and sizes packets for the 44.1 kHz family.
5. On stop or unplug it releases the interface so Android's own driver takes the DAC back.

`nusb` handles control transfers and descriptors if it can wrap an Android fd; otherwise the same ioctls
are used directly. Fallback if a phone blocks usbdevfs isochronous transfers: `libusb1-sys` (vendored C).

## 5. Data storage (loss-store, redb)

| Table | Key → value |
|---|---|
| tracks | track id → path/URI, tags, format, bit depth, rate, duration, ReplayGain, mtime, source, ISRC, fingerprint |
| albums / artists | derived ids → aggregates (rebuilt incrementally) |
| folders | folder id → URI, last scan, state (online/offline) |
| playlists, playlist_items | playlist id → name, cover, rules (smart), ordered track ids |
| collections | (kind, id) → loved / wishlist / favourite artist entries |
| play_events | timestamp → track id, ms listened, source, completed/skipped |
| downloads, download_history | job id → request, status; history entries |
| lyrics_cache, translations | track id → parsed lyrics, provider, offset, translations |
| analysis | track id → loudness, DR, spectrum summary, song features, hi-res verdict |
| network_sources | connection id → protocol, host, options (secrets in the vault) |
| settings | key → typed value (see §7) |
| kv | misc (schema version, last run times) |

The library loads into memory-light indexes at startup: 50,000 tracks in under 1 s on the Xiaomi.
Migrations are versioned and tested with fixture databases.

Status (M0-07): `crates/loss-store` has `tracks`, `settings` (one `app` row until M0-08) and `kv`, schema
version 1, values as JSON. Old `library.json`/`settings.json` are imported once and kept as
`*.json.imported`; passwords go to the vault during the import. Measured: 50,000 tracks open, load
and index in about 0.1 s on the CI-class desktop (release). Downloads still use `downloads.json`
until M6; the other tables arrive with the tasks that need them.

## 6. UI (app/)

* Dioxus 0.7 in the Android System WebView (Chromium). The decision is to stay on Dioxus. Revisit only
  if virtualized lists can't hold 60 fps on the Xiaomi; the fallback would be Dioxus Native (Blitz).
* **Router**: a stack per tab plus modal layers (sheets, dialogs, player). Android Back pops the top
  layer; at the root of Home it moves the task to the background. Predictive back on Android 14+.
* **Components**: virtualized list/grid, bottom sheet, dialog, context menu (long-press and
  right-click), snackbar with undo, swipe rows, pinch-zoom grid, fast scroller, marquee text, wavy
  slider, chips, segmented buttons, switches, steppers, cookie button, cover with fly-in (FLIP) transition.
* **Theme**: Material 3 tokens generated in Rust (`ui/theme.rs`) from the album art, wallpaper (Material
  You) or accent; light, dark and AMOLED; Material and Glass styles; interface scale; animation-speed
  multiplier; reduced motion.
* **Icons**: Material Symbols Rounded, embedded as a subsetted font.
* **Text**: every string goes through Fluent (`t!("key")`); RTL layouts for Arabic. Done in M0-09:
  `crates/loss-i18n` (concurrent `FluentBundle`, English fallback, missing id shows the id),
  `en/loss.ftl` with plural selectors, pseudo-locales `en-XA` (accented, via `fluent-pseudo`) and
  `ar-XB` (mirrored, RTL). The language follows the phone (`Locale.getDefault()` over JNI, `LANG`
  on desktop) unless set in Settings → Look → Language; the app root is keyed by language and gets
  `lang`/`dir`. Registry text uses `setting-<key>`, `section-<name>`, `choice-<key>-<value>` ids.
* **Layouts**: phone portrait, phone landscape, tablet (navigation rail, two-pane), TV (D-pad focus),
  desktop (tables, menus, window modes, mini-player window).
* **Haptics**: `HapticFeedbackConstants` and `Vibrator` through the bridge.

## 7. Settings

A single registry describes every setting: key, type, default, section, title and description keys,
search keywords, platforms, and whether it needs a restart. The registry drives the settings screens,
settings search, backup/restore and migration. All 174 Mornye settings are mapped in
[PARITY.md](PARITY.md) Appendix A.

Status (M0-08): `loss_core::registry` lists every user setting (`SETTINGS`), with `get`/`set` by key
(values checked against the kind), `search` (all words must match title, description, keywords,
section or choice labels; title matches first) and `INTERNAL` for app state such as volume and the
last track. Tests fail when a `Settings` field is in neither list. The settings screen is built
from it (generic rows for toggles, choices, steps, text, folders and colours; custom widgets for
music folders and the server). The database stores one row per setting (schema 2; the schema-1
single row is split by a migration). Titles are English until M0-09.

## 8. Android glue (android/glue)

A Gradle library module merged into the app (package `app.loss.glue`). It is bundled through Dioxus'
Android artifact mechanism: `app/src/glue.rs` holds a `manganis::ffi` block naming `android/glue`, and
`dx` 0.7.10 copies the folder into its Gradle project as `:plugins:lossglue` (done in M0-05). Only
`MainActivity` stays in `app/android/`, because it must extend dx's `WryActivity`.

The bridge (M0-06, `crates/loss-platform`):

* Kotlin → Rust: `LossBridge.emit(kind, json)` calls `LossNative.onEvent`, which Rust registers with
  `RegisterNatives` at startup (then calls `LossBridge.nativeReady`, which flushes anything queued
  before). Kinds: `media {cmd}`, `focus {change}`, `noisy`, `outputs_added/removed {types}`,
  `volume {level, max}`. `LossNative.back()` returns whether Rust handled Back.
* Rust parses them into `loss_platform::Event` and hands them to the `loss-events` thread, which
  blocks on a channel. Focus and unplug decisions are `loss_platform::FocusPolicy` (unit-tested);
  Kotlin only holds the focus request and reports.
* Rust → Kotlin: typed functions in `loss_platform::android` (`update_playback`, `stop_playback`,
  `update_downloads`) and app helpers in `app/src/platform.rs`, all through cached global class
  references loaded with the activity's class loader.
* The `loss-media` thread still ticks every 300 ms to push player state out; it becomes event-driven
  when the engine publishes state changes (M2-01).

| Class | Job |
|---|---|
| `MainActivity` | permissions, Back → Rust, keep-screen-on, window flags, intents (VIEW/SEND/PROCESS_TEXT) |
| `LossBridge` / `LossNative` | static calls from Rust; native `onEvent(kind, json)` into Rust |
| `PlaybackService` | MediaSession, media notification, audio focus, becoming-noisy, wake lock |
| `DownloadService` | dataSync foreground service and notifications for downloads, scans and conversions |
| `UsbAccess` | USB permission, attach/detach events, fd hand-off |
| `AudioRoutes` | AudioDeviceCallback, mixer attributes (bit-perfect), spatializer info |
| `Vault` | Android Keystore key wrap/unwrap |
| `PlayerWidget`, `DownloadsWidget` (+ config activity) | RemoteViews from Rust state |
| `AutoBrowserService` | Android Auto media browsing |
| `CoverProvider` | content:// artwork for Auto, widgets and notification |
| `Saf` | tree picker, persisted permissions, fd opening, MediaStore delete requests |
| `Installer` | PackageInstaller sessions for the in-app updater |

## 9. Networking

* One `reqwest` client (rustls). Per-connection certificate pinning for self-signed servers
  (custom `ServerCertVerifier`). Cleartext HTTP only for approved hosts and the local network; this
  replaces today's allow-all manifest flag.
* Polite clients: MusicBrainz at 1 request/s with a generic `Loss/<version>` user agent and no personal
  data, caching, and backoff on 429 and 503.
* Streaming reader: HTTP Range and SMB reads behind `symphonia::MediaSource`, with read-ahead and
  reconnect.

## 10. Extensions (loss-ext)

WebAssembly modules run in `wasmi` (pure Rust), with fuel and memory limits. A manifest declares id,
version, minimum app version, capabilities and allowed network domains. Capabilities: metadata, lyrics,
artwork, home feed, search, URL handler, and legal download catalogue (returns direct file URLs plus
licence). Host API: HTTP fetch (allow-listed), storage, settings and account forms, browser sign-in
callback, logging. There are **no crypto or decryption host functions**. Repositories are GitHub repos
with a signed `registry.json` (ed25519).

## 11. Security

* Vault: AES-GCM file whose key is wrapped by Android Keystore (desktop: OS keyring via `keyring`).
* Link: pairing code plus TLS with pinned self-signed certificates (`rcgen`/`rustls`); per-device
  revocation.
* Extensions are sandboxed (see §10). Zip extraction is guarded against path traversal (exists).
  Every network input is size-limited.

## 12. Third-party code that isn't pure Rust

| Piece | Why | Status |
|---|---|---|
| Android Kotlin glue | Android entry points require JVM classes | planned, kept minimal |
| SQLite | not used; redb instead | — |
| ring (rustls crypto) | assembly for speed | already used |
| libusb (fallback only) | only if usbdevfs isochronous transfers fail on a device | contingency |
| WavPack | no mature pure-Rust decoder yet | M2-08 decides: port vs `wavpack` binding |
| MP3 / AAC / Opus encoders (conversion) | no mature pure-Rust encoders | M4-20 decides: Android MediaCodec (platform) first |
| Discord Social SDK (Android) | Discord's mobile presence needs it | M8-13 |
| iOS WidgetKit (Swift) | Apple requires Swift for widgets | optional, M10-11 |

## 13. External services

| Service | Used for | Key |
|---|---|---|
| LRCLIB | lyrics | none |
| NetEase, QQ Music | lyrics (word-synced, translation) | none (unofficial; optional, user-enabled) |
| Musixmatch | lyrics | user's API key |
| MusicBrainz + Cover Art Archive | tags, covers, releases, events | none (rate limit) |
| AcoustID | fingerprint lookups | free app key (owner registers) |
| Song.link (Odesli) | open in other services | none |
| Last.fm | scrobbling, artist info | owner's API key and secret |
| ListenBrainz | scrobbling, fresh releases | user token |
| AudD or ACRCloud | identify a song from the mic | user's key |
| Bandsintown / Ticketmaster, setlist.fm | concerts, set lists | keys |
| OpenAI-compatible (incl. Ollama, LM Studio) | lyrics translation | user's key or local server |
| Internet Archive, Jamendo, iTunes podcast search | legal downloads (exist) | Jamendo client id |
| Discord | status | owner's application id |

## 14. Testing and CI

* `cargo test` for every crate. DSP and engine tests use generated signals: frequency response,
  gapless joins at sample level, resampler passband and aliasing, dither statistics.
* Desktop UI smoke test: `scripts/ui-smoke.sh` (Xvfb, PulseAudio null sink, xdotool tour, screenshots).
* GitHub Actions: fmt, clippy, tests, Android APK build (arm64 and x86_64). Emulator smoke test with
  screenshot and logcat. Tagged releases with a signed APK.
* Device checks on the owner's phone: [DEVICE-TESTS.md](DEVICE-TESTS.md).

## 15. Reference: Mornye

Pinned at commit `c4a1c7b` (5.4.0). `scripts/fetch-mornye.sh` clones it to a scratch folder, never into
this repo. Useful paths when implementing a feature:

* UI text, as the best spec of behaviour: `lib/l10n/arb/app_en.arb`
* Settings and defaults: `lib/models/settings.dart`, `theme_settings.dart`, `equalizer_settings.dart`
* Screens: `lib/screens/`. Widgets: `lib/widgets/`. Services: `lib/services/`
* Android: `android/app/src/main/` (manifest, Kotlin, widget layouts)
* Rust backend: `rust_backend/crates/` (core, network, providers, link, cast, mobile)

Mornye is LGPL-3.0. Loss re-implements behaviour; it doesn't copy Mornye's code, names, logos or theme
branding.
