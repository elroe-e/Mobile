//! Background reliability (M0-13): what keeps Loss playing and downloading with the screen off,
//! and which of those settings need the owner's attention. Phones like the Xiaomi 14 Ultra
//! (HyperOS) stop background apps aggressively unless the app is exempt.
//!
//! The glue reports the phone's state as JSON (`LossBridge.reliability`); this decides what to
//! show. Xiaomi's Autostart switch can't be read by apps, so it is always "check yourself".

use serde::Deserialize;

/// What Android reports.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Status {
    pub manufacturer: String,
    pub sdk: i32,
    /// Exempt from battery optimisation ("No restrictions" on HyperOS).
    pub battery_unrestricted: bool,
    pub notifications: bool,
    /// The user (or the system) restricted background activity for Loss.
    pub background_restricted: bool,
    /// The phone has an Autostart settings screen Loss can open.
    pub autostart_screen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    Battery,
    Background,
    Notifications,
    Autostart,
}

impl CheckId {
    /// The glue's name for the settings screen that fixes it.
    pub fn screen(self) -> &'static str {
        match self {
            CheckId::Battery => "battery",
            CheckId::Background => "app",
            CheckId::Notifications => "notifications",
            CheckId::Autostart => "autostart",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Ok,
    NeedsAction,
    /// Apps can't read it; the owner checks it in the screen Loss opens.
    CheckYourself,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    pub id: CheckId,
    pub state: CheckState,
}

impl Status {
    pub fn parse(json: &str) -> Option<Status> {
        serde_json::from_str(json).ok()
    }

    /// Xiaomi, Redmi and POCO phones (HyperOS / MIUI).
    pub fn is_xiaomi(&self) -> bool {
        let m = self.manufacturer.to_ascii_lowercase();
        ["xiaomi", "redmi", "poco"].iter().any(|x| m.contains(x))
    }

    /// The checks to show, most important first.
    pub fn checks(&self) -> Vec<Check> {
        let state = |ok: bool| {
            if ok {
                CheckState::Ok
            } else {
                CheckState::NeedsAction
            }
        };
        let mut v = vec![
            Check {
                id: CheckId::Battery,
                state: state(self.battery_unrestricted),
            },
            Check {
                id: CheckId::Background,
                state: state(!self.background_restricted),
            },
            Check {
                id: CheckId::Notifications,
                state: state(self.notifications),
            },
        ];
        if self.is_xiaomi() || self.autostart_screen {
            v.push(Check {
                id: CheckId::Autostart,
                state: CheckState::CheckYourself,
            });
        }
        v
    }

    /// Whether playback is likely to be stopped with the screen off (worth a notice on Home).
    pub fn needs_attention(&self) -> bool {
        !self.battery_unrestricted || self.background_restricted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XIAOMI_DEFAULT: &str = r#"{"manufacturer":"Xiaomi","sdk":36,"battery_unrestricted":false,
        "notifications":true,"background_restricted":false,"autostart_screen":true}"#;

    #[test]
    fn xiaomi_out_of_the_box_needs_the_battery_exemption() {
        let s = Status::parse(XIAOMI_DEFAULT).unwrap();
        assert!(s.is_xiaomi());
        assert!(s.needs_attention());
        let c = s.checks();
        assert_eq!(
            c[0],
            Check {
                id: CheckId::Battery,
                state: CheckState::NeedsAction
            }
        );
        assert_eq!(c[1].state, CheckState::Ok);
        assert_eq!(c[2].state, CheckState::Ok);
        assert_eq!(
            c[3],
            Check {
                id: CheckId::Autostart,
                state: CheckState::CheckYourself
            }
        );
    }

    #[test]
    fn all_set() {
        let s = Status {
            manufacturer: "Xiaomi".into(),
            battery_unrestricted: true,
            notifications: true,
            ..Default::default()
        };
        assert!(!s.needs_attention());
        assert!(s
            .checks()
            .iter()
            .all(|c| c.state != CheckState::NeedsAction));
    }

    #[test]
    fn other_phones_have_no_autostart_row() {
        let s = Status {
            manufacturer: "Google".into(),
            battery_unrestricted: true,
            notifications: false,
            background_restricted: true,
            ..Default::default()
        };
        assert!(s.needs_attention());
        let ids: Vec<CheckId> = s.checks().iter().map(|c| c.id).collect();
        assert!(!ids.contains(&CheckId::Autostart));
        assert_eq!(s.checks()[1].state, CheckState::NeedsAction); // background restricted
        assert_eq!(s.checks()[2].state, CheckState::NeedsAction); // notifications off
    }

    #[test]
    fn missing_fields_and_garbage() {
        assert_eq!(Status::parse("{}"), Some(Status::default()));
        assert_eq!(Status::parse("nope"), None);
        assert!(Status {
            manufacturer: "POCO".into(),
            ..Default::default()
        }
        .is_xiaomi());
    }
}
