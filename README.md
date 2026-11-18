# Loss

A music player and downloader for Android, written in Rust.

- **Player**: plays FLAC, MP3, AAC/ALAC (M4A), Ogg Vorbis, Opus and WAV in pure Rust.
  Queue, shuffle, repeat, seeking and volume. Shows the stream quality
  (e.g. `FLAC · 24-bit · 96 kHz`).
- **Library**: scans your music folders and downloads, reads tags and cover
  art, and browses by album, artist and song, with search. Play counts and
  most-played stay on your phone.
- **Synced lyrics**: from `.lrc` files, embedded tags, or LRCLIB on request.
  Lines highlight in time, and tapping a line seeks to it.
- **Downloads**: a resumable, parallel download manager. Pause and resume use
  HTTP Range requests. Failed downloads are retried with backoff, and the
  queue survives app restarts. Downloaded files are named
  `Artist/Album/NN Title.ext`, get missing tags and cover art, get synced
  lyrics, and `.zip` albums are unpacked.
- **Where you can download from**: only sources that allow it.
  - **Internet Archive**: netlabels, live concerts and public-domain
    recordings. The licence is shown on every result, and there's an option
    to show Creative Commons items only.
  - **Jamendo**: Creative Commons music from independent artists. Needs a
    free API client id.
  - **Podcasts**: search the public directory and download episodes from the
    show's own RSS feed.
  - **My server**: your own Subsonic / Navidrome / Gonic server. It uses
    salted-token auth, so the password is never sent in clear text.
  - **Direct links**: purchase download links (Bandcamp, Qobuz, 7digital…)
    or files on your NAS.
- **Playlist import**: paste an M3U playlist or a CSV export from a streaming
  service and see which songs you already own.

Loss does not download from streaming services and never removes copy
protection.

## Project layout

| Path | What it is |
|---|---|
| `crates/loss-core` | Library model, folder scanning, tags (lofty), settings, LRC parser, playlist import |
| `crates/loss-audio` | Audio engine: Symphonia decoding → cpal output (AAudio on Android), queue, resampling |
| `crates/loss-download` | Download manager (tokio + reqwest/rustls), sources, tagging, lyrics |
| `app` | The Dioxus app: UI, Android paths and permissions (JNI) |

## Building the APK

Requirements:

- Rust with the `aarch64-linux-android` target
- `cargo install dioxus-cli --version 0.7.10 --locked`
- Android SDK (platform 35, build-tools 35) and NDK 27
- JDK 17 or newer

Then run:

```sh
./scripts/build-android.sh      # → dist/Loss-arm64.apk
```

To install it, copy the APK to your phone and open it (allow "install unknown
apps"), or use `adb install dist/Loss-arm64.apk`.

## Developing on a computer

The same UI runs as a desktop window, which is handy for quick iteration:

```sh
cargo run -p loss --no-default-features --features desktop
```

On Linux this needs `libwebkit2gtk-4.1-dev`, `libxdo-dev` and `libasound2-dev`.

## Tests

```sh
cargo test -p loss-core -p loss-audio -p loss-download
cargo test -p loss-download --test manager -- --ignored   # live Internet Archive check
```

## Not done yet

- Lock-screen and notification media controls, and guaranteed background
  playback. Android requires a small Java/Kotlin media service for these.
  Playback keeps going while the app is in the background, but Android may
  stop it after a while.
- Equalizer, home-screen widgets, Android Auto, and USB DAC bit-perfect output.
- An Android folder picker. For now, folders are typed in or picked from
  suggestions.
