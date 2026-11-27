//! The facts behind the audio-info sheet (M2-18): what is playing, what Loss does to it, and what it plays
//! through. `build` turns plain facts into groups of rows; the sheet shows them in the app's
//! language and "Copy report" gives the same rows in English.

use crate::services;
use loss_audio::StreamInfo;
use loss_i18n::Arg;
use loss_platform::modes::{self, Mixer, Mode, SpatialState};
use loss_platform::route::{Device, Kind};

/// One line of the sheet: the label's id (`info-…`) and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: &'static str,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: &'static str,
    pub rows: Vec<Row>,
}

/// Everything the sheet needs, as plain data.
pub struct Facts<'a> {
    pub info: &'a StreamInfo,
    /// The file's extension, lower case ("flac").
    pub extension: String,
    pub file_bytes: u64,
    /// Length of the music in seconds.
    pub duration: f64,
    pub mode: Mode,
    pub device: Option<&'a Device>,
    pub mixer: Option<&'a Mixer>,
    pub spatial_setting: bool,
}

type Tr<'a> = &'a dyn Fn(&str, &[(&str, Arg)]) -> String;

fn text(tr: Tr, id: &str) -> String {
    tr(id, &[])
}

fn container(ext: &str) -> &'static str {
    match ext {
        "flac" => "FLAC",
        "m4a" | "mp4" | "m4b" | "aac" => "MP4",
        "ogg" | "oga" | "opus" => "Ogg",
        "mp3" => "MPEG audio",
        "wav" => "WAVE",
        "aif" | "aiff" => "AIFF",
        "wv" => "WavPack",
        "dsf" => "DSF",
        "dff" => "DSDIFF",
        "mka" | "webm" => "Matroska",
        "ape" => "Monkey's Audio",
        _ => "",
    }
}

fn encoding_names(codes: &[i32]) -> String {
    let mut names: Vec<&str> = codes
        .iter()
        .filter_map(|c| match *c {
            2 => Some("16-bit"),
            21 => Some("24-bit"),
            22 => Some("32-bit"),
            4 => Some("float"),
            _ => None,
        })
        .collect();
    names.dedup();
    names.join(", ")
}

fn khz(rate: u32) -> String {
    let k = rate as f64 / 1000.0;
    let s = format!("{k:.1}");
    format!("{} kHz", s.strip_suffix(".0").unwrap_or(&s))
}

/// Bit-perfect verdict: the translated sentence.
fn bit_perfect(f: &Facts, tr: Tr) -> String {
    let info = f.info;
    if !info.processing.is_empty() {
        return tr(
            "info-verdict-no-processing",
            &[("list", Arg::Text(info.processing.join(", ")))],
        );
    }
    match f.mode {
        Mode::BitPerfect | Mode::DirectUsb => text(tr, "info-verdict-yes"),
        Mode::Standard => text(tr, "info-verdict-no-standard"),
        Mode::HiRes | Mode::Exclusive => {
            // Same rate and nothing changed by Loss: whether the phone's own mixer leaves it
            // alone is up to Android (volume, effects), so it isn't a promise.
            if info.output_rate == 0 || info.output_rate == info.sample_rate {
                text(tr, "info-verdict-maybe-mixer")
            } else {
                text(tr, "info-verdict-no-rate")
            }
        }
    }
}

pub fn build(f: &Facts, tr: Tr) -> Vec<Group> {
    let info = f.info;
    let yes_no = |b: bool| text(tr, if b { "info-yes" } else { "info-no" });
    let unknown = || text(tr, "info-unknown");
    let row = |id, value: String| Row { id, value };

    // The file.
    let mut source = vec![row("info-codec", info.codec.to_uppercase())];
    let cont = container(&f.extension);
    if !cont.is_empty() {
        source.push(row("info-container", cont.to_string()));
    }
    source.push(row(
        "info-sample-rate",
        if info.sample_rate > 0 {
            khz(info.sample_rate)
        } else {
            unknown()
        },
    ));
    source.push(row(
        "info-bit-depth",
        info.bits_per_sample
            .map(|b| format!("{b}-bit"))
            .unwrap_or_else(unknown),
    ));
    source.push(row(
        "info-channels",
        match info.channels {
            1 => text(tr, "info-channels-mono"),
            2 => text(tr, "info-channels-stereo"),
            n => tr("info-channels-n", &[("count", Arg::Number(n as f64))]),
        },
    ));
    if f.file_bytes > 0 && f.duration > 1.0 {
        let kbps = f.file_bytes as f64 * 8.0 / f.duration / 1000.0;
        source.push(row("info-bitrate", format!("{} kbps", kbps.round() as i64)));
        source.push(row(
            "info-size",
            format!("{:.1} MB", f.file_bytes as f64 / 1_000_000.0),
        ));
        // Against the same music as plain PCM.
        if let (Some(bits), true) = (info.bits_per_sample, info.sample_rate > 0) {
            let pcm = info.sample_rate as f64 * bits as f64 / 8.0
                * info.channels.max(1) as f64
                * f.duration;
            if pcm > 0.0 && !info.codec.to_lowercase().starts_with("dsd") {
                let pct = (f.file_bytes as f64 / pcm * 100.0).round();
                let key = if info.is_lossless() {
                    "info-compression-lossless"
                } else {
                    "info-compression-lossy"
                };
                source.push(row(
                    "info-compression",
                    tr(key, &[("pct", Arg::Number(pct))]),
                ));
            }
        }
    }

    // What Loss does.
    let mut path: Vec<Row> = info
        .processing
        .iter()
        .map(|p| row("info-stage", p.clone()))
        .collect();
    if path.is_empty() {
        path.push(row("info-stage", text(tr, "info-path-clean")));
    }
    path.push(row("info-bit-perfect", bit_perfect(f, tr)));

    // Where it goes.
    let mut out = Vec::new();
    match f.device {
        Some(d) => {
            out.push(row("info-device", d.name.clone()));
            if !d.rates.is_empty() {
                let rates: Vec<String> = d.rates.iter().map(|r| khz(*r)).collect();
                out.push(row("info-device-rates", rates.join(", ")));
            }
            let enc = encoding_names(&d.encodings);
            if !enc.is_empty() {
                out.push(row("info-device-formats", enc));
            }
            if d.kind() == Kind::Bluetooth {
                out.push(row("info-bluetooth", text(tr, "info-bluetooth-note")));
            }
        }
        None => out.push(row("info-device", unknown())),
    }
    if !info.output.is_empty() {
        out.push(row("info-stream", info.output.clone()));
    }
    if let Some(m) = f.mixer {
        if m.frames_per_buffer > 0 && info.output_rate > 0 {
            let ms = m.frames_per_buffer as f64 * 1000.0 / info.output_rate as f64;
            out.push(row(
                "info-buffer",
                format!("{} frames ({ms:.1} ms)", m.frames_per_buffer),
            ));
        }
        out.push(row("info-low-latency", yes_no(m.low_latency)));
        out.push(row("info-pro-audio", yes_no(m.pro_audio)));
        let state = modes::spatial_state(m.spatial.as_ref(), f.mode, f.spatial_setting);
        out.push(row("info-spatial", spatial_text(state, tr)));
    }
    vec![
        Group {
            id: "info-group-source",
            rows: source,
        },
        Group {
            id: "info-group-path",
            rows: path,
        },
        Group {
            id: "info-group-output",
            rows: out,
        },
    ]
}

/// The spatial state as a sentence.
pub fn spatial_text(state: SpatialState, tr: Tr) -> String {
    let id = match state {
        SpatialState::Unsupported => "spatial-unsupported",
        SpatialState::Off => "info-spatial-off",
        SpatialState::Blocked => "spatial-blocked",
        SpatialState::SystemOff => "spatial-system-off",
        SpatialState::NotThisOutput => "spatial-not-this-output",
        SpatialState::On {
            head_tracking: true,
        } => "spatial-on-tracking",
        SpatialState::On {
            head_tracking: false,
        } => "spatial-on",
    };
    text(tr, id)
}

/// The report people copy: English, whatever the app's language.
pub fn report(groups: &[Group]) -> String {
    let mut s = String::from("Loss audio info\n");
    for g in groups {
        s.push_str(&format!("\n{}\n", loss_i18n::tr_en_args(g.id, &[])));
        for r in &g.rows {
            s.push_str(&format!(
                "  {}: {}\n",
                loss_i18n::tr_en_args(r.id, &[]),
                r.value
            ));
        }
    }
    s
}

pub fn current_facts<R>(f: impl FnOnce(&Facts) -> R) -> Option<R> {
    let svc = services::get();
    let st = svc.player.state();
    let cur = st.current()?;
    let meta = std::fs::metadata(&cur.path).ok();
    let settings = svc.settings();
    let mixer = svc.mixer.lock().ok().and_then(|m| m.clone());
    let device = svc.output.lock().ok().and_then(|d| d.clone());
    let mode = svc.effective_mode_of(Mode::parse(&settings.output_mode));
    let facts = Facts {
        info: &st.info,
        extension: cur
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase(),
        file_bytes: meta.map(|m| m.len()).unwrap_or(0),
        duration: st.duration,
        mode,
        device: device.as_ref(),
        mixer: mixer.as_ref(),
        spatial_setting: settings.spatial_audio,
    };
    Some(f(&facts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(id: &str, a: &[(&str, Arg)]) -> String {
        loss_i18n::tr_en_args(id, a)
    }

    fn flac() -> StreamInfo {
        StreamInfo {
            codec: "flac".into(),
            sample_rate: 44_100,
            bits_per_sample: Some(16),
            channels: 2,
            output_rate: 48_000,
            processing: vec!["resample 44100→48000 Hz".into()],
            output: "I16 · 48000 Hz".into(),
        }
    }

    fn beats() -> Device {
        Device {
            id: 7,
            type_code: 22,
            name: "Beats Studio Pro".into(),
            rates: vec![32_000, 48_000],
            channels: vec![2],
            encodings: vec![2, 21, 4],
        }
    }

    fn facts<'a>(info: &'a StreamInfo, mode: Mode, dev: Option<&'a Device>) -> Facts<'a> {
        Facts {
            info,
            extension: "flac".into(),
            file_bytes: 20_000_000,
            duration: 200.0,
            mode,
            device: dev,
            mixer: None,
            spatial_setting: false,
        }
    }

    fn value(groups: &[Group], id: &str) -> Option<String> {
        groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.id == id)
            .map(|r| r.value.clone())
    }

    #[test]
    fn describes_the_file_and_the_path() {
        let info = flac();
        let dev = beats();
        let g = build(&facts(&info, Mode::HiRes, Some(&dev)), &en);
        assert_eq!(value(&g, "info-codec").as_deref(), Some("FLAC"));
        assert_eq!(value(&g, "info-container").as_deref(), Some("FLAC"));
        assert_eq!(value(&g, "info-sample-rate").as_deref(), Some("44.1 kHz"));
        assert_eq!(value(&g, "info-bit-depth").as_deref(), Some("16-bit"));
        // 20 MB over 200 s = 800 kbps; plain 16/44.1 stereo is 1411 kbps → 57%.
        assert_eq!(value(&g, "info-bitrate").as_deref(), Some("800 kbps"));
        let c = value(&g, "info-compression").unwrap();
        assert!(c.contains("57"), "{c}");
        assert_eq!(
            value(&g, "info-device").as_deref(),
            Some("Beats Studio Pro")
        );
        assert_eq!(
            value(&g, "info-device-formats").as_deref(),
            Some("16-bit, 24-bit, float")
        );
        // Loss resamples, so it is not bit-perfect, and says why.
        let v = value(&g, "info-bit-perfect").unwrap();
        assert!(v.contains("resample 44100→48000 Hz"), "{v}");
    }

    #[test]
    fn bit_perfect_verdicts() {
        let mut info = flac();
        info.processing.clear();
        let dev = beats();
        let say = |mode, info: &StreamInfo, dev| {
            let g = build(&facts(info, mode, dev), &en);
            value(&g, "info-bit-perfect").unwrap()
        };
        // The phone's bit-perfect mixer: yes.
        assert_eq!(
            say(Mode::BitPerfect, &info, Some(&dev)),
            en("info-verdict-yes", &[])
        );
        // Same rate over USB in Hi-Res: not guaranteed, Android's mixer may still convert.
        info.output_rate = 44_100;
        assert_eq!(
            say(Mode::HiRes, &info, Some(&dev)),
            en("info-verdict-maybe-mixer", &[])
        );
        // A rate change with nothing else: no.
        info.output_rate = 48_000;
        assert_eq!(
            say(Mode::HiRes, &info, Some(&dev)),
            en("info-verdict-no-rate", &[])
        );
        assert_eq!(
            say(Mode::Standard, &info, None),
            en("info-verdict-no-standard", &[])
        );
        // Untouched samples read "nothing changes".
        let g = build(&facts(&info, Mode::HiRes, None), &en);
        assert_eq!(value(&g, "info-stage"), Some(en("info-path-clean", &[])));
    }

    #[test]
    fn the_report_is_english_and_lists_every_row() {
        let info = flac();
        let dev = beats();
        let g = build(&facts(&info, Mode::HiRes, Some(&dev)), &en);
        let r = report(&g);
        assert!(r.starts_with("Loss audio info"));
        assert!(r.contains("Codec: FLAC"), "{r}");
        assert!(r.contains("Output device: Beats Studio Pro"), "{r}");
        for group in &g {
            assert!(r.contains(&en(group.id, &[])));
            for row in &group.rows {
                assert!(r.contains(&en(row.id, &[])), "{}", row.id);
            }
        }
    }

    #[test]
    fn every_id_exists() {
        let info = flac();
        let dev = beats();
        let mixer = Mixer::parse(
            r#"{"sdk":36,"low_latency":true,"pro_audio":false,"frames_per_buffer":192}"#,
        )
        .unwrap();
        let mut f = facts(&info, Mode::HiRes, Some(&dev));
        f.mixer = Some(&mixer);
        for g in build(&f, &en) {
            assert!(loss_i18n::has(g.id), "{}", g.id);
            for r in g.rows {
                assert!(loss_i18n::has(r.id), "{}", r.id);
            }
        }
    }
}
