//! Settings, built from the registry (`loss_core::registry`), with search. Plus playlist
//! import, listening stats, logs and about.

use super::icons::Icon;
use super::state;
use crate::services;
use dioxus::prelude::*;
use loss_core::registry::{self, Kind, Section, Value};
use loss_core::{format_duration, playlist, Settings, SubsonicConfig};
use loss_i18n::t;
use std::path::PathBuf;

#[component]
fn Toggle(label: String, hint: String, on: bool, onchange: EventHandler<bool>) -> Element {
    rsx! {
        div { class: "setting", onclick: move |_| onchange.call(!on),
            div { class: "row-text",
                div { class: "title", "{label}" }
                if !hint.is_empty() {
                    div { class: "subtitle", "{hint}" }
                }
            }
            div { class: if on { "switch on" } else { "switch" }, div { class: "knob" } }
        }
    }
}

/// Which Settings page is open; kept by the App so Back can return to the list.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SettingsPage {
    Main,
    Logs,
    Reliability,
    /// Debug builds only.
    Gallery,
}

#[derive(Clone, Copy)]
pub struct SettingsNav(pub Signal<SettingsPage>);

pub fn use_provide_nav() -> SettingsNav {
    use_context_provider(|| SettingsNav(Signal::new(SettingsPage::Main)))
}

#[component]
pub fn SettingsTab() -> Element {
    let nav = use_context::<SettingsNav>();
    match (nav.0)() {
        SettingsPage::Main => rsx! { SettingsMain {} },
        SettingsPage::Logs => rsx! { super::logs::LogsPage {} },
        SettingsPage::Reliability => rsx! { super::reliability::ReliabilityPage {} },
        SettingsPage::Gallery => rsx! { super::gallery::GalleryPage {} },
    }
}

/// Applies side effects a setting needs beyond being saved.
fn after_change(key: &str, st: super::AppState) {
    match key {
        "theme" | "dynamic_color" | "accent" => st.settings_changed(),
        "detailed_logging" => {
            crate::diag::set_detailed(services::get().settings().detailed_logging)
        }
        "language" => {
            let pref = services::get().settings().language;
            loss_i18n::set_language(&pref, crate::platform::system_locale().as_deref());
            st.settings_changed();
        }
        _ => {}
    }
}

/// One setting from the registry, drawn according to its kind. Custom kinds have their own
/// components (folders, server) and are not drawn here.
#[component]
fn SettingRow(
    setting: &'static str,
    cur: Settings,
    on_set: EventHandler<(&'static str, Value)>,
) -> Element {
    let key = setting;
    let Some(d) = registry::find(key) else {
        return rsx! {};
    };
    let value = registry::get(&cur, key);
    let title = d.title();
    let hint = d.description();
    // Buttons go under the title when they wouldn't leave it room on a phone.
    let options = match &d.kind {
        Kind::Choice(o) => o.len(),
        Kind::Steps(o) => o.len(),
        _ => 0,
    };
    let stacked = title.chars().count() + options * 4 > 30 || !hint.is_empty();
    match (&d.kind, value) {
        (Kind::Toggle, Some(Value::Bool(on))) => rsx! {
            Toggle {
                label: title,
                hint,
                on,
                onchange: move |v| {
                    super::kit::haptic(super::kit::Haptic::Confirm);
                    on_set.call((key, Value::Bool(v)))
                },
            }
        },
        (Kind::Choice(opts), Some(Value::Text(v))) => rsx! {
            div { class: if stacked { "setting col" } else { "setting" },
                div { class: "row-text",
                    div { class: "title", "{title}" }
                    if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
                }
                div { class: "stepper",
                    for (opt , label) in opts.iter().map(|o| (*o, d.choice_label(o))) {
                        button {
                            class: if v == opt { "seg active" } else { "seg" },
                            onclick: move |_| {
                                super::kit::haptic(super::kit::Haptic::Tick);
                                on_set.call((key, Value::Text(opt.to_string())))
                            },
                            "{label}"
                        }
                    }
                }
            }
        },
        (Kind::Steps(opts), Some(Value::Int(v))) => rsx! {
            div { class: if stacked { "setting col" } else { "setting" },
                div { class: "row-text",
                    div { class: "title", "{title}" }
                    if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
                }
                div { class: "stepper",
                    for n in opts.iter().copied() {
                        button {
                            class: if v == n { "seg active" } else { "seg" },
                            onclick: move |_| {
                                super::kit::haptic(super::kit::Haptic::Tick);
                                on_set.call((key, Value::Int(n)))
                            },
                            "{n}"
                        }
                    }
                }
            }
        },
        (Kind::Text { placeholder }, Some(Value::Text(v))) => rsx! {
            div { class: "setting col",
                div { class: "title", "{title}" }
                input {
                    placeholder: *placeholder,
                    initial_value: "{v}",
                    onchange: move |e: FormEvent| on_set.call((key, Value::Text(e.value()))),
                }
                if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
            }
        },
        (Kind::Folder, Some(Value::Text(v))) => rsx! {
            div { class: "setting col",
                div { class: "title", "{title}" }
                input {
                    class: "mono",
                    initial_value: "{v}",
                    onchange: move |e: FormEvent| on_set.call((key, Value::Text(e.value()))),
                }
                if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
            }
        },
        (Kind::Color(opts), Some(Value::Text(v))) => rsx! {
            div { class: "setting col",
                div { class: "title", "{title}" }
                div { class: "swatches",
                    for c in opts.iter().copied() {
                        button {
                            class: if v == c { "swatch active" } else { "swatch" },
                            style: "background: {c}",
                            "aria-label": "{c}",
                            onclick: move |_| {
                                super::kit::haptic(super::kit::Haptic::Tick);
                                on_set.call((key, Value::Text(c.to_string())))
                            },
                        }
                    }
                }
                if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
            }
        },
        _ => rsx! {},
    }
}

#[component]
fn SettingsMain() -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let mut s = use_signal(|| svc.settings());
    let mut query = use_signal(String::new);
    let set = move |(key, v): (&'static str, Value)| {
        services::get().update_settings(move |s| {
            if let Err(e) = registry::set(s, key, v) {
                log::warn!("setting {key}: {e:?}");
            }
        });
        s.set(services::get().settings());
        after_change(key, st);
    };
    let reload = move |_| s.set(services::get().settings());
    let cur = s();
    let q = query();
    let results = registry::search(&q);
    // Results grouped by section, sections in the order their best match appears.
    let mut groups: Vec<(Section, Vec<&'static registry::SettingDef>)> = Vec::new();
    for d in results.iter().copied() {
        match groups.iter_mut().find(|(sec, _)| *sec == d.section) {
            Some((_, v)) => v.push(d),
            None => groups.push((d.section, vec![d])),
        }
    }

    // Stats
    let (count, secs, top): (usize, f64, Vec<(String, String, u32)>) = {
        let lib = svc.library();
        (
            lib.len(),
            lib.tracks.iter().map(|t| t.duration_secs).sum(),
            lib.most_played(5)
                .into_iter()
                .map(|t| (t.title.clone(), t.artist.clone(), t.play_count))
                .collect(),
        )
    };
    let plays: u32 = svc.library().tracks.iter().map(|t| t.play_count).sum();

    rsx! {
        header { class: "page-header", h1 { {t!("tab-settings")} } }
        div { class: "search-box settings-search",
            Icon { name: "search", size: 20 }
            input {
                r#type: "search",
                placeholder: t!("settings-search"),
                initial_value: "{q}",
                oninput: move |e| query.set(e.value()),
            }
        }

        if !q.trim().is_empty() {
            if results.is_empty() {
                p { class: "muted empty-note", {t!("settings-no-match", query = q.trim())} }
            }
            for (section , defs) in groups {
                div { key: "{section.title()}",
                    div { class: "search-section muted small", "{section.title()}" }
                    div { class: "card",
                        for d in defs.iter().copied().filter(|d| d.key != "subsonic") {
                            SectionItem { key: "{d.key}", def_key: d.key, cur: cur.clone(), on_set: set, on_saved: reload }
                        }
                    }
                    if defs.iter().any(|d| d.key == "subsonic") {
                        SectionItem { def_key: "subsonic", cur: cur.clone(), on_set: set, on_saved: reload }
                    }
                }
            }
        } else {
            for section in Section::ALL {
                h2 { class: "section-title", "{section.title()}" }
                SectionCard { section, cur: cur.clone(), on_set: set, on_saved: reload }
                if section == Section::Sources {
                    h2 { class: "section-title", {t!("settings-import-playlist")} }
                    PlaylistImport {}

                    h2 { class: "section-title", {t!("settings-your-listening")} }
                    div { class: "card",
                        div { class: "stats",
                            div { div { class: "big", "{count}" } div { class: "muted small", "songs" } }
                            div { div { class: "big", "{format_duration(secs)}" } div { class: "muted small", "of music" } }
                            div { div { class: "big", "{plays}" } div { class: "muted small", "plays" } }
                        }
                        if !top.is_empty() {
                            div { class: "subtitle", {t!("stats-most-played")} }
                            for (i , (t , a , n)) in top.iter().enumerate() {
                                div { key: "{i}", class: "small", "{i + 1}. {t} — {a} ({n})" }
                            }
                        }
                        p { class: "muted small", {t!("stats-private")} }
                    }
                }
            }

            h2 { class: "section-title", {t!("settings-about")} }
            div { class: "card",
                p { {t!("about-version", version = env!("CARGO_PKG_VERSION"))} }
                p { class: "muted small",
                    {t!("about-credits")}
                }
            }
        }
        div { style: "height: 24px" }
    }
}

/// A section's settings in one card; the server form gets its own card after it.
#[component]
fn SectionCard(
    section: Section,
    cur: Settings,
    on_set: EventHandler<(&'static str, Value)>,
    on_saved: EventHandler<()>,
) -> Element {
    let defs: Vec<&'static str> = registry::in_section(section).map(|d| d.key).collect();
    let in_card: Vec<&'static str> = defs.iter().copied().filter(|k| *k != "subsonic").collect();
    let has_server = defs.contains(&"subsonic");
    rsx! {
        div { class: "card",
            if section == Section::Audio {
                OutputRow {}
            }
            if section == Section::Help {
                LogsRow {}
                ReliabilityRow {}
                if cfg!(debug_assertions) {
                    button {
                        class: "setting nav-row",
                        onclick: move |_| {
                            let mut page = use_context::<SettingsNav>().0;
                            page.set(SettingsPage::Gallery);
                        },
                        Icon { name: "grid", size: 22 }
                        div { class: "row-text",
                            div { class: "title", {t!("gallery-title")} }
                            div { class: "subtitle", {t!("gallery-desc")} }
                        }
                        Icon { name: "chevron_right" }
                    }
                }
            }
            for k in in_card {
                SectionItem { key: "{k}", def_key: k, cur: cur.clone(), on_set, on_saved }
            }
        }
        if has_server {
            SubsonicSettings { current: cur.subsonic.clone().unwrap_or_default(), on_saved: move |_| on_saved.call(()) }
        }
    }
}

/// One registry entry: the generic row, or the custom widget for lists and the server.
#[component]
fn SectionItem(
    def_key: &'static str,
    cur: Settings,
    on_set: EventHandler<(&'static str, Value)>,
    on_saved: EventHandler<()>,
) -> Element {
    match def_key {
        "music_folders" => rsx! { MusicFolders { cur, on_saved } },
        "subsonic" => rsx! {
            SubsonicSettings { current: cur.subsonic.clone().unwrap_or_default(), on_saved: move |_| on_saved.call(()) }
        },
        _ => rsx! { SettingRow { setting: def_key, cur, on_set } },
    }
}

#[component]
fn LogsRow() -> Element {
    rsx! {
        button {
            class: "setting nav-row",
            onclick: move |_| {
                let mut page = use_context::<SettingsNav>().0;
                page.set(SettingsPage::Logs);
            },
            Icon { name: "info", size: 22 }
            div { class: "row-text",
                div { class: "title", {t!("settings-logs")} }
                div { class: "subtitle", {t!("settings-logs-desc")} }
            }
            Icon { name: "chevron_right" }
        }
    }
}

/// What Loss plays through and the rates it takes (Android; M2-03).
#[component]
fn OutputRow() -> Element {
    let st = state();
    let stream = st.player.read().info.output.clone();
    let Some(d) = services::get().output.lock().ok().and_then(|o| o.clone()) else {
        return rsx! {};
    };
    let rates = if d.rates.is_empty() {
        t!("output-any-rate")
    } else {
        d.rates
            .iter()
            .map(|r| {
                format!("{:.1}", *r as f64 / 1000.0)
                    .trim_end_matches(".0")
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(", ")
            + " kHz" // i18n: ignore (unit)
    };
    rsx! {
        div { class: "setting",
            Icon { name: "volume_up", size: 22 }
            div { class: "row-text",
                div { class: "title", {t!("output-playing-through", name = d.name.clone())} }
                div { class: "subtitle", {t!("output-rates", rates = rates)} }
                if !stream.is_empty() {
                    div { class: "subtitle", {t!("output-stream", format = stream)} }
                }
            }
        }
    }
}

#[component]
fn ReliabilityRow() -> Element {
    rsx! {
        button {
            class: "setting nav-row",
            onclick: move |_| {
                let mut page = use_context::<SettingsNav>().0;
                page.set(SettingsPage::Reliability);
            },
            Icon { name: "play", size: 22 }
            div { class: "row-text",
                div { class: "title", {t!("reliability-title")} }
                div { class: "subtitle", {t!("settings-reliability-desc")} }
            }
            Icon { name: "chevron_right" }
        }
    }
}

#[component]
fn MusicFolders(cur: Settings, on_saved: EventHandler<()>) -> Element {
    let st = state();
    let svc = services::get();
    let mut new_folder = use_signal(String::new);
    let save = move |f: Box<dyn FnOnce(&mut loss_core::Settings)>| {
        services::get().update_settings(f);
        on_saved.call(());
        services::get().rescan();
    };
    let suggestions: Vec<PathBuf> = svc
        .paths
        .music_suggestions
        .iter()
        .filter(|p| !cur.music_folders.contains(p))
        .cloned()
        .collect();
    rsx! {
        for f in cur.music_folders.clone() {
            div { class: "setting",
                div { class: "row-text", div { class: "title mono", "{f.display()}" } }
                button {
                    class: "icon-btn small",
                    "aria-label": t!("settings-remove-folder"),
                    onclick: {
                        let f = f.clone();
                        move |_| {
                            let f = f.clone();
                            let at = services::get()
                                .settings()
                                .music_folders
                                .iter()
                                .position(|x| x == &f)
                                .unwrap_or(0);
                            let gone = f.clone();
                            save(Box::new(move |s| s.music_folders.retain(|x| x != &gone)));
                            // Undo puts it back where it was.
                            st.toast_undo(t!("toast-folder-removed"), move || {
                                let f = f.clone();
                                services::get().update_settings(move |s| {
                                    if !s.music_folders.contains(&f) {
                                        let i = at.min(s.music_folders.len());
                                        s.music_folders.insert(i, f);
                                    }
                                });
                                on_saved.call(());
                                services::get().rescan();
                            });
                        }
                    },
                    Icon { name: "delete", size: 20 }
                }
            }
        }
        for sug in suggestions {
            button {
                class: "pill",
                onclick: {
                    move |_| {
                        let sug = sug.clone();
                        save(Box::new(move |s| s.music_folders.push(sug)));
                    }
                },
                "+ {sug.display()}"
            }
        }
        form {
            class: "inline-form",
            onsubmit: {
                move |e: FormEvent| {
                    e.prevent_default();
                    let p = PathBuf::from(new_folder().trim());
                    if !p.as_os_str().is_empty() {
                        save(Box::new(move |s| {
                            if !s.music_folders.contains(&p) {
                                s.music_folders.push(p)
                            }
                        }));
                        new_folder.set(String::new());
                        document::eval("document.getElementById('new-folder').value = ''");
                    }
                }
            },
            input {
                id: "new-folder",
                placeholder: "/storage/emulated/0/Music/…", // i18n: ignore
                initial_value: "{new_folder}",
                oninput: move |e| new_folder.set(e.value()),
            }
            button { class: "pill", r#type: "submit", {t!("action-add")} }
        }
        button {
            class: "primary",
            disabled: (st.scanning)().is_some(),
            onclick: move |_| services::get().rescan(),
            Icon { name: "refresh", size: 18 }
            if (st.scanning)().is_some() { " " {t!("settings-scanning")} } else { " " {t!("settings-rescan")} }
        }
        if !crate::platform::has_audio_permission() {
            button { class: "pill", onclick: move |_| crate::platform::request_audio_permission(), {t!("permission-allow-music")} }
            button { class: "pill", onclick: move |_| crate::platform::open_app_settings(), {t!("permission-open-settings")} }
        }
    }
}

#[component]
fn SubsonicSettings(current: SubsonicConfig, on_saved: EventHandler<()>) -> Element {
    let st = state();
    let mut url = use_signal(|| current.url.clone());
    let mut user = use_signal(|| current.username.clone());
    let mut pass = use_signal(|| current.password.clone());
    let mut status: Signal<Option<(bool, String)>> = use_signal(|| None);
    let mut testing = use_signal(|| false);
    rsx! {
        div { class: "card",
            div { class: "title", {t!("setting-subsonic")} }
            input { placeholder: "https://music.example.com", initial_value: "{url}", oninput: move |e| url.set(e.value()) }
            input { placeholder: t!("server-username"), initial_value: "{user}", oninput: move |e| user.set(e.value()) }
            input { r#type: "password", placeholder: t!("server-password"), initial_value: "{pass}", oninput: move |e| pass.set(e.value()) }
            div { class: "hero-actions",
                button {
                    class: "primary",
                    disabled: testing(),
                    onclick: move |_| {
                        let cfg = SubsonicConfig { url: url().trim().trim_end_matches('/').to_string(), username: user().trim().to_string(), password: pass() };
                        if cfg.url.is_empty() {
                            services::get().update_settings(|s| s.subsonic = None);
                            on_saved.call(());
                            status.set(Some((true, t!("server-removed"))));
                            return;
                        }
                        testing.set(true);
                        let client = services::get().downloads.client();
                        spawn(async move {
                            let c2 = cfg.clone();
                            let r = services::get().downloads.runtime().spawn(async move { loss_download::sources::subsonic::ping(&client, &c2).await }).await;
                            testing.set(false);
                            match r {
                                Ok(Ok(())) => {
                                    services::get().update_settings(move |s| s.subsonic = Some(cfg));
                                    on_saved.call(());
                                    status.set(Some((true, t!("server-saved"))));
                                    st.toast(t!("toast-server-connected"));
                                }
                                Ok(Err(e)) => status.set(Some((false, e.to_string()))),
                                Err(e) => status.set(Some((false, e.to_string()))),
                            }
                        });
                    },
                    if testing() { {t!("server-testing")} } else { {t!("server-test-save")} }
                }
            }
            if let Some((ok, msg)) = status() {
                p { class: if ok { "small ok" } else { "small error" }, "{msg}" }
            }
            p { class: "muted small", {t!("server-password-note")} }
        }
    }
}

#[component]
fn PlaylistImport() -> Element {
    let st = state();
    let mut text = use_signal(String::new);
    let mut result: Signal<Option<Vec<playlist::MatchResult>>> = use_signal(|| None);
    let res = result();
    rsx! {
        div { class: "card",
            p { class: "muted small", {t!("import-hint")} }
            textarea { rows: "4", initial_value: "{text}", placeholder: "#EXTM3U …  or  Track Name,Artist Name(s)…", oninput: move |e| text.set(e.value()) }
            button {
                class: "pill",
                onclick: move |_| {
                    let entries = playlist::parse(&text());
                    let lib = services::get().library();
                    result.set(Some(playlist::match_library(entries, &lib)));
                },
                {t!("import-match")}
            }
            if let Some(r) = res {
                {
                    let found: Vec<String> = r.iter().filter_map(|m| m.track_id.clone()).collect();
                    let missing: Vec<String> = r.iter().filter(|m| m.track_id.is_none()).map(|m| {
                        if m.entry.artist.is_empty() { m.entry.title.clone() } else { format!("{} — {}", m.entry.artist, m.entry.title) }
                    }).collect();
                    let n_found = found.len();
                    rsx! {
                        p { class: "small", {t!("import-owned", owned = n_found, total = r.len())} }
                        if n_found > 0 {
                            button {
                                class: "primary",
                                onclick: move |_| {
                                    super::play_tracks(&found, 0);
                                    st.toast(t!("toast-playing-matched"));
                                },
                                Icon { name: "play", size: 18 }
                                " " {t!("import-play-owned", count = n_found)}
                            }
                        }
                        if !missing.is_empty() {
                            div { class: "subtitle", {t!("import-missing")} }
                            for (i , m) in missing.iter().take(50).enumerate() {
                                div { key: "{i}", class: "small muted", "• {m}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
