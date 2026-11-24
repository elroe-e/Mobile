//! Settings → Audio → USB DAC details (M2-06): what a plugged-in DAC or headset offers, and a
//! report with its raw descriptors to send in. Parsing is in `loss-usb`; listing and opening
//! in `crate::dac`.

use super::icons::Icon;
use super::settings::{SettingsNav, SettingsPage};
use super::state;
use crate::dac::{self, Outcome, UsbDevice};
use crate::services;
use dioxus::prelude::*;
use loss_i18n::t;
use loss_usb::{Probe, Rates};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Clone)]
enum View {
    Reading,
    Asking,
    Failed(String),
    Done(Box<Probe>),
}

async fn read(name: String) -> Outcome {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(dac::probe(&name));
    });
    rx.await
        .unwrap_or_else(|_| Outcome::Failed("stopped".into()))
}

fn khz(r: &[u32]) -> String {
    r.iter()
        .map(|r| {
            format!("{:.1}", *r as f64 / 1000.0)
                .trim_end_matches(".0")
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(", ")
        + " kHz" // i18n: ignore (unit)
}

fn class_name(p: &Probe) -> String {
    match p.parsed.audio_version {
        Some(v) if v >= 0x0300 => t!("dac-class-3"),
        Some(v) if v >= 0x0200 => t!("dac-class-2"),
        _ => t!("dac-class-1"),
    }
}

/// One line per playback format, e.g. "2 channels · 24-bit · 32, 48 kHz".
fn formats(p: &Probe) -> Vec<String> {
    let clock = p.output_rates();
    let mut lines: Vec<String> = p
        .parsed
        .outputs()
        .map(|a| {
            let rates = match &a.rates {
                Rates::Discrete(r) => khz(r),
                Rates::Range(lo, hi) => khz(&[*lo, *hi]),
                Rates::FromClock if !clock.is_empty() => khz(&clock),
                Rates::FromClock => t!("dac-rates-from-device"),
            };
            let (ch, bits) = (a.channels as i64, a.bits as i64);
            t!("dac-format", channels = ch, bits = bits, rates = rates)
        })
        .collect();
    lines.dedup();
    lines
}

fn volume(p: &Probe) -> String {
    let ranges: Vec<String> = p
        .queried
        .iter()
        .flat_map(|q| q.volume.iter())
        .filter_map(|(_, r)| r.as_ref().ok())
        .map(|(lo, hi, _)| {
            let (min, max) = (format!("{lo:.0}"), format!("{hi:.0}"));
            t!("dac-volume", min = min, max = max)
        })
        .collect();
    match ranges.first() {
        Some(r) => r.clone(),
        None if p.parsed.features.iter().any(|f| f.volume) => t!("dac-volume-unknown"),
        None => t!("dac-volume-none"),
    }
}

#[component]
pub fn DacProbePage() -> Element {
    let st = state();
    let mut devices: Signal<Vec<UsbDevice>> = use_signal(dac::list);
    let mut views: Signal<HashMap<String, View>> = use_signal(HashMap::new);
    let mut start = move |name: String| {
        views.write().insert(name.clone(), View::Reading);
        spawn(async move {
            let v = match read(name.clone()).await {
                Outcome::AskingPermission => View::Asking,
                Outcome::Failed(e) => View::Failed(e),
                Outcome::Done(p) => View::Done(p),
            };
            views.write().insert(name, v);
        });
    };
    // Plugging, unplugging and permission answers bump the counter: list again, and read the
    // devices whose permission was being asked for.
    use_future(move || async move {
        let mut seen = services::get().usb_version.load(Ordering::Relaxed);
        loop {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let now = services::get().usb_version.load(Ordering::Relaxed);
            if now == seen {
                continue;
            }
            seen = now;
            devices.set(dac::list());
            let asking: Vec<String> = views
                .peek()
                .iter()
                .filter(|(_, v)| matches!(v, View::Asking))
                .map(|(n, _)| n.clone())
                .collect();
            for n in asking {
                start(n);
            }
        }
    });

    let list = devices();
    let shown = views();
    rsx! {
        button {
            class: "back",
            "aria-label": t!("action-back"),
            onclick: move |_| {
                let mut page = use_context::<SettingsNav>().0;
                page.set(SettingsPage::Main);
            },
            Icon { name: "back" }
        }
        header { class: "page-header", h1 { {t!("dac-title")} } }
        p { class: "muted pad", {t!("dac-intro")} }
        if list.is_empty() {
            p { class: "muted pad", {t!("dac-none")} }
        }
        for d in list {
            div { key: "{d.name}", class: "card",
                div { class: "setting",
                    Icon { name: "usb", size: 22 }
                    div { class: "row-text",
                        div { class: "title", "{d.label}" }
                        div { class: "subtitle", {format!("{:04x}:{:04x}", d.vendor, d.product)} }
                    }
                    if !matches!(shown.get(&d.name), Some(View::Reading)) {
                        button {
                            class: "pill",
                            onclick: {
                                let n = d.name.clone();
                                move |_| start(n.clone())
                            },
                            {t!("dac-read")}
                        }
                    }
                }
                match shown.get(&d.name).cloned() {
                    None => rsx! {},
                    Some(View::Reading) => rsx! { p { class: "muted pad", {t!("dac-reading")} } },
                    Some(View::Asking) => rsx! { p { class: "muted pad", {t!("dac-asking")} } },
                    Some(View::Failed(e)) => rsx! { p { class: "small error pad", {t!("dac-failed", error = e)} } },
                    Some(View::Done(p)) => {
                        let report = p.report();
                        let subject = t!("dac-report-subject", name = d.label.clone());
                        let (r1, r2, s2) = (report.clone(), report, subject.clone());
                        rsx! {
                            div { class: "setting",
                                Icon { name: "info", size: 22 }
                                div { class: "row-text",
                                    div { class: "title", {class_name(&p)} }
                                    for line in formats(&p) {
                                        div { class: "subtitle", "{line}" }
                                    }
                                    div { class: "subtitle", {volume(&p)} }
                                }
                            }
                            div { class: "row pad",
                                button {
                                    class: "pill",
                                    onclick: move |_| super::copy_text(st, &subject, &r1),
                                    Icon { name: "copy", size: 18 }
                                    {t!("dac-copy")}
                                }
                                button {
                                    class: "pill",
                                    onclick: move |_| super::share_text(st, &s2, &r2),
                                    {t!("dac-share")}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
