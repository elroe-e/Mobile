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
library-show-more = Show more ({ $count })
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

