//! Settings, built from the registry (`loss_core::registry`), with search. Plus playlist
//! import, listening stats, logs and about.

use super::icons::Icon;
use super::state;
use crate::services;
use dioxus::prelude::*;
use loss_core::registry::{self, Kind, Section, Value};
use loss_core::{format_duration, playlist, Settings, SubsonicConfig};
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
    }
}

/// Applies side effects a setting needs beyond being saved.
fn after_change(key: &str, st: super::AppState) {
    match key {
        "theme" | "dynamic_color" | "accent" => st.settings_changed(),
        "detailed_logging" => {
            crate::diag::set_detailed(services::get().settings().detailed_logging)
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
    let title = d.title;
    let hint = d.description;
    // Buttons go under the title when they wouldn't leave it room on a phone.
    let options = match &d.kind {
        Kind::Choice(o) => o.len(),
        Kind::Steps(o) => o.len(),
        _ => 0,
    };
    let stacked = title.chars().count() + options * 4 > 30 || !hint.is_empty();
    match (&d.kind, value) {
        (Kind::Toggle, Some(Value::Bool(on))) => rsx! {
            Toggle { label: title, hint, on, onchange: move |v| on_set.call((key, Value::Bool(v))) }
        },
        (Kind::Choice(opts), Some(Value::Text(v))) => rsx! {
            div { class: if stacked { "setting col" } else { "setting" },
                div { class: "row-text",
                    div { class: "title", "{title}" }
                    if !hint.is_empty() { div { class: "subtitle", "{hint}" } }
                }
                div { class: "stepper",
                    for (opt , label) in opts.iter().copied() {
                        button {
                            class: if v == opt { "seg active" } else { "seg" },
                            onclick: move |_| on_set.call((key, Value::Text(opt.to_string()))),
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
                            onclick: move |_| on_set.call((key, Value::Int(n))),
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
                            onclick: move |_| on_set.call((key, Value::Text(c.to_string()))),
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
        header { class: "page-header", h1 { "Settings" } }
        div { class: "search-box settings-search",
            Icon { name: "search", size: 20 }
            input {
                r#type: "search",
                placeholder: "Search settings",
                initial_value: "{q}",
                oninput: move |e| query.set(e.value()),
            }
        }

        if !q.trim().is_empty() {
            if results.is_empty() {
                p { class: "muted empty-note", "No settings match “{q.trim()}”." }
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
                    h2 { class: "section-title", "Import a playlist" }
                    PlaylistImport {}

                    h2 { class: "section-title", "Your listening" }
                    div { class: "card",
                        div { class: "stats",
                            div { div { class: "big", "{count}" } div { class: "muted small", "songs" } }
                            div { div { class: "big", "{format_duration(secs)}" } div { class: "muted small", "of music" } }
                            div { div { class: "big", "{plays}" } div { class: "muted small", "plays" } }
                        }
                        if !top.is_empty() {
                            div { class: "subtitle", "Most played" }
                            for (i , (t , a , n)) in top.iter().enumerate() {
                                div { key: "{i}", class: "small", "{i + 1}. {t} — {a} ({n})" }
                            }
                        }
                        p { class: "muted small", "Statistics stay on your phone." }
                    }
                }
            }

            h2 { class: "section-title", "About" }
            div { class: "card",
                p { "Loss {env!(\"CARGO_PKG_VERSION\")} — a music player and downloader written in Rust." }
                p { class: "muted small",
                    "Built with Dioxus, Symphonia, cpal, lofty and reqwest. Lyrics from LRCLIB. "
                    "Loss downloads only from sources that allow it and never removes copy protection."
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
            if section == Section::Help {
                LogsRow {}
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
                div { class: "title", "Logs and diagnostics" }
                div { class: "subtitle", "See what the app is doing, copy details for a bug report, check your audio outputs" }
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
                    "aria-label": "Remove folder",
                    onclick: {
                        let f = f.clone();
                        move |_| {
                            let f = f.clone();
                            save(Box::new(move |s| s.music_folders.retain(|x| x != &f)));
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
                placeholder: "/storage/emulated/0/Music/…",
                initial_value: "{new_folder}",
                oninput: move |e| new_folder.set(e.value()),
            }
            button { class: "pill", r#type: "submit", "Add" }
        }
        button {
            class: "primary",
            disabled: (st.scanning)().is_some(),
            onclick: move |_| services::get().rescan(),
            Icon { name: "refresh", size: 18 }
            if (st.scanning)().is_some() { " Scanning…" } else { " Rescan library" }
        }
        if !crate::platform::has_audio_permission() {
            button { class: "pill", onclick: move |_| crate::platform::request_audio_permission(), "Allow access to music" }
            button { class: "pill", onclick: move |_| crate::platform::open_app_settings(), "Open app settings" }
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
            div { class: "title", "My server (Subsonic / Navidrome)" }
            input { placeholder: "https://music.example.com", initial_value: "{url}", oninput: move |e| url.set(e.value()) }
            input { placeholder: "Username", initial_value: "{user}", oninput: move |e| user.set(e.value()) }
            input { r#type: "password", placeholder: "Password", initial_value: "{pass}", oninput: move |e| pass.set(e.value()) }
            div { class: "hero-actions",
                button {
                    class: "primary",
                    disabled: testing(),
                    onclick: move |_| {
                        let cfg = SubsonicConfig { url: url().trim().trim_end_matches('/').to_string(), username: user().trim().to_string(), password: pass() };
                        if cfg.url.is_empty() {
                            services::get().update_settings(|s| s.subsonic = None);
                            on_saved.call(());
                            status.set(Some((true, "Server removed".into())));
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
                                    status.set(Some((true, "Connected and saved".into())));
                                    st.toast("Server connected");
                                }
                                Ok(Err(e)) => status.set(Some((false, e.to_string()))),
                                Err(e) => status.set(Some((false, e.to_string()))),
                            }
                        });
                    },
                    if testing() { "Testing…" } else { "Test & save" }
                }
            }
            if let Some((ok, msg)) = status() {
                p { class: if ok { "small ok" } else { "small error" }, "{msg}" }
            }
            p { class: "muted small", "Your password is kept in the app's private storage and only sent as a salted token." }
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
            p { class: "muted small", "Paste an M3U playlist or a CSV export (with Track Name / Artist columns) to see which songs you already have." }
            textarea { rows: "4", initial_value: "{text}", placeholder: "#EXTM3U …  or  Track Name,Artist Name(s)…", oninput: move |e| text.set(e.value()) }
            button {
                class: "pill",
                onclick: move |_| {
                    let entries = playlist::parse(&text());
                    let lib = services::get().library();
                    result.set(Some(playlist::match_library(entries, &lib)));
                },
                "Match with my library"
            }
            if let Some(r) = res {
                {
                    let found: Vec<String> = r.iter().filter_map(|m| m.track_id.clone()).collect();
                    let missing: Vec<String> = r.iter().filter(|m| m.track_id.is_none()).map(|m| {
                        if m.entry.artist.is_empty() { m.entry.title.clone() } else { format!("{} — {}", m.entry.artist, m.entry.title) }
                    }).collect();
                    let n_found = found.len();
                    rsx! {
                        p { class: "small", "You own {n_found} of {r.len()} songs." }
                        if n_found > 0 {
                            button {
                                class: "primary",
                                onclick: move |_| {
                                    super::play_tracks(&found, 0);
                                    st.toast("Playing matched songs");
                                },
                                Icon { name: "play", size: 18 }
                                " Play the {n_found} songs you own"
                            }
                        }
                        if !missing.is_empty() {
                            div { class: "subtitle", "Missing (buy them or look for them in Discover):" }
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
