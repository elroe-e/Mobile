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
- [ ] Settings → search "dark", "lyrics" and "navidrome": each finds its setting, and changing it
      there sticks after closing and reopening Loss.

## Round M2 (hi-fi audio with the Beats)

Known from the 2026-11-21 report: the Beats take 32/48 kHz only, and this phone doesn't offer the
Android bit-perfect mixer for them.

- [ ] Settings → Audio output → **DAC probe** with the Beats plugged in → Copy report → send it to
      Claude. This adds the raw USB descriptors (does a hidden 44.1 kHz setting exist?).
- [ ] Output mode **Bit-perfect** is shown as unavailable, with the reason (recheck after HyperOS
      updates).
- [ ] Output mode **Direct USB**: Android asks for USB permission once; 16/48 and 24/48 files show
      "Bit-perfect: yes"; a 16/44.1 file shows "resampled to 48 kHz by Loss (Best)".
- [ ] Standard mode: a 16/44.1 file shows "resampled to 48 kHz by Loss"; Android doesn't resample
      again.
- [ ] A 24/96 file shows "resampled to 48 kHz (Best)" and sounds clean (no clicks or crackle).
- [ ] Unplug and replug during playback: playback pauses, then works again after pressing play.
- [ ] Direct USB volume: the hardware volume control changes loudness smoothly; first-use loudness
      warning appears.
- [ ] Equalizer and AutoEq "Beats Studio Pro" profile: on/off sounds different; in Bit-perfect mode the
      app explains that EQ is bypassed.
- [ ] ReplayGain album mode: loud and quiet albums play at similar loudness.
- [ ] Battery: one hour of FLAC playback with the screen off, note the battery drop.

## Later rounds

Claude adds a section here for each milestone with phone-only behaviour (widgets, Android Auto, Cast,
Link, TV, haptics, scrobbling…).
