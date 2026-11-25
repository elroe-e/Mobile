//! Settings → Audio → Equalizer (M2-10): on/off, the response curve, presets, bands, tone and
//! stereo, dynamics. Sliders play live while they move and are saved when let go. The sound is
//! `loss_audio::eq`; settings and presets are `loss_core::equalizer`.

use super::icons::Icon;
use super::kit::{Dialog, Sheet};
use super::settings::{SettingsNav, SettingsPage, Toggle};
use super::{state, Slider};
use crate::services;
use dioxus::prelude::*;
use loss_core::equalizer::{self as eqs, EqPreset, EqSettings, ImportError, BAND_COUNTS, BUILT_IN};
use loss_i18n::t;

/// Saves and applies.
fn save(mut cur: Signal<EqSettings>, next: EqSettings) {
    cur.set(next.clone());
    services::get().update_settings(|s| s.equalizer = next);
}

/// Plays without saving (while a slider moves).
fn preview(cur: Signal<EqSettings>, next: EqSettings) {
    let mut c = cur;
    c.set(next.clone());
    services::get().preview_eq(&next);
}

fn signed(v: f64) -> String {
    if v > 0.05 {
        format!("+{v:.1}")
    } else {
        format!("{v:.1}")
    }
}

fn hz_label(f: f64) -> String {
    if f >= 1000.0 {
        let k = f / 1000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{}k", k.round()) // i18n: ignore (unit)
        } else {
            format!("{k:.1}k") // i18n: ignore (unit)
        }
    } else if (f - f.round()).abs() < 0.05 {
        format!("{}", f.round())
    } else {
        format!("{f:.1}")
    }
}

/// The response curve as an SVG path over 20 Hz–20 kHz, ±15 dB.
fn curve_path(e: &EqSettings) -> (String, f64) {
    let params = services::eq_params(&EqSettings {
        enabled: true,
        ..e.clone()
    })
    .unwrap_or_default();
    let rate = 48_000.0;
    let (w, h) = (300.0, 90.0);
    let mut d = String::new();
    for i in 0..=120 {
        let f = 20.0 * 1000f64.powf(i as f64 / 120.0);
        let db = params.response_db(f, rate).clamp(-15.0, 15.0);
        let x = w * i as f64 / 120.0;
        let y = h / 2.0 - db / 15.0 * (h / 2.0 - 4.0);
        let cmd = if i == 0 { "M" } else { "L" }; // i18n: ignore (SVG path commands)
        d.push_str(&format!("{cmd}{x:.1},{y:.1} "));
    }
    (d, params.input_gain_db(rate))
}

#[component]
pub fn EqualizerPage() -> Element {
    let st = state();
    let cur: Signal<EqSettings> = use_signal(|| services::get().settings().equalizer);
    let mut presets: Signal<Vec<EqPreset>> = use_signal(|| services::get().settings().eq_presets);
    let mut reset_open = use_signal(|| false);
    let mut save_open = use_signal(|| false);
    let mut import_open = use_signal(|| false);
    let mut name = use_signal(String::new);
    let mut import_text = use_signal(String::new);
    let mut import_error = use_signal(|| None::<String>);

    let e = cur();
    let bypassed = services::get().effective_mode() == loss_platform::modes::Mode::BitPerfect;
    let (path, input_gain) = curve_path(&e);
    let centres = eqs::centres(e.gains.len());
    let custom = !BUILT_IN
        .iter()
        .any(|(_, c)| e.sounds_like(&eqs::built_in(c)))
        && !presets().iter().any(|p| e.sounds_like(&p.settings));
    let user_presets = presets();

    rsx! {
        div { class: "page-top",
            button {
                class: "back",
                "aria-label": t!("action-back"),
                onclick: move |_| {
                    let mut page = use_context::<SettingsNav>().0;
                    page.set(SettingsPage::Main);
                },
                Icon { name: "back" }
            }
            button {
                class: "icon-btn",
                "aria-label": t!("eq-reset"),
                onclick: move |_| reset_open.set(true),
                Icon { name: "refresh" }
            }
        }
        header { class: "page-header", h1 { {t!("eq-title")} } }

        div { class: "card",
            Toggle {
                label: t!("eq-enable"),
                hint: String::new(),
                on: e.enabled,
                onchange: move |v| {
                    let mut n = cur();
                    n.enabled = v;
                    save(cur, n);
                },
            }
        }
        if bypassed && e.enabled {
            p { class: "notice pad", {t!("eq-bypass")} }
        }

        // The curve.
        div { class: "card eq-curve",
            div { class: "row-text",
                div { class: "title", {t!("eq-curve")} }
                div { class: "subtitle", {t!("eq-input-gain", db = signed(input_gain))} }
            }
            svg {
                view_box: "0 0 300 90",
                class: "eq-graph",
                "aria-hidden": "true",
                line { x1: "0", y1: "45", x2: "300", y2: "45", class: "eq-zero" }
                path { d: "{path}", class: "eq-line" }
            }
        }

        // Presets.
        h2 { class: "section-title", {t!("eq-presets")} }
        div { class: "chips eq-presets",
            if custom {
                span { class: "np-chip on", {t!("eq-custom")} }
            }
            for (id , curve) in BUILT_IN.iter() {
                button {
                    key: "{id}",
                    class: if e.sounds_like(&eqs::built_in(curve)) { "np-chip on" } else { "np-chip" },
                    onclick: move |_| {
                        let n = cur().with_sound_of(&eqs::built_in(curve));
                        save(cur, n);
                    },
                    {preset_name(id)}
                }
            }
            for (i , p) in user_presets.iter().enumerate() {
                span {
                    key: "user-{i}",
                    class: if e.sounds_like(&p.settings) { "np-chip on" } else { "np-chip" },
                    button {
                        class: "chip-main",
                        onclick: {
                            let s = p.settings.clone();
                            move |_| {
                                let n = cur().with_sound_of(&s);
                                save(cur, n);
                            }
                        },
                        "{p.name}"
                    }
                    button {
                        class: "chip-x",
                        "aria-label": t!("eq-delete-preset"),
                        onclick: move |_| {
                            let mut list = presets();
                            if i < list.len() {
                                list.remove(i);
                            }
                            presets.set(list.clone());
                            services::get().update_settings(|s| s.eq_presets = list);
                        },
                        Icon { name: "close", size: 16 }
                    }
                }
            }
        }
        div { class: "row pad",
            button { class: "pill", onclick: move |_| save_open.set(true), {t!("eq-save-short")} }
            button {
                class: "pill",
                onclick: move |_| {
                    import_error.set(None);
                    import_open.set(true);
                },
                {t!("eq-import-short")}
            }
            button {
                class: "pill",
                onclick: move |_| {
                    let p = EqPreset { name: t!("eq-share-name"), settings: cur() };
                    super::share_text(st, &t!("eq-share-name"), &p.to_json());
                },
                {t!("eq-share-short")}
            }
        }

        // Bands.
        h2 { class: "section-title", {t!("eq-bands")} }
        div { class: "card",
            div { class: "stepper",
                for n in BAND_COUNTS {
                    button {
                        class: if e.gains.len() == n { "seg active" } else { "seg" },
                        onclick: move |_| {
                            let next = cur().with_band_count(n);
                            save(cur, next);
                        },
                        "{n}"
                    }
                }
            }
            div { class: "eq-bands",
                for (i , (g , f)) in e.gains.iter().copied().zip(centres.iter().copied()).enumerate() {
                    div { key: "{e.gains.len()}-{i}", class: "eq-band",
                        span { class: "eq-db", {signed(g)} }
                        div { class: "eq-slot",
                        Slider {
                            value: g + 15.0,
                            max: 30.0,
                            step: 0.5,
                            class: "vertical",
                            on_input: move |v: f64| {
                                let mut n = cur();
                                if let Some(x) = n.gains.get_mut(i) {
                                    *x = v - 15.0;
                                }
                                preview(cur, n);
                            },
                            on_commit: move |v: f64| {
                                let mut n = cur();
                                if let Some(x) = n.gains.get_mut(i) {
                                    *x = v - 15.0;
                                }
                                save(cur, n);
                            },
                        }
                        }
                        span { class: "eq-hz", {hz_label(f)} }
                    }
                }
            }
        }

        // Tone and stereo.
        h2 { class: "section-title", {t!("eq-tone")} }
        div { class: "card",
            EqSlider { cur, field: Field::Preamp }
            EqSlider { cur, field: Field::Bass }
            EqSlider { cur, field: Field::Treble }
            EqSlider { cur, field: Field::Balance }
        }

        // Dynamics.
        h2 { class: "section-title", {t!("eq-dynamics")} }
        div { class: "card",
            Toggle {
                label: t!("eq-headroom"),
                hint: t!("eq-headroom-desc"),
                on: e.auto_headroom,
                onchange: move |v| {
                    let mut n = cur();
                    n.auto_headroom = v;
                    save(cur, n);
                },
            }
            Toggle {
                label: t!("eq-limiter"),
                hint: t!("eq-limiter-desc"),
                on: e.limiter,
                onchange: move |v| {
                    let mut n = cur();
                    n.limiter = v;
                    save(cur, n);
                },
            }
            Toggle {
                label: t!("eq-compressor"),
                hint: t!("eq-compressor-desc"),
                on: e.compressor,
                onchange: move |v| {
                    let mut n = cur();
                    n.compressor = v;
                    save(cur, n);
                },
            }
            if e.compressor {
                EqSlider { cur, field: Field::Threshold }
                EqSlider { cur, field: Field::Ratio }
                EqSlider { cur, field: Field::Attack }
                EqSlider { cur, field: Field::Release }
            }
        }

        Dialog {
            open: reset_open,
            title: t!("eq-reset-title"),
            text: t!("eq-reset-text"),
            confirm: t!("eq-reset"),
            on_confirm: move |_| {
                let n = cur().reset();
                save(cur, n);
            },
        }
        Sheet { open: save_open, title: t!("eq-save"),
            div { class: "pad",
                input {
                    r#type: "text",
                    placeholder: t!("eq-preset-name"),
                    initial_value: "",
                    oninput: move |ev| name.set(ev.value()),
                }
                button {
                    class: "primary",
                    onclick: move |_| {
                        let n = name().trim().chars().take(60).collect::<String>();
                        if n.is_empty() {
                            return;
                        }
                        let mut list = presets();
                        list.retain(|p| p.name != n);
                        list.push(EqPreset { name: n, settings: cur() });
                        presets.set(list.clone());
                        services::get().update_settings(|s| s.eq_presets = list);
                        save_open.set(false);
                        st.toast(t!("eq-saved"));
                    },
                    {t!("eq-save")}
                }
            }
        }
        Sheet { open: import_open, title: t!("eq-import"),
            div { class: "pad",
                p { class: "muted small", {t!("eq-import-hint")} }
                textarea {
                    class: "links",
                    rows: "6",
                    initial_value: "",
                    oninput: move |ev| import_text.set(ev.value()),
                }
                if let Some(err) = import_error() {
                    p { class: "small error", "{err}" }
                }
                button {
                    class: "primary",
                    onclick: move |_| match EqPreset::from_json(&import_text(), &t!("eq-imported-name")) {
                        Ok(p) => {
                            let mut list = presets();
                            list.retain(|q| q.name != p.name);
                            let settings = p.settings.clone();
                            list.push(p);
                            presets.set(list.clone());
                            services::get().update_settings(|s| s.eq_presets = list);
                            let n = cur().with_sound_of(&settings);
                            save(cur, n);
                            import_open.set(false);
                            st.toast(t!("eq-saved"));
                        }
                        Err(ImportError::TooBig) => import_error.set(Some(t!("eq-import-too-big"))),
                        Err(ImportError::Invalid) => import_error.set(Some(t!("eq-import-invalid"))),
                    },
                    {t!("eq-import")}
                }
            }
        }
    }
}

fn balance_label(v: f64) -> String {
    let pct = (v.abs() * 100.0).round() as i64;
    if pct == 0 {
        t!("eq-balance-centre")
    } else if v < 0.0 {
        t!("eq-balance-left", pct = pct)
    } else {
        t!("eq-balance-right", pct = pct)
    }
}

/// One value of the settings with a horizontal slider.
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Preamp,
    Bass,
    Treble,
    Balance,
    Threshold,
    Ratio,
    Attack,
    Release,
}

impl Field {
    fn label(self) -> String {
        match self {
            Field::Preamp => t!("eq-preamp"),
            Field::Bass => t!("eq-bass"),
            Field::Treble => t!("eq-treble"),
            Field::Balance => t!("eq-balance"),
            Field::Threshold => t!("eq-threshold"),
            Field::Ratio => t!("eq-ratio"),
            Field::Attack => t!("eq-attack"),
            Field::Release => t!("eq-release"),
        }
    }

    /// (lowest, highest, step)
    fn range(self) -> (f64, f64, f64) {
        match self {
            Field::Preamp => (-24.0, 12.0, 0.5),
            Field::Bass | Field::Treble => (-12.0, 12.0, 0.5),
            Field::Balance => (-1.0, 1.0, 0.05),
            Field::Threshold => (-60.0, 0.0, 1.0),
            Field::Ratio => (1.0, 20.0, 0.5),
            Field::Attack => (1.0, 100.0, 1.0),
            Field::Release => (20.0, 1000.0, 10.0),
        }
    }

    fn get(self, e: &EqSettings) -> f64 {
        match self {
            Field::Preamp => e.preamp,
            Field::Bass => e.bass,
            Field::Treble => e.treble,
            Field::Balance => e.balance,
            Field::Threshold => e.threshold,
            Field::Ratio => e.ratio,
            Field::Attack => e.attack,
            Field::Release => e.release,
        }
    }

    fn put(self, e: &mut EqSettings, v: f64) {
        let slot = match self {
            Field::Preamp => &mut e.preamp,
            Field::Bass => &mut e.bass,
            Field::Treble => &mut e.treble,
            Field::Balance => &mut e.balance,
            Field::Threshold => &mut e.threshold,
            Field::Ratio => &mut e.ratio,
            Field::Attack => &mut e.attack,
            Field::Release => &mut e.release,
        };
        *slot = v;
    }

    fn show(self, v: f64) -> String {
        match self {
            Field::Preamp | Field::Bass | Field::Treble => t!("eq-value-db", value = signed(v)),
            Field::Balance => balance_label(v),
            Field::Threshold => t!("eq-value-db", value = format!("{v:.0}")),
            Field::Ratio => t!("eq-value-ratio", value = format!("{v:.1}")),
            Field::Attack | Field::Release => t!("eq-value-ms", value = format!("{v:.0}")),
        }
    }
}

/// A built-in preset's name.
fn preset_name(id: &str) -> String {
    match id {
        "eq-preset-flat" => t!("eq-preset-flat"),
        "eq-preset-bass" => t!("eq-preset-bass"),
        "eq-preset-treble" => t!("eq-preset-treble"),
        "eq-preset-vocal" => t!("eq-preset-vocal"),
        "eq-preset-rock" => t!("eq-preset-rock"),
        "eq-preset-pop" => t!("eq-preset-pop"),
        "eq-preset-jazz" => t!("eq-preset-jazz"),
        "eq-preset-classical" => t!("eq-preset-classical"),
        "eq-preset-electronic" => t!("eq-preset-electronic"),
        "eq-preset-acoustic" => t!("eq-preset-acoustic"),
        "eq-preset-loudness" => t!("eq-preset-loudness"),
        _ => t!("eq-preset-spoken"),
    }
}

#[component]
fn EqSlider(cur: Signal<EqSettings>, field: Field) -> Element {
    let v = field.get(&cur());
    let (lo, hi, step) = field.range();
    rsx! {
        div { class: "setting col",
            div { class: "row-text eq-row",
                div { class: "title", {field.label()} }
                div { class: "subtitle", {field.show(v)} }
            }
            Slider {
                value: v - lo,
                max: hi - lo,
                step,
                on_input: move |x: f64| {
                    let mut n = cur();
                    field.put(&mut n, x + lo);
                    preview(cur, n);
                },
                on_commit: move |x: f64| {
                    let mut n = cur();
                    field.put(&mut n, x + lo);
                    save(cur, n);
                },
            }
        }
    }
}
