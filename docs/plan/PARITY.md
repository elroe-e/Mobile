# Parity matrix: Loss vs Mornye

This is the checklist that guarantees nothing from the comparison is missed. Every item from the
Loss vs Mornye audit (Mornye 5.4.0, commit `c4a1c7b`, 19 Nov 2026) has an ID and the plan task(s)
that deliver it. **When a task is finished, set the State of every item it covers to `done`.**

States: `todo` (planned) · `doing` · `done` · `keep` (already equal or better in Loss; protect with tests) · `excluded` (left out on purpose).

Totals: 102 only in Mornye, 42 simpler in Loss, 11 different, 21 the same, 5 only in Loss, 2 left out on purpose — 183 items.

Problems found during the audit are tracked as PRB-1…PRB-8 in [PLAN.md](PLAN.md) milestone M1.


## Platforms & app

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| PLT-01 | Android phones | Yes | Yes, Android 8 or newer, 64-bit phones | Same | — | keep |
| PLT-02 | Android TV / Google TV | TV layout with remote-control navigation and a TV banner | No | Only in Mornye | M9-06 | todo |
| PLT-03 | Tablets & big screens | Dedicated tablet layouts and a landscape player | Album grid widens; no tablet layout | Simpler in Loss | M9-07 | todo |
| PLT-04 | Android Auto | Browse and play from the car screen | No | Only in Mornye | M9-02 | todo |
| PLT-05 | iPhone & iPad | Yes, through AltStore / SideStore | No | Only in Mornye | M10-11 | todo |
| PLT-06 | Windows, macOS, Linux | Full desktop apps: song tables, right-click menus, drag and drop, window modes, mini-player window | Only a developer test window | Only in Mornye | M10-10, M2-21, M9-08 | todo |
| PLT-07 | Languages | 21 languages with an in-app language picker | English only | Only in Mornye | M0-09, M10-09 | doing |
| PLT-08 | First-run setup | Setup wizard (language, permissions, download folder) and a tutorial | Asks for music and notification permission | Simpler in Loss | M10-04 | todo |
| PLT-09 | App updates | Built-in updater with Stable and Preview channels and “What’s new” | Install each new APK yourself | Only in Mornye | M0-04, M10-05 | todo |
| PLT-10 | Crash reports & logs | Saves crash reports; log viewer with search, filter and share | None inside the app | Only in Mornye | M0-11 | done |
| PLT-11 | Back button | Goes back through screens (predictive back) | Always leaves the app, even from an album page | Different | M0-10, M1-04 | done |
| PLT-12 | Open files and links from other apps | “Open with” for audio files, share songs or links into the app, search selected text | No | Only in Mornye | M9-05 | todo |
| PLT-13 | Built with | Flutter/Dart screens, Rust backend, Kotlin and C++ for Android | Rust for screens, player and downloads, plus one small Kotlin file | Different | — | keep |
| PLT-14 | Licence | Open source, LGPL-3.0 | No licence chosen yet | Different | M10-12 | todo |

## Look & feel

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| LOOK-01 | Design style | Material 3 Expressive, drawn with native widgets | Material 3 style recreated inside a web view | Simpler in Loss | M0-10, M10-01 | doing |
| LOOK-02 | Second theme | “Mornye” glass theme: frosted glass, 7 accent colours, glass clarity and outline settings | One style | Only in Mornye | M10-02 | todo |
| LOOK-03 | Colours from album art | Player, mini player and album/artist headers | The whole app recolours to the playing song | Different | M10-01 | todo |
| LOOK-04 | Colours from wallpaper (Material You) | Yes | No | Only in Mornye | M10-01 | todo |
| LOOK-05 | Accent colour | Any colour from a picker | 6 preset colours | Simpler in Loss | M10-01 | todo |
| LOOK-06 | Light, dark, follow phone | Yes | Yes | Same | — | keep |
| LOOK-07 | Pure black (AMOLED) mode | Yes | No | Only in Mornye | M10-01 | todo |
| LOOK-08 | Font | Google Sans Flex, your phone’s font, or import any TTF/OTF | Google Sans Flex only | Simpler in Loss | M10-01 | todo |
| LOOK-09 | Interface size | Make everything smaller or larger | No setting | Only in Mornye | M10-01 | todo |
| LOOK-10 | Animation speed | Slower / faster slider | Follows the phone’s “reduce motion” only | Simpler in Loss | M10-01 | todo |
| LOOK-11 | Cover fly-in animation | Covers fly between screens, for example into the player | Player slides up; artwork fades in | Simpler in Loss | M3-13 | todo |
| LOOK-12 | Blur | Blurred navigation bar, plus an “always use blur” option | Blur behind lyrics and on the round player buttons | Simpler in Loss | M10-01 | todo |
| LOOK-13 | Icons | Material Symbols Rounded, plus an iOS-style set | Classic Material icons (45 of them) | Different | M0-10 | done |
| LOOK-14 | Shapes | Squircles, pills, scalloped “cookie” buttons | Rounded rectangles, pills, scalloped play button | Simpler in Loss | M10-01 | todo |
| LOOK-15 | Wavy seek bar | Squiggly line while playing | Thick flat bar | Only in Mornye | M3-02 | todo |
| LOOK-16 | Animated album covers | Plays video covers from metadata sources | No | Only in Mornye | M10-03 | todo |
| LOOK-17 | Vibration feedback | On swipes, selections and buttons (27 places) | None | Only in Mornye | M0-10 | done |
| LOOK-18 | Long song titles | Scroll sideways so you can read them | Cut off with “…” | Only in Mornye | M0-10, M3-11 | doing |
| LOOK-19 | Grid / list switch, pinch to resize | Yes | Albums are a grid only | Only in Mornye | M4-05 | todo |
| LOOK-20 | Fast scroller | Drag handle for long lists | No | Only in Mornye | M0-10, M4-05 | doing |
| LOOK-21 | Page transitions | Yes | Yes: tab changes, player open/close, song changes | Same | — | keep |
| LOOK-22 | Navigation bar | Pill behind the active tab | Pill behind the active tab | Same | — | keep |

## Home screen

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| HOME-01 | Greeting | Changes with the time of day, many variants, uses your name | Changes with the time of day | Simpler in Loss | M8-01 | todo |
| HOME-02 | Search on Home | Search or paste a link right on Home | No search on Home | Only in Mornye | M8-01 | todo |
| HOME-03 | Continue listening | Yes | Yes, with a scalloped play button | Same | M2-23 | todo |
| HOME-04 | Shortcut tiles | 4 tiles you choose from 9 (Shuffle, Equalizer, Stats, Network storage, Loved, Wishlist, Favourite artists, New releases, Link) | 4 fixed tiles (Shuffle, Free music, Downloads, Stats) | Simpler in Loss | M8-01 | todo |
| HOME-05 | Recently played | Yes | Yes, as “Jump back in” | Same | — | keep |
| HOME-06 | Recently added | Yes | Yes | Same | — | keep |
| HOME-07 | On repeat | Yes | Yes | Same | — | keep |
| HOME-08 | Mixes from your library | Favourites, genre, artist, decade, New additions, Rediscover and Discover mixes, refreshed daily, savable as playlists | No | Only in Mornye | M8-02 | todo |
| HOME-09 | Explore feed | Recommendations from an extension, with a country choice | No | Only in Mornye | M8-03 | todo |
| HOME-10 | Import playlist button | On Home | In Settings | Different | M8-01 | todo |

## Library

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| LIB-01 | Songs, albums, artists | Yes | Yes | Same | — | keep |
| LIB-02 | Folders view | Browse your music by folder | No | Only in Mornye | M4-05 | todo |
| LIB-03 | Genres view | Yes | No | Only in Mornye | M4-05 | todo |
| LIB-04 | Playlists | Create, rename, delete, custom cover, reorder, make one from a folder | No playlists | Only in Mornye | M4-11 | todo |
| LIB-05 | Loved songs, Wishlist, Favourite artists | Yes | No | Only in Mornye | M4-10 | todo |
| LIB-06 | Export playlists (M3U8) | Yes | No | Only in Mornye | M4-11 | todo |
| LIB-07 | Import playlists (M3U, CSV) | Saves them as playlists matched to your files | Shows which songs you own and plays them; doesn’t save a playlist | Simpler in Loss | M4-11 | todo |
| LIB-08 | Search | Songs, albums, artists and words inside lyrics | Title, artist and album | Simpler in Loss | M4-07, M5-12 | todo |
| LIB-09 | Sorting | Latest, oldest, size, album, year, genre, title, artist, length | Latest, A–Z, artist | Simpler in Loss | M4-06 | todo |
| LIB-10 | Filters | Quality (Hi-Res, CD, lossy), format, downloaded or local, missing tags | No | Only in Mornye | M4-06 | todo |
| LIB-11 | Quality badges | Yes, with a setting for how they show | Yes: Hi-Res, 24-bit, 16-bit, FLAC, MP3 | Same | M4-06 | todo |
| LIB-12 | Select many songs | Batch delete, convert, ReplayGain, fix tags, add to playlist | No | Only in Mornye | M4-08 | todo |
| LIB-13 | Swipe a song to queue it | One way plays next, the other adds to the queue | Use the ⋮ menu | Simpler in Loss | M4-09 | todo |
| LIB-14 | What tapping a song does | Setting: play it or open its details | Always plays | Only in Mornye | M4-09 | todo |
| LIB-15 | Album page | Description, disc headings, song sort (album order, most played, custom), missing-track count, find artwork | Cover, play, shuffle, queue, song list | Simpler in Loss | M4-12 | todo |
| LIB-16 | Artist page | Online info: popular songs, singles, compilations, monthly listeners, concerts, favourite button | Your albums and songs by that artist | Simpler in Loss | M4-13 | todo |
| LIB-17 | Duplicate finder | Finds copies and keeps the best one | No | Only in Mornye | M4-14 | todo |
| LIB-18 | Delete songs | Yes | No | Only in Mornye | M4-15 | todo |
| LIB-19 | Song details screen | File info, audio info, every tag, ReplayGain values, lyrics, links, copy path | No | Only in Mornye | M4-16 | todo |
| LIB-20 | Edit tags | Every field, replace or resize the cover, fill from online or MusicBrainz | No | Only in Mornye | M4-17 | todo |
| LIB-21 | Fix tags from online | Proposes changes for you to review, with undo | No | Only in Mornye | M4-18 | todo |
| LIB-22 | Find missing covers | Looks covers up online | Uses covers inside files or a folder.jpg | Only in Mornye | M4-19 | todo |
| LIB-23 | Convert format | FLAC, ALAC, MP3, AAC, Opus, with dither and resampler choices | No | Only in Mornye | M4-20 | todo |
| LIB-24 | Split CUE sheets | Yes | No | Only in Mornye | M4-21, M2-08 | todo |
| LIB-25 | Choosing music folders | Folder picker: phone storage, SD card or USB drive | Type a path or tap a suggestion | Simpler in Loss | M4-02 | todo |
| LIB-26 | Automatic scanning | On app open, daily or weekly; pause and resume | At launch and when you tap Rescan | Simpler in Loss | M4-01, M4-03 | todo |
| LIB-27 | Unreadable files report | Lists files it couldn’t read and why | No | Only in Mornye | M4-04 | todo |
| LIB-28 | Missing and moved files | Clean up missing entries, relink moved downloads | Missing files drop out on the next scan | Simpler in Loss | M4-04 | todo |
| LIB-29 | Library storage | SQLite database | One JSON file; fine for thousands of songs, slower for very large libraries | Different | M0-07, M4-01, M4-24 | doing |
| LIB-30 | Reorganise files | Moves and renames files to match your folder rules | No | Only in Mornye | M4-22 | todo |
| LIB-31 | Play in another app | Open songs in an external player | No | Only in Mornye | M4-23 | todo |

## Player screen

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| PLY-01 | Artwork layout | Default, card or full screen (setting) | Full screen, fading into the background | Simpler in Loss | M3-01 | todo |
| PLY-02 | Compact and landscape player | Compact bar, landscape layout, art beside lyrics | No | Only in Mornye | M3-01 | todo |
| PLY-03 | Quality label | Plus a full audio-info sheet: signal path, bit-perfect or not, output device, Bluetooth codec | Codec, bit depth and sample rate | Simpler in Loss | M2-18 | todo |
| PLY-04 | Play, pause, next, previous, shuffle, repeat | Yes | Yes | Same | — | keep |
| PLY-05 | Seek bar | Wavy expressive bar | Thick flat bar that holds still while you drag | Different | M3-02 | todo |
| PLY-06 | Volume slider | Phone volume; can be hidden; warns above a level you choose | Phone volume with a percentage (not tested on a phone) | Simpler in Loss | M2-17, M3-12 | todo |
| PLY-07 | Swipe to change song | On the artwork and the mini player | No | Only in Mornye | M3-03 | todo |
| PLY-08 | Love button | Yes | No | Only in Mornye | M3-04, M4-10 | todo |
| PLY-09 | Sleep timer | Set minutes or stop at the end of the song | No | Only in Mornye | M3-06 | todo |
| PLY-10 | Speed and pitch | Yes | No | Only in Mornye | M2-13, M3-07 | todo |
| PLY-11 | Output picker and casting | Choose output, Google Cast, AirPlay on iPhone | No | Only in Mornye | M3-08, M9-03 | todo |
| PLY-12 | Lyric line under the title | Yes | No | Only in Mornye | M3-09 | todo |
| PLY-13 | Up next | In the queue tab | “Up next” button | Same | — | keep |
| PLY-14 | Queue | Drag to reorder, swipe to remove, lock, no-duplicates option, clear | Tap to jump, remove | Simpler in Loss | M3-10 | todo |
| PLY-15 | Song menu in the player | Details, share, go to album or artist, open in another app | No | Only in Mornye | M3-05 | todo |
| PLY-16 | Mini player | Swipe gestures, progress, play, next | Progress, play, next | Simpler in Loss | M3-11 | todo |

## Sound & playback

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| SND-01 | File formats | Very wide through FFmpeg and Media3, including Opus, WavPack and DSD (DSF/DFF) | MP3, FLAC, AAC/M4A, ALAC, Ogg Vorbis, WAV, AIFF. Opus files show up but don’t play | Simpler in Loss | M1-03, M2-08 | doing |
| SND-02 | Gapless playback | Yes (setting) | A short gap between songs | Only in Mornye | M1-06 | done |
| SND-03 | AutoMix / crossfade | Beat-matched transitions with crossfade, muffled, echo and pitch effects | No | Only in Mornye | M2-15 | todo |
| SND-04 | Equalizer | 5 to 31 bands, bass, treble, balance, pre-amp, limiter, compressor, presets to save, import and share | No | Only in Mornye | M2-10, M2-11 | todo |
| SND-05 | Volume levelling (ReplayGain) | By track or album, with pre-amp | No | Only in Mornye | M2-09 | todo |
| SND-06 | Skip silence | Yes | No | Only in Mornye | M2-14 | todo |
| SND-07 | Spatial audio | On/off, shows head-tracking support | No | Only in Mornye | M2-16 | todo |
| SND-08 | Hi-res and bit-perfect output | USB bit-perfect (Android 14+), direct USB driver, DSD over USB, exclusive mode for music players, sample-rate limit | Opens the output at the song’s rate when the phone allows; Android may still resample | Simpler in Loss | M2-02, M2-04, M2-05, M2-06, M2-07 | todo |
| SND-09 | Choice of playback engine | Old or new (Media3) engine | One engine written in Rust | Different | M2-04 | todo |
| SND-10 | Pause for calls and other apps | Yes | No, keeps playing | Only in Mornye | M1-01 | done |
| SND-11 | Pause when headphones are unplugged | Yes | No, switches to the speaker | Only in Mornye | M1-02 | done |
| SND-12 | Resume when headphones or car connect | Off, after a disconnect, or always | No | Only in Mornye | M2-17 | todo |
| SND-13 | Pause when muted | Yes | No | Only in Mornye | M2-17 | todo |
| SND-14 | Loud-volume warning | Yes | No | Only in Mornye | M2-17, M3-12 | todo |
| SND-15 | Lock screen and notification controls | Yes | Yes (not tested on a phone) | Same | M1-08 | todo |
| SND-16 | Lyrics on the lock screen | Yes | No | Only in Mornye | M3-14 | todo |
| SND-17 | Autoplay when the queue ends | Related songs from your library; learns from your plays and skips on the phone | Stops at the end | Only in Mornye | M3-15 | todo |
| SND-18 | Music haptics | The phone vibrates to the beat | No | Only in Mornye | M2-19 | todo |
| SND-19 | Resume after a restart | Restores the whole queue | “Continue listening” restarts the last album at the last position | Simpler in Loss | M2-23 | todo |
| SND-20 | Counting plays | Plays and listening time | Plays (after 30 seconds or half the song) | Simpler in Loss | M2-24 | todo |

## Lyrics

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| LYR-01 | Synced lyrics | Auto-scroll, tap a line to jump | Auto-scroll, tap a line to jump | Same | — | keep |
| LYR-02 | Word-by-word (karaoke) | Yes | No | Only in Mornye | M5-01, M5-04 | todo |
| LYR-03 | Lyrics sources | LRCLIB, NetEase, Musixmatch, QQ Music, Apple Music, Zarz-LRC and extensions, in an order you choose | LRCLIB, .lrc files, and lyrics inside the song file | Simpler in Loss | M5-02 | todo |
| LYR-04 | Search online from the player | Yes | Yes | Same | — | keep |
| LYR-05 | Choose between results | Yes | Takes the best match | Only in Mornye | M5-03 | todo |
| LYR-06 | Translation | AI translation with your own key or a local server, two languages | No | Only in Mornye | M5-07 | todo |
| LYR-07 | Romanisation | Korean, Japanese, Chinese, Cyrillic, Hindi, Punjabi and more | No | Only in Mornye | M5-06 | todo |
| LYR-08 | Lyrics editor | Write or fix lyrics and line timings | No | Only in Mornye | M5-08 | todo |
| LYR-09 | Timing offset | Per song or for all songs | No | Only in Mornye | M5-05 | todo |
| LYR-10 | Text size and blur | Settings for both | Fixed | Only in Mornye | M5-04 | todo |
| LYR-11 | Keep screen on | While lyrics are open | No | Only in Mornye | M5-04 | todo |
| LYR-12 | Share lyrics as an image | Up to 5 lines on a card | No | Only in Mornye | M5-10 | todo |
| LYR-13 | Full-screen lyrics | Controls hide after 5 seconds | No | Only in Mornye | M5-04 | todo |
| LYR-14 | Where lyrics are saved | Inside the file, a .lrc file, or both | A .lrc file next to the song, or app storage | Simpler in Loss | M5-09 | todo |
| LYR-15 | Instrumental detection | Yes | No | Only in Mornye | M5-11 | todo |

## Downloads

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| DL-01 | Streaming-service downloads | From paid streaming services through community extensions | Not included | Left out on purpose | — | excluded |
| DL-02 | Extension store | Install extensions from GitHub repositories, set their order, auto-update | No | Only in Mornye | M6-12, M6-13, M6-14, M6-15 | todo |
| DL-03 | Download queue | Yes | Yes | Same | — | keep |
| DL-04 | Several at once | Setting | 1 to 5 at once | Same | — | keep |
| DL-05 | Pause, resume, retry | Yes | Yes; resumes where it stopped and retries on its own | Same | — | keep |
| DL-06 | Survives app restart | Yes | Yes | Same | — | keep |
| DL-07 | Progress notifications | Yes, plus a download widget | Only inside the app | Only in Mornye | M1-05, M6-11 | doing |
| DL-08 | Keeps going in the background | Background download service | May stop when the app is in the background | Only in Mornye | M1-05 | done |
| DL-09 | Wi-Fi only | Yes | No | Only in Mornye | M6-02 | todo |
| DL-10 | Pick a quality | Quality picker for each download | Takes the file the source offers | Different | M6-03 | todo |
| DL-11 | File and folder names | Templates like {artist}/{album}, separate singles folder, several album layouts | Fixed: Artist / Album / 01 Title | Simpler in Loss | M6-04 | todo |
| DL-12 | Tags, cover and lyrics added | Plus ReplayGain, artist-tag modes, separators, cover size | Title, artist, album, cover, lyrics | Simpler in Loss | M6-05 | todo |
| DL-13 | Convert after download | MP3, AAC, Opus, ALAC or capped FLAC | No | Only in Mornye | M6-06, M4-20 | todo |
| DL-14 | Skip songs you already have | Checks history | Skips duplicates still in the queue only | Simpler in Loss | M6-07 | todo |
| DL-15 | Download history | Kept history | Finished list until you clear it | Simpler in Loss | M6-01 | todo |
| DL-16 | Unpack zip albums | No | Yes, for purchases like Bandcamp | Only in Loss | — | keep |
| DL-17 | Save straight to a NAS | Upload to SMB or WebDAV | No | Only in Mornye | M6-08, M7-04 | todo |
| DL-18 | Download folder | Folder picker, any folder or SD card | Typed path; default is the app’s own folder | Simpler in Loss | M6-09 | todo |
| DL-19 | Whole albums | Albums and full discographies | “Download all” for an album; newest 25 podcast episodes | Simpler in Loss | M6-10 | todo |
| DL-20 | Fake hi-res check | Checks downloads and re-downloads fakes | No | Only in Mornye | M6-06, M8-08 | todo |

## Finding music

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| DISC-01 | Internet Archive | No | Search with licence badges and a Creative Commons-only filter | Only in Loss | — | keep |
| DISC-02 | Jamendo | No | Search independent artists’ Creative Commons music | Only in Loss | — | keep |
| DISC-03 | Podcasts | No | Search the podcast directory, download episodes from the show’s own feed | Only in Loss | M6-10 | todo |
| DISC-04 | Purchase download links | No | Paste links from Bandcamp, Qobuz or 7digital | Only in Loss | — | keep |
| DISC-05 | Streaming catalogue search | Through extensions | Not included | Left out on purpose | — | excluded |
| DISC-06 | New releases | From your favourite artists | No | Only in Mornye | M8-04 | todo |
| DISC-07 | Concerts | Upcoming shows, tickets, set lists, add to calendar | No | Only in Mornye | M8-05 | todo |
| DISC-08 | Identify a song | Listens and identifies it with Shazam | No | Only in Mornye | M8-06 | todo |
| DISC-09 | Open in other services | Song.link to open the song elsewhere | No | Only in Mornye | M8-07 | todo |

## Servers & network

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| NET-01 | Subsonic / Navidrome | Browse, search, stream, sync to your library, keep offline copies, 3 sign-in methods | Search and download only, salted-token sign-in | Simpler in Loss | M7-03, M7-05 | todo |
| NET-02 | SMB, WebDAV, HTTP folders | Stream from them and add them to your library | No | Only in Mornye | M7-01, M7-02, M7-04 | todo |
| NET-03 | Self-signed HTTPS servers | Trust one approved certificate | No | Only in Mornye | M7-06 | todo |
| NET-04 | Network options | Compatibility mode, local-network access switch | No | Only in Mornye | M7-07, M6-02 | todo |

## Stats & sharing

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| STAT-01 | Most played | Yes | Top 5 in Settings and “On repeat” on Home | Same | — | keep |
| STAT-02 | Listening statistics | Listening time, streaks, charts by day, top artists and albums, music taste | Song count, total length, play count | Simpler in Loss | M8-09, M2-24 | todo |
| STAT-03 | Rewind | Monthly and yearly recap story with share cards | No | Only in Mornye | M8-10 | todo |
| STAT-04 | Last.fm and ListenBrainz | Scrobbling and “now playing” | No | Only in Mornye | M8-12 | todo |
| STAT-05 | Discord status | Shows what you’re playing | No | Only in Mornye | M8-13 | todo |
| STAT-06 | Profile | Your name and photo | No | Only in Mornye | M8-11 | todo |

## Other devices & add-ons

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| DEV-01 | Mornye Link | Pair with a computer or another phone: play its library, remote control, play together in sync, send songs, manage files, act as a Subsonic server | No | Only in Mornye | M9-04 | todo |
| DEV-02 | Google Cast | Yes | No | Only in Mornye | M9-03 | todo |
| DEV-03 | Home-screen widgets | Player widget in 4 layouts, plus a download-queue widget | No | Only in Mornye | M9-01, M6-11 | todo |
| DEV-04 | Bluetooth and headset buttons | Yes | Yes (not tested on a phone) | Same | — | keep |

## Audio tools

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| TOOL-01 | Audio analysis | Spectrum, LUFS loudness, true peak, dynamic range, spectrogram | No | Only in Mornye | M8-08 | todo |
| TOOL-02 | Hi-res check | Finds fake hi-res, lossy sources and damaged files | No | Only in Mornye | M8-08 | todo |
| TOOL-03 | Null test | Compares two versions of a song | No | Only in Mornye | M8-08 | todo |
| TOOL-04 | ReplayGain scan | Add or remove tags for songs and albums | No | Only in Mornye | M8-08, M6-05 | todo |

## Settings & data

| ID | Feature | Mornye | Loss today | Status | Tasks | State |
|---|---|---|---|---|---|---|
| SET-01 | Number of settings | About 190 (160 app, plus theme, equalizer and AutoMix) | About 15 | Simpler in Loss | M0-08, M10-08 | doing |
| SET-02 | Settings search | Yes | No | Only in Mornye | M0-08, M10-08 | doing |
| SET-03 | Backup and restore | Settings, history, playlists, favourites, extensions | No | Only in Mornye | M10-06 | todo |
| SET-04 | Storage and cache page | See and clear each cache | No | Only in Mornye | M10-07 | todo |
| SET-05 | Passwords and keys | Kept in the phone’s secure storage | Server password kept in the app’s private settings file | Simpler in Loss | M1-07 | done |
| SET-06 | Plain-HTTP connections | Blocked, with a local-network option | Allowed for all servers (needed for home servers) | Different | M7-07 | todo |
| SET-07 | Keyboard shortcuts | Yes, on desktop | No | Only in Mornye | M10-10 | todo |
| SET-08 | About and community | Contributors, translators, links, donations, Telegram | One About card | Simpler in Loss | M10-08 | todo |

## Appendix A: every Mornye setting

All settings from Mornye's `lib/models/settings.dart` and `lib/models/theme_settings.dart`, with the task
that adds the equivalent to Loss. Equalizer (`equalizer_settings.dart`: enabled, 5/10/15/31-band gains, preamp,
bass, treble, balance, auto headroom, limiter, compressor threshold/ratio/attack/release, presets) → **M2-10**.
AutoMix options (duration, effect auto/crossfade/muffled/echo/pitch/custom, speed, pitch, echo, low-pass) → **M2-15**.
Home shortcuts (4 slots from shuffle, equalizer, listeningStats, networkStorage, loved, wishlist, favoriteArtists,
newReleases, link) → **M8-01**.

| Group | Mornye key | Mornye default | Task | Note |
|---|---|---|---|---|
| App | `defaultService` | `''` | M6-14 | Legal sources only; picks the default source and fallback order |
| App | `audioQuality` | `'LOSSLESS'` | M6-03 |  |
| App | `filenameFormat` | `'{title} - {artist}'` | M6-04 |  |
| App | `downloadDirectory` | `''` | M6-09 |  |
| App | `networkDownloadFolder` | `''` | M6-08 |  |
| App | `networkDownloadLabel` | `''` | M6-08 |  |
| App | `downloadDirectoryBookmark` | `''` | M6-09 |  |
| App | `storageMode` | `'app'` | M6-09 |  |
| App | `downloadTreeUri` | `''` | M6-09 |  |
| App | `autoFallback` | `true` | M6-14 | Legal sources only; picks the default source and fallback order |
| App | `embedMetadata` | `true` | M6-05 |  |
| App | `embeddedCoverMaxDimension` | `0` | M6-05 |  |
| App | `artistTagMode` | `artistTagModeJoined` | M6-05 |  |
| App | `metadataSeparators` | `const MetadataSeparators()` | M6-05 |  |
| App | `embedLyrics` | `true` | M5-09 |  |
| App | `embedReplayGain` | `false` | M6-05 |  |
| App | `playbackNormalization` | `false` | M2-09 |  |
| App | `playbackNormalizationPreamp` | `0` | M2-09 |  |
| App | `playbackNormalizationMode` | `'track'` | M2-09 |  |
| App | `confirmLoudVolume` | `true` | M2-17 |  |
| App | `loudVolumeLevel` | `80` | M2-17 |  |
| App | `pauseOnMute` | `true` | M2-17 |  |
| App | `headphoneResume` | `'off'` | M2-17 |  |
| App | `gaplessPlayback` | `true` | M1-06 |  |
| App | `autoMix` | `false` | M2-15 |  |
| App | `autoMixDuration` | `0` | M2-15 |  |
| App | `autoMixEffect` | `AutoMixEffect.auto` | M2-15 |  |
| App | `autoMixSpeed` | `1` | M2-15 |  |
| App | `autoMixPitch` | `0` | M2-15 |  |
| App | `autoMixEcho` | `false` | M2-15 |  |
| App | `autoMixLowPass` | `false` | M2-15 |  |
| App | `playbackSpeed` | `1` | M2-13 |  |
| App | `playbackKeepPitch` | `true` | M2-13 |  |
| App | `playbackPitch` | `0` | M2-13 |  |
| App | `discordRichPresenceEnabled` | `false` | M8-13 |  |
| App | `usbBitPerfect` | `false` | M2-04 |  |
| App | `usbDirect` | `false` | M2-05 |  |
| App | `usbDsdOverPcm` | `false` | M2-05 |  |
| App | `usbAllowFixedVolume` | `false` | M2-05 |  |
| App | `usbMaxSampleRate` | `0` | M2-05 |  |
| App | `dapExclusive` | `false` | M2-04 |  |
| App | `media3Engine` | `false` | M2-04 |  |
| App | `skipSilence` | `false` | M2-14 |  |
| App | `spatialAudio` | `true` | M2-16 |  |
| App | `queueNoDuplicates` | `false` | M3-10 |  |
| App | `queueLocked` | `false` | M3-10 |  |
| App | `exclusiveOutput` | `false` | M2-21 | Desktop exclusive output |
| App | `exclusiveOutputDevice` | `''` | M2-21 | Desktop exclusive output |
| App | `autoplay` | `false` | M3-15 |  |
| App | `autoplayPersonalization` | `true` | M3-15 |  |
| App | `autoplayRandomness` | `0` | M3-15 |  |
| App | `songAnalysis` | `false` | M3-15 |  |
| App | `songAnalysisOnlyWhileCharging` | `true` | M3-15 |  |
| App | `musicHaptics` | `false` | M2-19 |  |
| App | `musicHapticsSensitivity` | `0.5` | M2-19 |  |
| App | `musicHapticsStrength` | `0.5` | M2-19 |  |
| App | `playerCoverStyle` | `''` | M3-01 |  |
| App | `landscapeLyricsArtwork` | `false` | M3-01 |  |
| App | `autoHideLyricsControls` | `true` | M5-04 |  |
| App | `autoHideLyricsControlsWithAccessibility` | `false` | M5-04 |  |
| App | `compactPlayer` | `false` | M3-01 |  |
| App | `isFirstLaunch` | `true` | M10-04 |  |
| App | `checkForUpdates` | `true` | M10-05 |  |
| App | `updateChannel` | `'stable'` | M10-05 |  |
| App | `hasSearchedBefore` | `false` | M4-07 |  |
| App | `folderOrganization` | `'none'` | M6-04 |  |
| App | `folderPathTemplate` | `'{artist}/{album}'` | M6-04 |  |
| App | `createPlaylistFolder` | `false` | M6-04 |  |
| App | `useAlbumArtistForFolders` | `true` | M6-04 |  |
| App | `usePrimaryArtistOnly` | `false` | M6-04 |  |
| App | `filterContributingArtistsInAlbumArtist` | `false` | M6-04 |  |
| App | `historyViewMode` | `'grid'` | M6-01 |  |
| App | `historyFilterMode` | `'all'` | M6-01 |  |
| App | `historySortMode` | `'latest'` | M6-01 |  |
| App | `libraryAlbumSortMode` | `'a-z'` | M4-06 |  |
| App | `libraryArtistShowsSongs` | `false` | M4-13 |  |
| App | `defaultLibraryView` | `'last'` | M4-05 |  |
| App | `libraryQualityLabelMode` | `AppSettings.libraryQualityLabelBitDepthOnly` | M4-06 |  |
| App | `swipeToQueue` | `true` | M4-09 |  |
| App | `librarySongTap` | `AppSettings.librarySongTapDetails` | M4-09 |  |
| App | `askQualityBeforeDownload` | `true` | M6-03 |  |
| App | `enableLogging` | `false` | M0-11 |  |
| App | `useExtensionProviders` | `true` | M6-14 | Legal sources only; picks the default source and fallback order |
| App | `defaultSearchTab` | `'all'` | M4-07 |  |
| App | `searchViewMode` | `'list'` | M4-07 |  |
| App | `homeShortcuts` | `const [` | M8-01 |  |
| App | `homeContinueListening` | `true` | M8-01 |  |
| App | `separateSingles` | `false` | M6-04 |  |
| App | `singleFilenameFormat` | `'{title} - {artist}'` | M6-04 |  |
| App | `albumFolderStructure` | `'artist_album'` | M6-04 |  |
| App | `showExtensionStore` | `true` | M6-13 |  |
| App | `autoUpdateExtensions` | `true` | M6-13 |  |
| App | `heroAnimationsEnabled` | `true` | M3-13 |  |
| App | `forceBackdropBlur` | `false` | M10-01 |  |
| App | `interfaceScale` | `1` | M10-01 |  |
| App | `animationSpeed` | `1` | M10-01 |  |
| App | `extensionVerificationBrowserMode` | `'in_app_first'` | M6-12 | Sign-in pages for extensions |
| App | `locale` | `'system'` | M10-09 |  |
| App | `lyricsMode` | `'embed'` | M5-09 |  |
| App | `keepScreenOnLyrics` | `true` | M5-04 |  |
| App | `lockScreenLyrics` | `false` | M3-14 |  |
| App | `motionArtworkEnabled` | `true` | M10-03 |  |
| App | `saveMotionArtworkFile` | `false` | M10-03 |  |
| App | `saveFolderCover` | `false` | M6-05 |  |
| App | `folderCoverMaxDimension` | `0` | M6-05 |  |
| App | `removeEmptyFolders` | `true` | M4-15 |  |
| App | `listeningStatisticsEnabled` | `true` | M8-09 |  |
| App | `autoConvertDownloads` | `false` | M6-06 |  |
| App | `redownloadFakeHiRes` | `false` | M6-06 |  |
| App | `autoConvertFormat` | `'mp3'` | M6-06 |  |
| App | `autoConvertBitrate` | `'320k'` | M6-06 |  |
| App | `autoConvertMaxBitDepth` | `16` | M6-06 |  |
| App | `autoConvertMaxSampleRate` | `48000` | M6-06 |  |
| App | `autoConvertSmartDither` | `true` | M6-06 |  |
| App | `autoConvertSmartDitherHighPass` | `false` | M6-06 |  |
| App | `useAllFilesAccess` | `false` | M6-09 |  |
| App | `autoExportFailedDownloads` | `false` | M6-01 |  |
| App | `downloadNetworkMode` | `'any'` | M6-02 |  |
| App | `networkCompatibilityMode` | `false` | M7-07 |  |
| App | `allowLocalNetwork` | `false` | M7-07 |  |
| App | `songLinkRegion` | `'US'` | M8-07 |  |
| App | `nativeDownloadWorkerEnabled` | `false` | M1-05 |  |
| App | `concurrentDownloads` | `1` | have | Loss already has this (1–5) |
| App | `localLibraryEnabled` | `false` | M4-01 |  |
| App | `localLibraryPath` | `''` | M4-02 |  |
| App | `localLibraryBookmark` | `''` | M4-02 |  |
| App | `localLibraryShowDuplicates` | `true` | M4-14 |  |
| App | `localLibraryAutoScan` | `'off'` | M4-03 |  |
| App | `linkHideDisconnectedSongs` | `true` | M9-04 |  |
| App | `hasCompletedTutorial` | `false` | M10-04 |  |
| App | `lyricsProviders` | `const [` | M5-02 |  |
| App | `lyricsPreferBestTiming` | `true` | M5-02 |  |
| App | `lyricsIncludeTranslationNetease` | `false` | M5-09 |  |
| App | `lyricsIncludeRomanizationNetease` | `false` | M5-09 |  |
| App | `lyricsMultiPersonWordByWord` | `true` | M5-09 |  |
| App | `lyricsAppleElrcWordSync` | `true` | M5-09 |  |
| App | `musixmatchLanguage` | `''` | M5-09 |  |
| App | `lyricsAiProvider` | `'openai'` | M5-07 |  |
| App | `lyricsAiBaseUrl` | `''` | M5-07 |  |
| App | `lyricsAiModel` | `''` | M5-07 |  |
| App | `lyricsTranslationLanguage` | `''` | M5-07 |  |
| App | `lyricsTranslationSecondLanguage` | `''` | M5-07 |  |
| App | `lyricsAiInstruction` | `''` | M5-07 |  |
| App | `lyricsAutoTranslate` | `false` | M5-07 |  |
| App | `lyricsAiRomanize` | `true` | M5-07 |  |
| App | `lyricsRomanize` | `true` | M5-06 |  |
| App | `lyricsRomanizedMainLine` | `false` | M5-06 |  |
| App | `lyricsAiWordSync` | `false` | M5-07 |  |
| App | `lyricsTranslationSaveTo` | `lyricsTranslationSaveToSongFile` | M5-07 |  |
| App | `lyricsTranslationAutoSave` | `false` | M5-07 |  |
| App | `lyricsTranslateDownloads` | `false` | M5-07 |  |
| App | `lastSeenVersion` | `''` | M10-05 |  |
| App | `deduplicateDownloads` | `true` | M6-07 |  |
| App | `allowQualityVariants` | `false` | M6-07 |  |
| App | `saveDownloadHistory` | `true` | M6-01 |  |
| App | `playerMode` | `'internal'` | M4-23 |  |
| App | `playerShowPronunciation` | `true` | M5-06, M5-07 |  |
| App | `playerShowTranslation` | `true` | M5-06, M5-07 |  |
| App | `playerShowVolume` | `true` | M3-12 |  |
| App | `playerShowLyricSnippet` | `false` | M3-09 |  |
| Theme | `themeMode` | `ThemeMode.system` | have | Loss has Auto / Dark / Light |
| Theme | `useDynamicColor` | `true` | M10-01 |  |
| Theme | `seedColorValue` | `kDefaultSeedColor` | M10-01 |  |
| Theme | `useAmoled` | `false` | M10-01 |  |
| Theme | `style` | `AppThemeStyle.material` | M10-02 | Loss gets its own glass style, not Mornye branding |
| Theme | `mornyeAccent` | `MornyeAccent.blue` | M10-02 | Loss gets its own glass style, not Mornye branding |
| Theme | `useSystemFont` | `false` | M10-01 |  |
| Theme | `customFontId` | `''` | M10-01 |  |
| Theme | `customFontName` | `''` | M10-01 |  |
| Theme | `lyricsTextScale` | `1` | M5-04 |  |
| Theme | `lyricsBlur` | `1` | M5-04 |  |
| Theme | `lyricsOffsetMs` | `0` | M5-05 |  |
| Theme | `mornyeGlassClarity` | `kDefaultMornyeGlassClarity` | M10-02 | Loss gets its own glass style, not Mornye branding |
| Theme | `mornyeGlassOutlines` | `true` | M10-02 | Loss gets its own glass style, not Mornye branding |

174 settings mapped (160 app + 14 theme).

