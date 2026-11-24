//! USB DACs for the probe screen (M2-06): list them, read their descriptors, ask them about
//! volume and rates. On Android the glue lists devices and hands over an open file descriptor
//! after the permission prompt; on desktop Linux the descriptors come from sysfs.

use loss_usb::Probe;

#[derive(Debug, Clone, PartialEq)]
pub struct UsbDevice {
    /// Android device path or sysfs directory; passed back to [`probe`].
    pub name: String,
    pub vendor: u16,
    pub product: u16,
    pub label: String,
    /// Has an audio interface.
    pub audio: bool,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// Android is asking the user; a `UsbPermission` event follows.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    AskingPermission,
    Done(Box<Probe>),
    Failed(String),
}

fn label(manufacturer: &str, product: &str, vendor: u16, id: u16) -> String {
    let name = format!("{} {}", manufacturer.trim(), product.trim());
    let name = name.trim();
    if name.is_empty() {
        format!("{vendor:04x}:{id:04x}")
    } else {
        name.to_string()
    }
}

/// USB devices with an audio interface.
pub fn list() -> Vec<UsbDevice> {
    let mut all = list_all();
    all.retain(|d| d.audio);
    all
}

#[cfg(target_os = "android")]
fn list_all() -> Vec<UsbDevice> {
    #[derive(serde::Deserialize)]
    struct J {
        name: String,
        vendor: u16,
        product: u16,
        #[serde(default)]
        manufacturer: String,
        #[serde(default)]
        product_name: String,
        #[serde(default)]
        classes: Vec<i32>,
    }
    let Some(json) = crate::platform::usb_devices() else {
        return vec![];
    };
    serde_json::from_str::<Vec<J>>(&json)
        .unwrap_or_default()
        .into_iter()
        .map(|j| UsbDevice {
            label: label(&j.manufacturer, &j.product_name, j.vendor, j.product),
            audio: j.classes.contains(&1),
            name: j.name,
            vendor: j.vendor,
            product: j.product,
        })
        .collect()
}

/// Desktop debug builds: `LOSS_DAC_FIXTURE=<hex file>` adds a pretend DAC read from a report or
/// fixture (e.g. `crates/loss-usb/fixtures/uac1-headset.hex`), for checking the page.
#[cfg(not(target_os = "android"))]
fn fixture() -> Option<(String, Vec<u8>)> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let path = std::env::var("LOSS_DAC_FIXTURE").ok()?;
    let raw = loss_usb::unhex(&std::fs::read_to_string(&path).ok()?);
    Some((format!("fixture:{path}"), raw))
}

#[cfg(not(target_os = "android"))]
fn list_all() -> Vec<UsbDevice> {
    let mut out = Vec::new();
    if let Some((name, raw)) = fixture() {
        if let Ok(p) = loss_usb::parse(&raw) {
            out.push(UsbDevice {
                name,
                vendor: p.vendor,
                product: p.product,
                label: label("Test", "headset", p.vendor, p.product),
                audio: p.is_audio(),
            });
        }
    }
    let Ok(dir) = std::fs::read_dir("/sys/bus/usb/devices") else {
        return out;
    };
    for e in dir.flatten() {
        let p = e.path();
        let Ok(raw) = std::fs::read(p.join("descriptors")) else {
            continue;
        };
        let Ok(parsed) = loss_usb::parse(&raw) else {
            continue;
        };
        let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap_or_default();
        out.push(UsbDevice {
            name: p.to_string_lossy().into_owned(),
            vendor: parsed.vendor,
            product: parsed.product,
            label: label(
                &read("manufacturer"),
                &read("product"),
                parsed.vendor,
                parsed.product,
            ),
            audio: parsed.is_audio(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Reads one device. Blocking (control requests); call off the UI thread.
#[cfg(target_os = "android")]
pub fn probe(name: &str) -> Outcome {
    let fd = match crate::platform::usb_open(name) {
        Some(-1) => return Outcome::AskingPermission,
        Some(fd) if fd >= 0 => fd,
        _ => return Outcome::Failed("the device is gone or can't be opened".into()),
    };
    let result = loss_usb::devfs::read_descriptors(fd).and_then(|raw| {
        let mut p = Probe::new(raw)?;
        p.queried = Some(loss_usb::devfs::query(fd, &p.parsed));
        Ok(p)
    });
    crate::platform::usb_close(name);
    match result {
        Ok(p) => Outcome::Done(Box::new(p)),
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(not(target_os = "android"))]
pub fn probe(name: &str) -> Outcome {
    if let Some((_, raw)) = fixture().filter(|(n, _)| n == name) {
        return match Probe::new(raw) {
            Ok(p) => Outcome::Done(Box::new(p)),
            Err(e) => Outcome::Failed(e),
        };
    }
    let path = std::path::Path::new(name).join("descriptors");
    match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(Probe::new)
    {
        Ok(p) => Outcome::Done(Box::new(p)),
        Err(e) => Outcome::Failed(e),
    }
}
