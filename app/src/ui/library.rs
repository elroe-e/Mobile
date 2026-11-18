//! Library: songs, albums and artists from your folders and downloads.

use super::icons::Icon;
use super::{play_tracks, queue_items, shuffle_tracks, state, Cover, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_audio::Command;
use loss_core::{format_duration, Track};

#[derive(Clone, PartialEq, Debug)]
enum View {
    Main,
    Album(String),
    Artist(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Seg {
    Songs,
    Albums,
    Artists,
}

#[component]
pub fn LibraryTab() -> Element {
    let view = use_signal(|| View::Main);
    match view() {
        View::Main => rsx! { LibraryMain { view } },
        View::Album(key) => rsx! { AlbumPage { key: "{key}", album_key: key.clone(), view } },
        View::Artist(name) => rsx! { ArtistPage { key: "{name}", name: name.clone(), view } },
    }
}

#[component]
fn LibraryMain(view: Signal<View>) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let mut seg = use_signal(|| Seg::Albums);
    let mut query = use_signal(String::new);
    let mut limit = use_signal(|| 150usize);
    let svc = services::get();
    let lib = svc.library();
    let total = lib.len();
    let total_secs: f64 = lib.tracks.iter().map(|t| t.duration_secs).sum();
    let q = query();
    let searching = !q.trim().is_empty();
    let scanning = (st.scanning)();

    let songs: Vec<Track> = if seg() == Seg::Songs || searching {
        lib.search(&q).into_iter().cloned().collect()
    } else {
        vec![]
    };
    let albums = if seg() == Seg::Albums && !searching { lib.albums() } else { vec![] };
    let artists = if seg() == Seg::Artists && !searching { lib.artists() } else { vec![] };
    let recent: Vec<Track> = if seg() == Seg::Albums && !searching {
        let mut seen = std::collections::HashSet::new();
        lib.recently_added(200)
            .into_iter()
            .filter(|t| seen.insert(t.album_key()))
            .take(10)
            .cloned()
            .collect()
    } else {
        vec![]
    };
    drop(lib);

    let song_ids: Vec<String> = songs.iter().map(|t| t.id.clone()).collect();
    let shown = songs.len().min(limit());

    rsx! {
        header { class: "page-header",
            h1 { "Library" }
            div { class: "header-actions",
                button {
                    class: "pill",
                    disabled: total == 0,
                    onclick: move |_| {
                        let ids: Vec<String> = services::get().library().tracks.iter().map(|t| t.id.clone()).collect();
                        shuffle_tracks(&ids);
                    },
                    Icon { name: "shuffle", size: 18 }
                    " Shuffle all"
                }
            }
        }
        div { class: "search-box",
            Icon { name: "search", size: 20 }
            input {
                r#type: "search",
                placeholder: "Search your music",
                initial_value: "{q}",
                oninput: move |e| query.set(e.value()),
            }
        }
        if let Some(n) = scanning {
            div { class: "info-bar", span { class: "spinner" } "Scanning your music… {n} songs found" }
        }
        if total == 0 && scanning.is_none() {
            EmptyLibrary {}
        } else {
            if !searching {
                div { class: "segmented",
                    for (s , label) in [(Seg::Albums, "Albums"), (Seg::Songs, "Songs"), (Seg::Artists, "Artists")] {
                        button {
                            class: if seg() == s { "seg active" } else { "seg" },
                            onclick: move |_| seg.set(s),
                            "{label}"
                        }
                    }
                }
                p { class: "muted small pad", "{total} songs · {format_duration(total_secs)}" }
            }
            if !recent.is_empty() {
                h2 { class: "section-title", "Recently added" }
                div { class: "h-scroll",
                    for t in recent.iter() {
                        div {
                            key: "{t.id}",
                            class: "h-card",
                            onclick: {
                                let key = t.album_key();
                                move |_| view.clone().set(View::Album(key.clone()))
                            },
                            Cover { track_id: t.id.clone(), class: "cover card-cover" }
                            div { class: "title", "{t.album}" }
                            div { class: "subtitle", "{t.album_artist}" }
                        }
                    }
                }
                h2 { class: "section-title", "All albums" }
            }
            if !albums.is_empty() {
                div { class: "album-grid",
                    for a in albums.iter() {
                        div {
                            key: "{a.key}",
                            class: "album-card",
                            onclick: {
                                let key = a.key.clone();
                                move |_| view.clone().set(View::Album(key.clone()))
                            },
                            Cover { track_id: a.cover_track.clone().unwrap_or_default(), class: "cover card-cover" }
                            div { class: "title", "{a.title}" }
                            div { class: "subtitle", "{a.artist}" }
                        }
                    }
                }
            }
            if !artists.is_empty() {
                div { class: "list",
                    for a in artists.iter() {
                        div {
                            key: "{a.name}",
                            class: "row",
                            onclick: {
                                let name = a.name.clone();
                                move |_| view.clone().set(View::Artist(name.clone()))
                            },
                            div { class: "avatar", "{a.name.chars().next().unwrap_or('?')}" }
                            div { class: "row-text",
                                div { class: "title", "{a.name}" }
                                div { class: "subtitle", "{a.album_keys.len()} albums · {a.track_count} songs" }
                            }
                            Icon { name: "chevron_right" }
                        }
                    }
                }
            }
            if seg() == Seg::Songs || searching {
                if songs.is_empty() {
                    p { class: "muted pad", "No songs match “{q}”." }
                }
                div { class: "list",
                    for (i , t) in songs.iter().take(shown).enumerate() {
                        SongRow { key: "{t.id}", track: t.clone(), ids: song_ids.clone(), index: i, show_cover: true }
                    }
                }
                if songs.len() > shown {
                    button { class: "pill center", onclick: move |_| limit.set(limit() + 300), "Show more ({songs.len() - shown})" }
                }
            }
        }
    }
}

#[component]
fn EmptyLibrary() -> Element {
    let mut st = state();
    let has_perm = crate::platform::has_audio_permission();
    rsx! {
        div { class: "empty",
            Icon { name: "library", size: 64 }
            h2 { "No music yet" }
            p { "Loss plays the music on your phone and anything you download." }
            if !has_perm {
                button {
                    class: "primary",
                    onclick: move |_| {
                        std::thread::spawn(|| {
                            crate::platform::request_audio_permission();
                            std::thread::sleep(std::time::Duration::from_secs(4));
                            services::get().rescan();
                        });
                    },
                    "Allow access to music"
                }
                button { class: "pill", onclick: move |_| crate::platform::open_app_settings(), "Open app settings" }
            }
            button { class: "primary", onclick: move |_| services::get().rescan(), Icon { name: "refresh", size: 18 } " Scan again" }
            button { class: "pill", onclick: move |_| st.tab.set(Tab::Settings), "Choose music folders" }
            button { class: "pill", onclick: move |_| st.tab.set(Tab::Discover), "Find free music to download" }
        }
    }
}

#[component]
pub fn SongRow(track: Track, ids: Vec<String>, index: usize, #[props(default = false)] show_cover: bool) -> Element {
    let st = state();
    let playing_id = st.player.read().current().map(|c| c.id.clone());
    let is_current = playing_id.as_deref() == Some(track.id.as_str());
    let mut menu = use_signal(|| false);
    let id = track.id.clone();
    rsx! {
        div { class: if is_current { "row current" } else { "row" },
            div {
                class: "row-main",
                onclick: move |_| play_tracks(&ids, index),
                if show_cover {
                    Cover { track_id: track.id.clone(), class: "cover row-cover" }
                } else {
                    div { class: "track-no", "{track.track_no.map(|n| n.to_string()).unwrap_or_default()}" }
                }
                div { class: "row-text",
                    div { class: "title", "{track.title}" }
                    div { class: "subtitle",
                        if track.origin == loss_core::TrackOrigin::Downloaded {
                            span { class: "dl-dot", title: "Downloaded" }
                        }
                        "{track.artist}"
                        if show_cover { " · {track.album}" }
                    }
                }
                div { class: "muted small", "{format_duration(track.duration_secs)}" }
            }
            button { class: "icon-btn small", onclick: move |_| menu.set(!menu()), Icon { name: "add", size: 20 } }
            if menu() {
                div { class: "row-menu",
                    button {
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                services::get().player.send(Command::PlayNext(queue_items(&[id.clone()])));
                                menu.set(false);
                                st.toast("Playing next");
                            }
                        },
                        Icon { name: "play_next", size: 18 }
                        " Play next"
                    }
                    button {
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                services::get().player.send(Command::Enqueue(queue_items(&[id.clone()])));
                                menu.set(false);
                                st.toast("Added to queue");
                            }
                        },
                        Icon { name: "queue", size: 18 }
                        " Add to queue"
                    }
                }
            }
        }
    }
}

#[component]
fn AlbumPage(album_key: String, view: Signal<View>) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let lib = svc.library();
    let Some(album) = lib.album(&album_key) else {
        drop(lib);
        return rsx! {
            button { class: "back", onclick: move |_| view.set(View::Main), Icon { name: "back" } " Library" }
            p { class: "pad muted", "This album is no longer in your library." }
        };
    };
    let tracks: Vec<Track> = album.tracks.iter().filter_map(|id| lib.get(id).cloned()).collect();
    drop(lib);
    let ids = album.tracks.clone();
    let ids2 = ids.clone();
    let ids3 = ids.clone();
    let artist = album.artist.clone();
    let year = album.year.map(|y| format!(" · {y}")).unwrap_or_default();
    rsx! {
        button { class: "back", onclick: move |_| view.set(View::Main), Icon { name: "back" } " Library" }
        div { class: "album-hero",
            Cover { track_id: album.cover_track.clone().unwrap_or_default(), class: "cover hero-cover", big: true }
            h1 { "{album.title}" }
            button { class: "link", onclick: move |_| view.set(View::Artist(artist.clone())), "{album.artist}" }
            p { class: "muted small", "{tracks.len()} songs · {format_duration(album.duration_secs)}{year}" }
            div { class: "hero-actions",
                button { class: "primary", onclick: move |_| play_tracks(&ids, 0), Icon { name: "play", size: 20 } " Play" }
                button {
                    class: "pill",
                    onclick: move |_| shuffle_tracks(&ids2),
                    Icon { name: "shuffle", size: 18 }
                    " Shuffle"
                }
                button {
                    class: "pill",
                    onclick: move |_| {
                        services::get().player.send(Command::Enqueue(queue_items(&ids3)));
                        st.toast("Album added to queue");
                    },
                    Icon { name: "queue", size: 18 }
                }
            }
        }
        div { class: "list",
            for (i , t) in tracks.iter().enumerate() {
                SongRow { key: "{t.id}", track: t.clone(), ids: album.tracks.clone(), index: i }
            }
        }
    }
}

#[component]
fn ArtistPage(name: String, view: Signal<View>) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let lib = svc.library();
    let norm = loss_core::normalize(&name);
    let albums: Vec<_> = lib
        .albums()
        .into_iter()
        .filter(|a| loss_core::normalize(&a.artist) == norm)
        .collect();
    let songs: Vec<Track> = lib
        .tracks
        .iter()
        .filter(|t| loss_core::normalize(&t.artist) == norm || loss_core::normalize(&t.album_artist) == norm)
        .cloned()
        .collect();
    drop(lib);
    let ids: Vec<String> = songs.iter().map(|t| t.id.clone()).collect();
    let ids_play = ids.clone();
    rsx! {
        button { class: "back", onclick: move |_| view.set(View::Main), Icon { name: "back" } " Library" }
        header { class: "page-header",
            h1 { "{name}" }
            button { class: "primary", onclick: move |_| play_tracks(&ids_play, 0), Icon { name: "play", size: 20 } " Play" }
        }
        p { class: "muted small pad", "{albums.len()} albums · {songs.len()} songs" }
        div { class: "album-grid",
            for a in albums.iter() {
                div {
                    key: "{a.key}",
                    class: "album-card",
                    onclick: {
                        let key = a.key.clone();
                        move |_| view.set(View::Album(key.clone()))
                    },
                    Cover { track_id: a.cover_track.clone().unwrap_or_default(), class: "cover card-cover" }
                    div { class: "title", "{a.title}" }
                    div { class: "subtitle", "{a.year.map(|y| y.to_string()).unwrap_or_default()}" }
                }
            }
        }
        h2 { class: "section-title", "Songs" }
        div { class: "list",
            for (i , t) in songs.iter().enumerate() {
                SongRow { key: "{t.id}", track: t.clone(), ids: ids.clone(), index: i, show_cover: true }
            }
        }
    }
}
