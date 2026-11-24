//! Audio outputs and the media route (M2-03). The glue reports every output with what Android
//! knows about it (`LossBridge.outputs`); this picks the one music plays through and tells the
//! engine its sample rates, so Loss converts rates itself (e.g. 44.1 → 48 kHz for the Beats
//! Studio Pro, which take only 32 and 48 kHz) instead of leaving it to Android's mixer.

use serde::Deserialize;

/// Android `AudioDeviceInfo.TYPE_*` values Loss tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Speaker,
    Earpiece,
    Wired,
    Usb,
    Bluetooth,
    Hdmi,
    Other,
}

impl Kind {
    pub fn from_android(t: i32) -> Kind {
        match t {
            2 => Kind::Speaker,
            1 => Kind::Earpiece,
            3..=6 => Kind::Wired,
            11 | 12 | 22 => Kind::Usb,
            7 | 8 | 23 | 26 | 27 | 30 => Kind::Bluetooth,
            9 | 10 => Kind::Hdmi,
            _ => Kind::Other,
        }
    }

    /// Which output Android uses when several are connected (USB and wired first).
    fn priority(self) -> u8 {
        match self {
            Kind::Usb => 6,
            Kind::Wired => 5,
            Kind::Bluetooth => 4,
            Kind::Hdmi => 3,
            Kind::Speaker => 2,
            Kind::Other => 1,
            Kind::Earpiece => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Device {
    pub id: i32,
    #[serde(rename = "type")]
    pub type_code: i32,
    pub name: String,
    /// Empty means Android didn't say (it then takes any rate, converting in its mixer).
    #[serde(default)]
    pub rates: Vec<u32>,
    #[serde(default)]
    pub channels: Vec<u32>,
    /// Android `AudioFormat.ENCODING_*` values.
    #[serde(default)]
    pub encodings: Vec<i32>,
}

impl Device {
    pub fn kind(&self) -> Kind {
        Kind::from_android(self.type_code)
    }

    /// The deepest PCM format it takes, in bits (32 for float).
    pub fn max_bits(&self) -> Option<u32> {
        self.encodings
            .iter()
            .filter_map(|e| match e {
                2 => Some(16),  // PCM_16BIT
                3 => Some(8),   // PCM_8BIT
                4 => Some(32),  // PCM_FLOAT
                21 => Some(24), // PCM_24BIT_PACKED
                22 => Some(32), // PCM_32BIT
                _ => None,
            })
            .max()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Outputs {
    pub devices: Vec<Device>,
    /// Ids media is routed to (Android 13+), or empty.
    #[serde(default)]
    pub active: Vec<i32>,
}

impl Outputs {
    pub fn parse(json: &str) -> Option<Outputs> {
        serde_json::from_str(json).ok()
    }

    /// The output music plays through: what Android says, else the most specific connected
    /// output (USB, then wired, Bluetooth, HDMI, speaker).
    pub fn active(&self) -> Option<&Device> {
        let routed = self
            .devices
            .iter()
            .filter(|d| self.active.contains(&d.id))
            .max_by_key(|d| d.kind().priority());
        routed.or_else(|| {
            self.devices
                .iter()
                .filter(|d| d.kind() != Kind::Earpiece && d.kind() != Kind::Other)
                .max_by_key(|d| d.kind().priority())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The owner's Xiaomi 14 Ultra with the Beats plugged in (docs/plan/reports/).
    const XIAOMI_BEATS: &str = r#"{"devices":[
        {"id":2,"type":1,"name":"24031PN0DC","rates":[48000],"channels":[1],"encodings":[2]},
        {"id":3,"type":2,"name":"24031PN0DC","rates":[48000],"channels":[2],"encodings":[2]},
        {"id":16,"type":18,"name":"24031PN0DC","rates":[8000,16000],"channels":[1,2],"encodings":[2]},
        {"id":33408,"type":22,"name":"USB-Audio - Beats Studio Pro","rates":[32000,48000],"channels":[2],"encodings":[2,4,21]}
    ],"active":[33408]}"#;

    #[test]
    fn the_beats_are_the_active_output() {
        let o = Outputs::parse(XIAOMI_BEATS).unwrap();
        let d = o.active().unwrap();
        assert_eq!(d.name, "USB-Audio - Beats Studio Pro");
        assert_eq!(d.kind(), Kind::Usb);
        assert_eq!(d.rates, vec![32000, 48000]);
        assert_eq!(d.max_bits(), Some(32));
    }

    #[test]
    fn without_a_route_the_most_specific_output_wins() {
        let mut o = Outputs::parse(XIAOMI_BEATS).unwrap();
        o.active.clear();
        assert_eq!(o.active().unwrap().kind(), Kind::Usb);
        // Unplugged: the speaker.
        o.devices.retain(|d| d.kind() != Kind::Usb);
        assert_eq!(o.active().unwrap().kind(), Kind::Speaker);
    }

    #[test]
    fn bluetooth_without_rates_takes_any() {
        let o = Outputs::parse(
            r#"{"devices":[{"id":9,"type":8,"name":"Beats","rates":[],"channels":[],"encodings":[]}],"active":[9]}"#,
        )
        .unwrap();
        let d = o.active().unwrap();
        assert_eq!(d.kind(), Kind::Bluetooth);
        assert!(d.rates.is_empty());
        assert_eq!(Outputs::parse("nope"), None);
    }
}
