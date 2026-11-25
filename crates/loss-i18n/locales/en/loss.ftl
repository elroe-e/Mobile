# Loss, English. Message ids: <area>-<what>. Arguments in { $braces }.
# Plural forms use Fluent selectors: { $count -> [one] … *[other] … }.

## Tabs and shared actions
tab-home = Home
tab-library = Library
tab-discover = Discover
tab-downloads = Downloads
tab-settings = Settings
action-play-all = Play all
action-clear = Clear
toast-copied = Copied

## Home
home-greeting-morning = Good morning
home-greeting-afternoon = Good afternoon
home-greeting-evening = Good evening
home-greeting-night = Late night
home-tagline = Your music, on your phone.
home-crash-title = Loss closed unexpectedly last time
home-crash-text = The crash report can help fix it.
home-crash-view = View
home-continue = Continue listening
home-shortcuts = Shortcuts
home-shuffle = Shuffle library
home-free-music = Free music
home-downloads-tile = { $running ->
    [0] Downloads
   *[other] Downloads · { $running }
}
home-stats-tile = { $songs ->
    [one] 1 song
   *[other] { $songs } songs
} · { $plays ->
    [one] 1 play
   *[other] { $plays } plays
}
home-empty-title = Your library is empty
home-empty-text = Add a music folder in Settings, or download free music from Discover.
home-jump-back = Jump back in
home-recent = Recently added
home-on-repeat = On repeat
home-footer = { $songs ->
    [one] 1 song
   *[other] { $songs } songs
} · { $length } of music

## Downloads
downloads-speed-saving = { $speed }/s · saving to { $folder }
downloads-saving = Saving to { $folder }
downloads-empty-title = No downloads
downloads-empty-text = Find free music, podcasts, your own server's albums, or paste a purchase link.
downloads-go-discover = Go to Discover
downloads-in-progress = In progress ({ $count })
downloads-finished = Finished ({ $count })
job-waiting = Waiting
job-retrying = Downloading (try { $attempt })
job-processing = Adding tags, cover and lyrics…
job-paused = Paused
job-done = Done · in your library
job-failed = Failed: { $error }
job-cancelled = Cancelled

## Downloads notification
notify-songs-left = { $count ->
    [one] 1 song left
   *[other] { $count } songs left
}
notify-downloading = Downloading { $title }
notify-paused = Downloads paused
notify-songs-waiting = { $count ->
    [one] 1 song waiting
   *[other] { $count } songs waiting
}
notify-done = { $done } done
notify-failed = { $failed } failed
notify-done-failed = { $done } done, { $failed } failed
notify-finished = Downloads finished

## Library
library-sort-latest = Latest
library-sort-title = A–Z
library-sort-artist = Artist
library-search = Search your library
library-songs = Songs
library-albums = Albums
library-artists = Artists
library-no-match = No songs match “{ $query }”.
library-empty-title = No music yet
library-empty-text = Loss plays the music on your phone and anything you download.
library-choose-folders = Choose music folders
library-find-free = Find free music to download
permission-allow-music = Allow access to music
permission-open-settings = Open app settings
artist-meta = { $albums ->
    [one] 1 album
   *[other] { $albums } albums
} · { $songs ->
    [one] 1 song
   *[other] { $songs } songs
}
album-meta = { $songs ->
    [one] 1 song
   *[other] { $songs } songs
} · { $length }
album-gone = This album is no longer in your library.
action-play-next = Play next
action-add-queue = Add to queue
toast-playing-next = Playing next
toast-added-queue = Added to queue
toast-album-queued = Album added to queue

## Now playing
player-now-playing = Now playing
player-lyrics = Lyrics
player-up-next = Up next
lyrics-none-found = No lyrics found for this song.
lyrics-search-failed = Couldn't search: { $error }
lyrics-source = Lyrics from LRCLIB or your files
lyrics-none-yet = No lyrics for this song yet.
lyrics-searching = Searching…
lyrics-search-online = Search online
queue-count = { $count ->
    [one] 1 song in queue
   *[other] { $count } songs in queue
}

## Discover
discover-hint-archive = Free concerts, netlabels and public-domain recordings. Each result shows its licence.
discover-hint-jamendo = Independent artists sharing music under Creative Commons. Needs a free client id (Settings).
discover-hint-podcast = Search podcasts and download episodes from the show's own feed.
discover-hint-server = Download from your own Navidrome / Subsonic server for offline listening.
discover-hint-links = Paste download links, one per line: Bandcamp or Qobuz purchase links, files on your NAS, etc. Zip albums are unpacked automatically.
discover-connect-server = Connect your server in Settings first.
discover-no-server = Server not configured
discover-nothing-found = Nothing found. Try other words.
discover-no-audio = No downloadable audio in this item.
discover-search-podcast = Podcast name or feed URL
discover-search-server = Search your server (empty = newest albums)
discover-search = Artist, album, genre…
discover-download-newest = Download newest { $count }
discover-download-all = Download all ({ $count })
discover-legal = Check each item's licence before sharing or reusing it. Loss only lists sources that let you download.
action-download = Download
action-search = Search
action-open-settings = Open Settings
toast-downloads-added = { $count ->
    [one] 1 download added
   *[other] { $count } downloads added
}
toast-added-downloads = Added to downloads

## Logs and diagnostics
logs-title = Logs
logs-filter-all = All
logs-filter-problems = Problems
logs-filter-info = Info
logs-filter-debug = Debug
logs-last-crash = Last crash
logs-crash-text = Loss closed unexpectedly. Sending this report helps fix it.
logs-crash-report = Loss crash report
logs-device = This device
logs-device-text = Phone model, Android version and every audio output with the sample rates and formats it accepts, including USB DACs.
logs-diagnostics = Loss diagnostics
logs-app-log = App log
logs-search = Search the log
logs-count = { $shown } of { $total } lines, newest first
logs-log = Loss log
logs-cleared = Log cleared
logs-empty = Nothing logged yet.
action-copy = Copy
action-share = Share
action-dismiss = Dismiss
action-refresh = Refresh
action-show = Show
action-add = Add

## Settings screen
settings-search = Search settings
settings-no-match = No settings match “{ $query }”.
settings-import-playlist = Import a playlist
settings-your-listening = Your listening
settings-about = About
settings-logs = Logs and diagnostics
settings-logs-desc = See what the app is doing, copy details for a bug report, check your audio outputs
settings-remove-folder = Remove folder
settings-scanning = Scanning…
settings-rescan = Rescan library
stats-most-played = Most played
stats-private = Statistics stay on your phone.
about-version = Loss { $version } — a music player and downloader written in Rust.
about-credits = Built with Dioxus, Symphonia, cpal, lofty and reqwest. Lyrics from LRCLIB. Loss downloads only from sources that allow it and never removes copy protection.
server-username = Username
server-password = Password
server-removed = Server removed
server-saved = Connected and saved
toast-server-connected = Server connected
server-testing = Testing…
server-test-save = Test & save
server-password-note = Your password is kept in the app's encrypted storage and only sent as a salted token.
import-hint = Paste an M3U playlist or a CSV export (with Track Name / Artist columns) to see which songs you already have.
import-match = Match with my library
import-owned = You own { $owned } of { $total } songs.
toast-playing-matched = Playing matched songs
import-play-owned = Play the { $count } songs you own
import-missing = Missing (buy them or look for them in Discover):

## Settings (ids come from loss_core::registry: setting-<key>, setting-<key>-desc)
setting-music_folders = Music folders
setting-music_folders-desc = Folders Loss scans for your music
setting-download_dir = Download folder
setting-download_dir-desc = The default needs no storage permission. Files there are removed if you uninstall Loss.
setting-max_parallel_downloads = Downloads at once
setting-max_retries = Retries for a failed download
setting-max_retries-desc = How often Loss tries again after a network error
setting-auto_tag = Tag downloads
setting-auto_tag-desc = Add title, artist, album and cover art when missing
setting-fetch_lyrics = Fetch lyrics
setting-fetch_lyrics-desc = Save synced lyrics from LRCLIB next to each download
setting-extract_zips = Unpack zip albums
setting-extract_zips-desc = Extract songs from .zip downloads (e.g. Bandcamp purchases)
setting-archive_cc_only = Internet Archive: Creative Commons only
setting-archive_cc_only-desc = Hide items without an open licence
setting-jamendo_client_id = Jamendo client id
setting-jamendo_client_id-desc = Needed to search Jamendo's free music
setting-subsonic = My server (Subsonic / Navidrome)
setting-subsonic-desc = Stream and download from your own music server
setting-theme = Theme
setting-dynamic_color = Colours from album art
setting-dynamic_color-desc = The whole app takes on the colours of what's playing
setting-accent = Accent colour
setting-accent-desc = Used when nothing is playing, or always if colours from album art are off
setting-detailed_logging = Detailed logging
setting-detailed_logging-desc = Record more detail. Turn on when reporting a problem.
section-library = Music folders
section-downloads = Downloads
section-sources = Sources
section-look = Look
section-help = Help
choice-theme-system = Auto
choice-theme-dark = Dark
choice-theme-light = Light

setting-language = Language
choice-language-system = Phone's language
choice-language-en = English
choice-language-en-XA = Pseudo (accented)
choice-language-ar-XB = Pseudo (right to left)

## Playback errors (details go to Settings → Logs)
player-error-output = Can't play: no audio output is available. Check that your headphones or DAC are connected.
player-error-missing = Can't play: the file is missing or can't be read.
player-error-unsupported = Can't play this file: its format isn't supported yet.

## Background playback (HyperOS and other phones that stop apps)
reliability-title = Background playback
settings-reliability-desc = Keep music and downloads going with the screen off
reliability-intro = Xiaomi and some other phones stop apps in the background to save battery. These settings keep Loss playing and downloading with the screen off.
reliability-battery = Battery: no restrictions
reliability-battery-desc = Lets Loss keep playing and downloading with the screen off.
reliability-background = Background activity
reliability-background-desc = Android can be set to restrict Loss in the background.
reliability-notifications = Notifications
reliability-notifications-desc = Needed for the player controls and download progress.
reliability-autostart = Autostart
reliability-autostart-desc = Lets your phone restart Loss' player if it was closed. Apps can't check this one: turn on Loss in the screen that opens.
reliability-state-ok = Set
reliability-state-off = Not set
reliability-state-unknown = Check it yourself
reliability-fix = Fix
reliability-footer = After changing a setting, come back here: this page updates by itself.
reliability-only-android = These settings exist only on Android phones.
home-reliability-title = Music may stop when the screen is off
home-reliability-text = Your phone limits Loss in the background. Fix it in a few taps.
action-open = Open
action-back = Back

action-cancel = Cancel
action-undo = Undo

library-downloaded = Downloaded
action-more = More

action-play = Play
action-shuffle = Shuffle

toast-folder-removed = Folder removed

## Component gallery (debug builds)
gallery-title = Component gallery
gallery-desc = Every building block of the interface, for checking looks and languages
gallery-buttons = Buttons and controls
gallery-switch = A switch
gallery-layers = Sheets, dialogs and messages
gallery-open-sheet = Open a sheet
gallery-open-dialog = Open a dialog
gallery-show-snackbar = Message with Undo
gallery-snackbar = Something was removed
gallery-undone = Brought back
gallery-vibrate = Vibrate
gallery-long-press = Long-press (or right-click) the row below to open its menu.
gallery-long-press-row = Long-press me
gallery-sheet-title = A bottom sheet
gallery-dialog-title = Delete this?
gallery-dialog-text = A dialog asks before something can't be undone.
gallery-dialog-confirm = Delete
gallery-confirmed = Confirmed
gallery-marquee = Long titles
gallery-marquee-text = A very long song title that doesn't fit, so it scrolls sideways to show the rest
gallery-virtual-list = 10,000 rows, only the visible ones drawn
gallery-row = Row { $number }

## Counts
count-songs = { $count ->
    [one] 1 song
   *[other] { $count } songs
}
count-albums = { $count ->
    [one] 1 album
   *[other] { $count } albums
}
count-artists = { $count ->
    [one] 1 artist
   *[other] { $count } artists
}
count-plays = { $count ->
    [one] 1 play
   *[other] { $count } plays
}

gallery-virtual-grid = A grid you can resize
gallery-pinch = Pinch with two fingers (or Ctrl + mouse wheel) to make the tiles bigger or smaller.

## Audio settings (M2-07)
section-audio = Audio
setting-resample_quality = Sample-rate conversion
setting-resample_quality-desc = When your headphones or DAC can't play a song's own rate (the Beats Studio Pro take only 48 kHz), Loss converts it. Best is the cleanest; Fast uses the least battery.
choice-resample_quality-best = Best
choice-resample_quality-high = High
choice-resample_quality-medium = Medium
choice-resample_quality-fast = Fast
setting-max_sample_rate = Highest sample rate
setting-max_sample_rate-desc = Higher-rate files are converted down to this rate. Lower rates use less battery.
choice-max_sample_rate-none = No limit
choice-max_sample_rate-48000 = 48 kHz
choice-max_sample_rate-96000 = 96 kHz
choice-max_sample_rate-192000 = 192 kHz
setting-output_mode = Output mode
setting-output_mode-desc = How music reaches your headphones or DAC.
choice-output_mode-standard = Standard
choice-output_mode-standard-desc = Loss converts every song to the phone's mixer rate, so Android doesn't convert it again.
choice-output_mode-hires = Hi-Res
choice-output_mode-hires-desc = Plays each song at its own sample rate when your headphones or DAC take it.
choice-output_mode-bitperfect = Bit-perfect
choice-output_mode-bitperfect-desc = Android sends the music to your USB DAC untouched. Needs Android 14 and a phone that offers it. Volume is set on the DAC.
choice-output_mode-exclusive = Exclusive
choice-output_mode-exclusive-desc = Asks for a stream other apps don't share, which skips some processing. Falls back to normal output when refused.
choice-output_mode-usb = Direct USB
choice-output_mode-usb-desc = Loss drives the USB DAC itself, the only bit-perfect route on phones without the Android 14 option.
mode-unavailable-old-android = Needs Android 14 or newer.
mode-unavailable-no-usb = Plug in a USB DAC or headphones first.
mode-unavailable-not-offered = This phone doesn't offer it for this DAC.
mode-unavailable-not-yet = Not available yet.
mode-unavailable-not-here = Only on Android.
output-mode-fallback = { $mode } isn't available: { $reason } Playing in Hi-Res.
setting-normalization = Volume levelling
setting-normalization-desc = Evens out loudness between songs using their ReplayGain or R128 tags, when they have them. It only turns songs down, so it never clips.
setting-normalization_mode = Levelling by
setting-normalization_mode-desc = Album keeps the loudness differences between songs of one album.
choice-normalization_mode-auto = Automatic
choice-normalization_mode-track = Song
choice-normalization_mode-album = Album
setting-normalization_preamp = Pre-amp (dB)
setting-normalization_preamp-desc = Lower values play quieter: 0 aims at −18 LUFS, −6 at −24 LUFS.
normalize-off-bit-perfect = Volume levelling is off in Bit-perfect mode: the music reaches the DAC untouched.
output-processing = Loss changes the sound: { $list }
setting-output_format = Output format
setting-output_format-desc = How samples are sent to Android. Automatic sends float, which Android's mixer uses anyway; with exclusive output it uses your DAC's own format. Integer formats are dithered when Loss changed the sound.
choice-output_format-auto = Automatic
choice-output_format-float = 32-bit float
choice-output_format-16 = 16-bit
choice-output_format-24 = 24-bit
choice-output_format-32 = 32-bit
setting-low_latency_output = Low latency
setting-low_latency_output-desc = Smaller audio buffers: controls react sooner, but playback uses more battery.
setting-spatial_audio = Spatial audio
setting-spatial_audio-desc = Lets Android add its 3D effect to music. Off keeps the sound as recorded.
output-stream = Stream: { $format }
dac-title = USB DAC details
dac-open-desc = Formats, sample rates and volume range of a plugged-in DAC or headset
dac-intro = Shows what a USB DAC or headset can play. Copy the report and send it in, so Loss can be tuned for it.
dac-none = No USB DAC or headset is connected.
dac-read = Read details
dac-reading = Reading…
dac-asking = Allow access when Android asks.
dac-failed = Couldn't read it: { $error }
dac-class-1 = USB Audio Class 1
dac-class-2 = USB Audio Class 2
dac-class-3 = USB Audio Class 3
dac-format = { $channels } channels · { $bits }-bit · { $rates }
dac-rates-from-device = rates chosen by the device
dac-volume = Hardware volume from { $min } to { $max } dB
dac-volume-unknown = Hardware volume (range not readable)
dac-volume-none = No hardware volume
dac-copy = Copy report
dac-share = Share report
dac-report-subject = USB DAC report: { $name }

output-playing-through = Playing through { $name }
output-rates = Takes { $rates }
output-any-rate = any sample rate (Android converts)
cue-untitled-track = Track { $number }
eq-title = Equalizer
eq-row-on = On
eq-row-off = Off
eq-enable = Use the equalizer
eq-bypass = Bit-perfect output sends music to the DAC untouched, so the equalizer is skipped. Choose another output mode to use it.
eq-curve = Curve
eq-input-gain = Input gain after headroom: { $db } dB
eq-presets = Presets
eq-custom = Custom
eq-save = Save preset
eq-preset-name = Preset name
eq-saved = Preset saved
eq-delete-preset = Delete preset
eq-import = Import preset
eq-import-hint = Paste a preset (JSON). Presets shared from Loss or Mornye work.
eq-import-invalid = That isn't an equalizer preset.
eq-import-too-big = That preset is too big (64 KB at most).
eq-imported-name = Imported
eq-save-short = Save
eq-import-short = Import
eq-share-short = Share
eq-share-name = Loss equalizer
eq-reset = Reset
eq-reset-title = Reset the equalizer?
eq-reset-text = A flat curve, and the default tone and dynamics. Your saved presets stay.
eq-bands = Bands
eq-tone = Tone and stereo
eq-preamp = Pre-amp
eq-bass = Bass
eq-treble = Treble
eq-balance = Balance
eq-balance-centre = Centre
eq-balance-left = { $pct } % left
eq-balance-right = { $pct } % right
eq-dynamics = Dynamics
eq-headroom = Automatic headroom
eq-headroom-desc = Lowers the input as much as the curve boosts, so boosted sounds don't clip.
eq-limiter = Peak limiter
eq-limiter-desc = Keeps peaks under −0.5 dBFS.
eq-compressor = Compressor
eq-compressor-desc = Narrows the gap between quiet and loud passages.
eq-threshold = Threshold
eq-ratio = Ratio
eq-attack = Attack
eq-release = Release
eq-value-db = { $value } dB
eq-value-ms = { $value } ms
eq-value-ratio = { $value }:1
eq-preset-flat = Flat
eq-preset-bass = Bass
eq-preset-treble = Treble
eq-preset-vocal = Vocal
eq-preset-rock = Rock
eq-preset-pop = Pop
eq-preset-jazz = Jazz
eq-preset-classical = Classical
eq-preset-electronic = Electronic
eq-preset-acoustic = Acoustic
eq-preset-loudness = Loudness
eq-preset-spoken = Spoken word
