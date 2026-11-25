#!/usr/bin/env bash
# Generates audio test files with ffmpeg into target/fixtures (M0-12). Nothing here is committed;
# CI regenerates them and runs `cargo test -p loss-audio -- --ignored generated`.
#
#   scripts/make-fixtures.sh [out-dir]
#
# Made: FLAC 16/44.1, 24/96, 24/192 and 24/48 (the Beats' own rate); ALAC; WavPack; 24-bit and
# 32-bit float WAV; gapless sets (one sine split into three tracks) as FLAC, MP3, AAC and Opus;
# a CUE sheet over one FLAC; a tagged FLAC with cover art; an LRC file.
# Not made, because ffmpeg can't write them: APE and DSD (DSF/DFF). M2-08 adds small committed
# fixtures for those.
set -euo pipefail

OUT="${1:-$(cd "$(dirname "$0")/.." && pwd)/target/fixtures}"
mkdir -p "$OUT"
cd "$OUT"
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }
sine() { echo "sine=frequency=$1:sample_rate=$2:duration=$3"; }

# Lossless, the common hi-res rates. 2 s of 440 Hz.
ff -f lavfi -i "$(sine 440 44100 2)" -ac 2 -sample_fmt s16 flac-16-44100.flac
ff -f lavfi -i "$(sine 440 48000 2)" -ac 2 -sample_fmt s32 -bits_per_raw_sample 24 flac-24-48000.flac
ff -f lavfi -i "$(sine 440 96000 2)" -ac 2 -sample_fmt s32 -bits_per_raw_sample 24 flac-24-96000.flac
ff -f lavfi -i "$(sine 440 192000 2)" -ac 2 -sample_fmt s32 -bits_per_raw_sample 24 flac-24-192000.flac
ff -f lavfi -i "$(sine 440 384000 2)" -ac 2 -sample_fmt s32 -bits_per_raw_sample 24 flac-24-384000.flac
ff -f lavfi -i "$(sine 440 96000 2)" -ac 2 -sample_fmt s32 -bits_per_raw_sample 32 -strict experimental \
  flac-32-96000.flac
ff -f lavfi -i "$(sine 440 48000 2)" -af "pan=5.1|c0=c0|c1=c0|c2=c0|c3=c0|c4=c0|c5=c0" -c:a flac \
  flac-51-48000.flac
ff -f lavfi -i "$(sine 440 44100 2)" -ac 2 -c:a alac alac-16-44100.m4a
ff -f lavfi -i "$(sine 440 44100 2)" -ac 2 -c:a wavpack wavpack-16-44100.wv
ff -f lavfi -i "$(sine 440 96000 2)" -ac 2 -sample_fmt s32p -bits_per_raw_sample 24 -c:a wavpack wavpack-24-96000.wv
ff -f lavfi -i "$(sine 440 48000 2)" -ac 2 -sample_fmt fltp -c:a wavpack wavpack-f32-48000.wv
ff -f lavfi -i "$(sine 440 96000 2)" -ac 2 -c:a pcm_s24le wav-24-96000.wav
ff -f lavfi -i "$(sine 440 48000 2)" -ac 2 -c:a pcm_f32le wav-f32-48000.wav

# Gapless: one 3 s sine cut into three 1 s tracks. Played back to back they must join without a
# gap or click. Lossy encoders add delay and padding; the files carry it (LAME tag, iTunSMPB,
# Opus pre-skip) for the player to remove.
mkdir -p gapless
ff -f lavfi -i "$(sine 440 44100 3)" -ac 2 -sample_fmt s16 gapless/whole.wav
for i in 0 1 2; do
  ff -i gapless/whole.wav -af "atrim=start_sample=$((i * 44100)):end_sample=$(((i + 1) * 44100)),asetpts=N/SR/TB" \
    gapless/part$i.wav
  ff -i gapless/part$i.wav gapless/flac-$i.flac
  ff -i gapless/part$i.wav -c:a libmp3lame -b:a 192k gapless/mp3-$i.mp3
  ff -i gapless/part$i.wav -c:a aac -b:a 192k gapless/aac-$i.m4a
  ff -i gapless/part$i.wav -c:a libopus -b:a 128k gapless/opus-$i.opus
done
rm -f gapless/part*.wav

# CUE sheet: three tracks in one FLAC.
ff -f lavfi -i "$(sine 330 44100 6)" -ac 2 -sample_fmt s16 cue-album.flac
cat > cue-album.cue <<'CUE'
PERFORMER "Loss Test Artist"
TITLE "Loss Test Album"
FILE "cue-album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "First"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Second"
    INDEX 01 00:02:00
  TRACK 03 AUDIO
    TITLE "Third"
    INDEX 01 00:04:00
CUE

# Tags and cover art.
ff -f lavfi -i "color=c=0x8a2be2:s=300x300:d=1" -frames:v 1 cover.png
ff -f lavfi -i "$(sine 550 44100 2)" -i cover.png -map 0:a -map 1:v -ac 2 -sample_fmt s16 \
  -c:v png -disposition:v attached_pic \
  -metadata title="Tagged Song" -metadata artist="Loss Test Artist" -metadata album="Loss Test Album" \
  -metadata album_artist="Loss Test Artist" -metadata track=3 -metadata disc=1 -metadata date=2024 \
  -metadata genre="Test" -metadata REPLAYGAIN_TRACK_GAIN="-6.50 dB" tagged-cover.flac

# Synced lyrics for tagged-cover.flac.
cat > tagged-cover.lrc <<'LRC'
[ti:Tagged Song]
[ar:Loss Test Artist]
[00:00.00]First line
[00:01.00]Second line
LRC

echo "Fixtures in $OUT:"
find . -type f | sort | sed 's|^\./|  |'
