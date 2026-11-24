//! Events from the platform glue. Kotlin sends `(kind, json)` pairs; [`Event::parse`] turns them
//! into typed values and rejects anything unknown or malformed (logged, never a panic).

use serde::Deserialize;

/// A playback or download command from the notification, lock screen, headset or Bluetooth
/// buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    /// Seek to this position, in seconds.
    Seek(f64),
    PauseAllDownloads,
    ResumeAllDownloads,
}

impl MediaCommand {
    /// Parses the command strings the glue uses: `play`, `pause`, `toggle`, `next`, `prev`,
    /// `seek <ms>`, `dl_pause_all`, `dl_resume_all`.
    pub fn parse(s: &str) -> Option<MediaCommand> {
        let s = s.trim();
        if let Some(ms) = s.strip_prefix("seek ") {
            let ms: f64 = ms.trim().parse().ok()?;
            return (ms.is_finite() && ms >= 0.0).then_some(MediaCommand::Seek(ms / 1000.0));
        }
        Some(match s {
            "play" => MediaCommand::Play,
            "pause" => MediaCommand::Pause,
            "toggle" => MediaCommand::Toggle,
            "next" => MediaCommand::Next,
            "prev" | "previous" => MediaCommand::Previous,
            "dl_pause_all" => MediaCommand::PauseAllDownloads,
            "dl_resume_all" => MediaCommand::ResumeAllDownloads,
            _ => return None,
        })
    }
}

/// Audio focus changes reported by Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusChange {
    /// Focus came back (e.g. the call ended).
    Gain,
    /// Another app took over for good.
    Loss,
    /// A call or a short sound; focus comes back later.
    LossTransient,
    /// Another app plays a short sound over ours; Android lowers our volume itself.
    Duck,
    /// We asked for focus to play and Android said no (e.g. during a call).
    Refused,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Media(MediaCommand),
    Focus(FocusChange),
    /// Android's "audio becoming noisy": the current output is about to switch to the speaker.
    Noisy,
    /// Audio outputs disappeared. Values are Android `AudioDeviceInfo.TYPE_*` codes, sinks only.
    OutputsRemoved(Vec<i32>),
    /// Audio outputs appeared (same codes).
    OutputsAdded(Vec<i32>),
    /// Android answered a USB permission request (M2-06): device path and the answer.
    UsbPermission {
        name: String,
        granted: bool,
    },
    /// The phone's media volume changed (hardware buttons, other apps, or Loss itself).
    Volume {
        level: i32,
        max: i32,
    },
}

#[derive(Deserialize)]
struct CmdJson {
    cmd: String,
}
#[derive(Deserialize)]
struct FocusJson {
    change: String,
}
#[derive(Deserialize)]
struct TypesJson {
    types: Vec<i32>,
}
#[derive(Deserialize)]
struct VolumeJson {
    level: i32,
    max: i32,
}

#[derive(Deserialize)]
struct UsbJson {
    name: String,
    granted: bool,
}

impl Event {
    pub fn parse(kind: &str, json: &str) -> Option<Event> {
        let ev = match kind {
            "media" => {
                let c: CmdJson = serde_json::from_str(json).ok()?;
                Event::Media(MediaCommand::parse(&c.cmd)?)
            }
            "focus" => {
                let f: FocusJson = serde_json::from_str(json).ok()?;
                Event::Focus(match f.change.as_str() {
                    "gain" => FocusChange::Gain,
                    "loss" => FocusChange::Loss,
                    "loss_transient" => FocusChange::LossTransient,
                    "duck" => FocusChange::Duck,
                    "refused" => FocusChange::Refused,
                    _ => return None,
                })
            }
            "noisy" => Event::Noisy,
            "outputs_removed" => {
                Event::OutputsRemoved(serde_json::from_str::<TypesJson>(json).ok()?.types)
            }
            "outputs_added" => {
                Event::OutputsAdded(serde_json::from_str::<TypesJson>(json).ok()?.types)
            }
            "usb_permission" => {
                let v: UsbJson = serde_json::from_str(json).ok()?;
                Event::UsbPermission {
                    name: v.name,
                    granted: v.granted,
                }
            }
            "volume" => {
                let v: VolumeJson = serde_json::from_str(json).ok()?;
                if v.max <= 0 || v.level < 0 {
                    return None;
                }
                Event::Volume {
                    level: v.level.min(v.max),
                    max: v.max,
                }
            }
            _ => return None,
        };
        Some(ev)
    }
}

/// Android `AudioDeviceInfo.TYPE_*` codes for outputs the user wears or plugs in. When one of
/// these disappears while playing, Loss pauses instead of carrying on through the speaker.
pub fn is_personal_output(device_type: i32) -> bool {
    matches!(
        device_type,
        3 // WIRED_HEADSET
            | 4 // WIRED_HEADPHONES
            | 5 // LINE_ANALOG
            | 6 // LINE_DIGITAL
            | 8 // BLUETOOTH_A2DP
            | 11 // USB_DEVICE
            | 12 // USB_ACCESSORY
            | 22 // USB_HEADSET
            | 23 // HEARING_AID
            | 26 // BLE_HEADSET
            | 27 // BLE_SPEAKER
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_media_commands() {
        let p = |c: &str| Event::parse("media", &format!("{{\"cmd\":\"{c}\"}}"));
        assert_eq!(p("play"), Some(Event::Media(MediaCommand::Play)));
        assert_eq!(p("prev"), Some(Event::Media(MediaCommand::Previous)));
        assert_eq!(
            p("seek 61500"),
            Some(Event::Media(MediaCommand::Seek(61.5)))
        );
        assert_eq!(
            p("dl_resume_all"),
            Some(Event::Media(MediaCommand::ResumeAllDownloads))
        );
        assert_eq!(p("seek -5"), None);
        assert_eq!(p("seek x"), None);
        assert_eq!(p("dance"), None);
    }

    #[test]
    fn parses_other_events() {
        assert_eq!(
            Event::parse("focus", r#"{"change":"loss_transient"}"#),
            Some(Event::Focus(FocusChange::LossTransient))
        );
        assert_eq!(Event::parse("noisy", "{}"), Some(Event::Noisy));
        assert_eq!(
            Event::parse("outputs_removed", r#"{"types":[22,2]}"#),
            Some(Event::OutputsRemoved(vec![22, 2]))
        );
        assert_eq!(
            Event::parse(
                "usb_permission",
                r#"{"name":"/dev/bus/usb/001/002","granted":true}"#
            ),
            Some(Event::UsbPermission {
                name: "/dev/bus/usb/001/002".into(),
                granted: true
            })
        );
        assert_eq!(
            Event::parse("volume", r#"{"level":200,"max":150}"#),
            Some(Event::Volume {
                level: 150,
                max: 150
            })
        );
        assert_eq!(Event::parse("volume", r#"{"level":3,"max":0}"#), None);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Event::parse("focus", "not json"), None);
        assert_eq!(Event::parse("focus", r#"{"change":"maybe"}"#), None);
        assert_eq!(Event::parse("teleport", "{}"), None);
    }

    #[test]
    fn personal_outputs() {
        assert!(is_personal_output(22)); // the Beats over USB-C
        assert!(is_personal_output(8));
        assert!(!is_personal_output(2)); // phone speaker
        assert!(!is_personal_output(1)); // earpiece
    }
}
