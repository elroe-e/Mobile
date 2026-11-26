# Phone checks

Things that can only be confirmed on the owner's phone. After a milestone lands, Claude lists the round
to run, and the owner reports results (or sends logs). Phone: **Xiaomi 14 Ultra** (HyperOS).
Headphones: **Beats Studio Pro** on the USB-C cable. Their own DAC takes 32 or 48 kHz only (16/24-bit), and noise
cancelling is off while wired, which is normal for these headphones.

## Before every round

1. Install the newest APK over the old one (from the chat, or later from CI/releases).
2. HyperOS: Settings → Apps → Loss → **Autostart: on**, **Battery saver: No restrictions**.
3. Sending logs:
   - until M0-11 lands, connect a computer with USB debugging on and run
     `adb logcat -d -s Loss AndroidRuntime > loss-log.txt`;
   - after M0-11, use Settings → Logs → Share.

Mark each line ✅ or ❌, with a note for anything odd.

## Round M1 (problems fixed + first full phone check)

Background playback (M0-13; this replaces the manual HyperOS steps above)
- [ ] Home shows "Music may stop when the screen is off". Tap **Fix**: Settings → Background playback
      lists Battery, Background activity, Notifications and Autostart with their state.
- [ ] Battery → **Fix** opens Android's "allow Loss to run in the background" question; after
      allowing it and coming back, the row says "Set" without reloading.
- [ ] Autostart → **Open** opens HyperOS' Autostart screen (switch Loss on there).
- [ ] With everything set, music plays for 30 minutes with the screen off, and a 20-song download
      finishes with the screen off. (This confirms M0-13.)

First, with the Beats plugged in: Settings → Logs and diagnostics → This device → **Share**, and send
the text to Claude. It lists what Android reports for the Beats (sample rates, formats, and whether
this phone offers bit-perfect playback to them), which shapes M2.

Playback and controls
- [ ] Play an album. The notification shows title, artist and cover, and its buttons work. The
      status-bar icon is a music note.
- [ ] Lock the phone: lock-screen controls work, and seeking from the lock screen works.
- [ ] Beats buttons on the cable (play/pause, next) control Loss.
- [ ] The volume slider in Loss moves the phone's volume and the % matches; the phone's volume buttons
      move Loss' slider.
- [ ] Press Back on an album page: you return to the library (the app doesn't close).
- [ ] Open an artist from an album, then press Back twice: artist → album → library.
- [ ] Back on the Settings → Logs page returns to Settings; Back on Home sends the app to the
      background while music keeps playing.

Interruptions
- [ ] Receive a phone call while playing: music pauses, then resumes after the call.
- [ ] During a call, press pause in the notification; after the call, music stays paused.
- [ ] Press the phone's volume buttons with Loss open: Loss' volume slider follows at once.
- [ ] Start a video in another app: Loss pauses.
- [ ] Unplug the USB-C cable while playing: music **pauses** and doesn't come out of the speaker.

Formats and gapless
- [ ] An `.opus` file plays and seeks.
- [ ] A live or continuous album (tracks that flow into each other) plays with no gap between tracks.

Background
- [ ] Start a 20-song download, turn the screen off for 10 minutes: the downloads finish, and a
      notification shows progress (song, songs left, speed). "Pause all" in the notification pauses
      them; "Resume" continues. Afterwards a "Downloads finished" notification remains.
- [ ] Play music with the screen off for 30 minutes: it doesn't stop.

Storage (database)
- [ ] After installing over the old version, your library, play counts and settings are all still
      there (they move into the new database on first start).
- [ ] Settings → Logs: the line "library: N songs loaded in X ms" — note X.

Security
- [ ] Settings → My server: after restarting the phone, the server still connects without retyping the
      password.
- [ ] Settings → Logs and diagnostics opens; the log shows lines; Copy and Share work.
- [ ] Settings → Look → Language shows "Phone's language" and "English"; all text is the same
      as before (nothing shows an id like `home-tagline`).
- [ ] Library → Songs: scrolling a long list is smooth; long-press a song: the phone vibrates and
      a menu slides up from the bottom (Play next, Add to queue); Back closes it.
- [ ] Swipe a song to the right: it slides, the phone vibrates and it plays next.
- [ ] Library → Albums: pinch with two fingers to make covers bigger or smaller; it's remembered.
- [ ] In a long list, a thin handle on the right edge can be dragged to jump through it.
- [ ] Icons look like before but rounder; the active tab's icon is filled.
- [ ] Settings → Audio shows Sample-rate conversion (Best… Fast) and Highest sample rate. (They
      affect the Beats once Loss knows their rates, M2-03.)
- [ ] Settings → remove a music folder: "Folder removed" with **Undo**; Undo brings it back.
- [ ] A song with a long title: in the player and the mini player the title scrolls sideways.
- [ ] Changing a setting's option (Theme, Downloads at once): a light tick vibration.
- [ ] Settings → search "dark", "lyrics" and "navidrome": each finds its setting, and changing it
      there sticks after closing and reopening Loss.

## Round M2 (hi-fi audio with the Beats)

Known from the 2026-11-21 report: the Beats take 32/48 kHz only, and this phone doesn't offer the
Android bit-perfect mixer for them.

- [ ] (M2-03) Plug in the Beats: Settings → Audio shows "Playing through … Beats Studio Pro" and
      "Takes 32, 48 kHz". Unplug: it changes to the speaker (or the Bluetooth device).
- [ ] (M2-03) With the Beats plugged in, play a 16/44.1 FLAC: the now-playing format details list
      the conversion to 48 kHz (Loss converts, not Android). Plug in during playback: the song
      keeps its position.
- [ ] (M2-02) Play FLAC, MP3 and Opus with the Beats: they sound as before. Settings → Audio shows
      "Stream: F32 · shared · power saving · 48000 Hz …".
- [ ] (M2-02) Turn on Exclusive output, then try each Output format (Automatic, 16, 24, float):
      the Stream line shows what was asked, or "shared" if the phone refused exclusive. Send the
      Stream line for each (Settings → Help → logs also has "AAudio asked …; opened …").
- [ ] (M2-02) Unplug the Beats during playback and plug them back: playback pauses, and pressing
      play works again from the same place.
- [ ] (M2-06) Settings → Audio → **USB DAC details** with the Beats plugged in → Read details →
      allow access when Android asks → Copy report → send it to Claude. This adds the raw USB
      descriptors (does a hidden 44.1 kHz setting exist?) and the volume range. Note whether
      playback was disturbed while it read.
- [ ] (M2-04) Settings → Audio → Output mode → **Bit-perfect** with the Beats plugged in: the Audio
      card says "Bit-perfect isn't available: This phone doesn't offer it for this DAC. Playing in
      Hi-Res." (recheck after HyperOS updates). Logs (Settings → Help) show the "mixer:" line.
- [ ] (M2-04) **Standard**, **Hi-Res** and **Exclusive** each play; the Stream line in Settings →
      Audio shows the path (Exclusive: "exclusive" or a fallback to "shared").
- [ ] Output mode **Direct USB**: Android asks for USB permission once; 16/48 and 24/48 files show
      "Bit-perfect: yes"; a 16/44.1 file shows "resampled to 48 kHz by Loss (Best)".
- [ ] Standard mode: a 16/44.1 file shows "resampled to 48 kHz by Loss"; Android doesn't resample
      again.
- [ ] A 24/96 file shows "resampled to 48 kHz (Best)" and sounds clean (no clicks or crackle).
- [ ] Unplug and replug during playback: playback pauses, then works again after pressing play.
- [ ] Direct USB volume: the hardware volume control changes loudness smoothly; first-use loudness
      warning appears.
- [ ] (M2-10) Settings → Audio → Equalizer: turn it on, pick Bass and Rock: the sound changes while
      playing without clicks; moving a band slider is heard within half a second; Bit-perfect
      mode shows the bypass notice. Save a preset, share it, import it back.
- [ ] (M2-11) With the Beats plugged in: Equalizer → Headphone correction → Find → "beats studio
      pro" → pick it: "Beats Studio Pro · measured by Rtings" shows and the sound changes. Unplug:
      the correction goes away (it's kept for the Beats); plug in again: it comes back.
- [ ] (M2-09) Settings → Audio → Volume levelling on: songs with ReplayGain tags from loud and
      quiet albums play at similar loudness; Settings → Audio shows "gain … dB"; in Bit-perfect
      mode the card says levelling is off.
- [ ] Battery: one hour of FLAC playback with the screen off, note the battery drop.

- [ ] (M2-08) Copy to the music folder: an album as one FLAC + .cue, a .dsf, a .wv and an ALAC/AAC
      album from iTunes. After a rescan the CUE album shows its tracks; each plays and seeks; the
      iTunes album plays without gaps between tracks; DSD shows "DSD64".

- [ ] (M2-12) Settings → Audio → Crossfeed → Medium with the Beats: a hard-panned old stereo
      recording sounds more natural; Settings → Audio lists "crossfeed (medium)". With the phone's
      speaker it does nothing. Mono recordings sound identical.

- [ ] (M2-13) Settings → Audio → Speed 150: music plays faster at the same pitch; Keep the pitch off:
      higher pitch too. Pitch +2: higher, same speed. The player's position keeps counting the
      song's own seconds. A podcast at 125 stays easy to follow.

- [ ] (M2-14) Settings → Audio → Skip silence on: a song with a long silent gap (live album, hidden
      track) jumps over most of it and keeps a short pause; the seek bar stays correct. Switch
      language, dark mode or font size while the app is open: it must not restart or stop music.

- [ ] (M2-15) Settings → Audio → AutoMix on, length 8: the end of a song mixes into the next one
      (Automatic: muffled when it ends loud). Try each effect. Two dance tracks at similar tempos:
      the new one comes in on the beat and the old one doesn't sound pitched. An album played in
      order still joins without overlap. Tapping Next cuts straight to the next song. Switching to
      Bit-perfect: no overlap, and Settings says so.

- [ ] (M2-16) With the Beats connected, Settings → Audio → Spatial audio on: the Audio card says
      whether Android can spatialize this output (with the phone's own Spatial audio switched on
      under Sound, it should say "on", with head tracking if the Beats report it); switched off
      there it says so. Output mode Exclusive: the card says spatial audio is off in this mode.
      Please send the card's wording if it looks wrong.

## Later rounds

Claude adds a section here for each milestone with phone-only behaviour (widgets, Android Auto, Cast,
Link, TV, haptics, scrobbling…).
