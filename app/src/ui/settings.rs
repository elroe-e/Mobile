//! Settings: folders, downloads, sources, playlist import, stats, about.

use super::icons::Icon;
use super::state;
use crate::services;
use dioxus::prelude::*;
use loss_core::{format_duration, playlist, SubsonicConfig};
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

#[component]
pub fn SettingsTab() -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let mut s = use_signal(|| svc.settings());
    let mut new_folder = use_signal(String::new);
    let save = move |f: Box<dyn FnOnce(&mut loss_core::Settings)>| {
        services::get().update_settings(f);
        s.set(services::get().settings());
    };
    let cur = s();
    let suggestions: Vec<PathBuf> = svc
        .paths
        .music_suggestions
        .iter()
        .filter(|p| !cur.music_folders.contains(p))
        .cloned()
        .collect();

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

        h2 { class: "section-title", "Music folders" }
        div { class: "card",
            for f in cur.music_folders.clone() {
                div { class: "setting",
                    div { class: "row-text", div { class: "title mono", "{f.display()}" } }
                    button {
                        class: "icon-btn small",
                        onclick: {
                            let f = f.clone();
                            let mut save = save;
                            move |_| {
                                let f = f.clone();
                                save(Box::new(move |s| s.music_folders.retain(|x| x != &f)));
                                services::get().rescan();
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
                        let mut save = save;
                        move |_| {
                            let sug = sug.clone();
                            save(Box::new(move |s| s.music_folders.push(sug)));
                            services::get().rescan();
                        }
                    },
                    "+ {sug.display()}"
                }
            }
            form {
                class: "inline-form",
                onsubmit: {
                    let mut save = save;
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
                            services::get().rescan();
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

        h2 { class: "section-title", "Downloads" }
        div { class: "card",
            div { class: "setting col",
                div { class: "title", "Download folder" }
                input {
                    class: "mono",
                    initial_value: "{cur.download_dir.display()}",
                    onchange: {
                        let mut save = save;
                        move |e: FormEvent| {
                            let p = PathBuf::from(e.value().trim());
                            save(Box::new(move |s| s.download_dir = p));
                        }
                    },
                }
                div { class: "subtitle", "The default needs no storage permission. Files there are removed if you uninstall Loss." }
            }
            div { class: "setting",
                div { class: "row-text", div { class: "title", "Downloads at once" } }
                div { class: "stepper",
                    for n in 1..=5usize {
                        button {
                            class: if cur.max_parallel_downloads == n { "seg active" } else { "seg" },
                            onclick: {
                                let mut save = save;
                                move |_| save(Box::new(move |s| s.max_parallel_downloads = n))
                            },
                            "{n}"
                        }
                    }
                }
            }
            Toggle {
                label: "Tag downloads",
                hint: "Add title, artist, album and cover art when missing",
                on: cur.auto_tag,
                onchange: { let mut save = save; move |v| save(Box::new(move |s| s.auto_tag = v)) },
            }
            Toggle {
                label: "Fetch lyrics",
                hint: "Save synced lyrics from LRCLIB next to each download",
                on: cur.fetch_lyrics,
                onchange: { let mut save = save; move |v| save(Box::new(move |s| s.fetch_lyrics = v)) },
            }
            Toggle {
                label: "Unpack zip albums",
                hint: "Extract songs from .zip downloads (e.g. Bandcamp purchases)",
                on: cur.extract_zips,
                onchange: { let mut save = save; move |v| save(Box::new(move |s| s.extract_zips = v)) },
            }
        }

        h2 { class: "section-title", "Sources" }
        div { class: "card",
            Toggle {
                label: "Internet Archive: Creative Commons only",
                hint: "Hide items without an open licence",
                on: cur.archive_cc_only,
                onchange: { let mut save = save; move |v| save(Box::new(move |s| s.archive_cc_only = v)) },
            }
            div { class: "setting col",
                div { class: "title", "Jamendo client id" }
                input {
                    placeholder: "Free from devportal.jamendo.com",
                    initial_value: "{cur.jamendo_client_id}",
                    onchange: { let mut save = save; move |e: FormEvent| { let v = e.value(); save(Box::new(move |s| s.jamendo_client_id = v.trim().to_string())) } },
                }
            }
        }
        SubsonicSettings { current: cur.subsonic.clone().unwrap_or_default(), on_saved: move |_| s.set(services::get().settings()) }

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

        h2 { class: "section-title", "Look" }
        div { class: "card",
            div { class: "setting",
                div { class: "row-text", div { class: "title", "Theme" } }
                div { class: "stepper",
                    for (mode , label) in [("system", "Auto"), ("dark", "Dark"), ("light", "Light")] {
                        button {
                            class: if cur.theme == mode { "seg active" } else { "seg" },
                            onclick: {
                                let mut save = save;
                                move |_| {
                                    save(Box::new(move |s| s.theme = mode.to_string()));
                                    st.settings_changed();
                                }
                            },
                            "{label}"
                        }
                    }
                }
            }
            Toggle {
                label: "Colours from album art",
                hint: "The whole app takes on the colours of what's playing",
                on: cur.dynamic_color,
                onchange: {
                    let mut save = save;
                    move |v| {
                        save(Box::new(move |s| s.dynamic_color = v));
                        st.settings_changed();
                    }
                },
            }
            div { class: "setting col",
                div { class: "title", if cur.dynamic_color { "Colour when nothing is playing" } else { "Accent colour" } }
                div { class: "swatches",
                    for c in ["#e8615a", "#4fb3a9", "#7c8cff", "#e3a83b", "#c56bd8", "#5fbf5f"] {
                        button {
                            class: if cur.accent == c { "swatch active" } else { "swatch" },
                            style: "background: {c}",
                            onclick: {
                                let mut save = save;
                                move |_| {
                                    save(Box::new(move |s| s.accent = c.to_string()));
                                    st.settings_changed();
                                }
                            },
                        }
                    }
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
        div { style: "height: 24px" }
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
