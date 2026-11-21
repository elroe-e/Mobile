//! Library: songs, albums and artists from your folders and downloads.

use super::icons::Icon;
use super::{play_tracks, queue_items, shuffle_tracks, state, CookieButton, Cover, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_audio::Command;
use loss_core::{format_duration, Album, Track};

#[derive(Clone, PartialEq, Debug)]
pub enum View {
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sort {
    Latest,
    Title,
    Artist,
}

impl Sort {
    fn next(self) -> Sort {
        match self {
            Sort::Latest => Sort::Title,
            Sort::Title => Sort::Artist,
            Sort::Artist => Sort::Latest,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Sort::Latest => "Latest",
            Sort::Title => "A–Z",
            Sort::Artist => "Artist",
        }
    }
}

/// Which library page is open, plus the pages before it. Kept by the App so
/// switching tabs and back returns to the same page, and so Android's Back
/// button can step back through album and artist pages.
#[derive(Clone, Copy, PartialEq)]
pub struct LibNav {
    pub view: Signal<View>,
    history: Signal<Vec<View>>,
}

impl LibNav {
    /// Open a page, remembering the current one for Back.
    pub fn go(mut self, v: View) {
        let current = self.view.peek().clone();
        if current != v {
            self.history.write().push(current);
            self.view.set(v);
        }
    }

    /// Go to the previous page. Returns false when already on the main page.
    pub fn back(mut self) -> bool {
        if *self.view.peek() == View::Main {
            return false;
        }
        let prev = self.history.write().pop().unwrap_or(View::Main);
        self.view.set(prev);
        true
    }
}

pub fn use_provide_nav() -> LibNav {
    use_context_provider(|| LibNav {
        view: Signal::new(View::Main),
        history: Signal::new(Vec::new()),
    })
}

#[component]
pub fn LibraryTab() -> Element {
    let nav = use_context::<LibNav>();
    let current = (nav.view)();
    match current {
        View::Main => rsx! { LibraryMain { nav } },
        View::Album(key) => rsx! { AlbumPage { key: "{key}", album_key: key.clone(), nav } },
        View::Artist(name) => rsx! { ArtistPage { key: "{name}", name: name.clone(), nav } },
    }
}

#[component]
fn LibraryMain(nav: LibNav) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let mut seg = use_signal(|| Seg::Albums);
    let mut sort = use_signal(|| Sort::Latest);
    let mut query = use_signal(String::new);
    let mut limit = use_signal(|| 150usize);
    let svc = services::get();
    let lib = svc.library();
    let total = lib.len();
    let q = query();
    let searching = !q.trim().is_empty();
    let scanning = (st.scanning)();

    let mut songs: Vec<Track> = lib.search(&q).into_iter().cloned().collect();
    let mut albums: Vec<Album> = lib.albums();
    let artists = lib.artists();
    let (n_songs, n_albums, n_artists) = (songs.len(), albums.len(), artists.len());
    match sort() {
        Sort::Latest => {
            songs.sort_by_key(|t| std::cmp::Reverse(t.added_at));
            let added: std::collections::HashMap<String, u64> = lib
                .tracks
                .iter()
                .map(|t| (t.album_key(), t.added_at))
                .collect();
            albums.sort_by(|a, b| added.get(&b.key).cmp(&added.get(&a.key)));
        }
        Sort::Title => {
            songs.sort_by_key(|t| loss_core::normalize(&t.title));
            albums.sort_by_key(|a| loss_core::normalize(&a.title));
        }
        Sort::Artist => {
            songs.sort_by_key(|t| loss_core::normalize(&t.artist));
            albums.sort_by_key(|a| loss_core::normalize(&a.artist));
        }
    }
    // Badge + first track per album.
    let album_info: Vec<(Album, String)> = albums
        .into_iter()
        .map(|a| {
            let badge = a
                .tracks
                .first()
                .and_then(|id| lib.get(id))
                .map(|t| t.quality_badge())
                .unwrap_or_default();
            (a, badge)
        })
        .collect();
    drop(lib);

    let song_ids: Vec<String> = songs.iter().map(|t| t.id.clone()).collect();
    let shown = songs.len().min(limit());
    let active = if searching { Seg::Songs } else { seg() };
    let count_line = match active {
        Seg::Songs => format!("{} songs", songs.len()),
        Seg::Albums => format!("{n_albums} albums"),
        Seg::Artists => format!("{n_artists} artists"),
    };

    rsx! {
        header { class: "page-header",
            h1 { "Library" }
            button {
                class: "icon-btn tonal",
                disabled: total == 0,
                onclick: move |_| {
                    let ids: Vec<String> = services::get().library().tracks.iter().map(|t| t.id.clone()).collect();
                    shuffle_tracks(&ids);
                },
                Icon { name: "shuffle" }
            }
        }
        div { class: "search-box",
            Icon { name: "search", size: 22 }
            input {
                r#type: "search",
                placeholder: "Search your library",
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
            div { class: "chips",
                for (s , icon , label , n) in [(Seg::Songs, "note", "Songs", n_songs), (Seg::Albums, "album", "Albums", n_albums), (Seg::Artists, "person", "Artists", n_artists)] {
                    button {
                        class: if active == s { "chip selected" } else { "chip" },
                        onclick: move |_| seg.set(s),
                        Icon { name: if active == s { "check" } else { icon }, size: 18 }
                        "{label}"
                        span { class: "chip-count", "{n}" }
                    }
                }
            }
            div { class: "toolbar",
                span { class: "muted small", "{count_line}" }
                if active != Seg::Artists {
                    button { class: "text-btn", onclick: move |_| sort.set(sort().next()),
                        Icon { name: "sort", size: 18 }
                        "{sort().label()}"
                    }
                }
            }
            if active == Seg::Albums {
                div { class: "album-grid",
                    for (a , badge) in album_info.iter() {
                        AlbumCard { key: "{a.key}", album: a.clone(), badge: badge.clone(), nav }
                    }
                }
            }
            if active == Seg::Artists {
                div { class: "list",
                    for a in artists.iter() {
                        div {
                            key: "{a.name}",
                            class: "row",
                            onclick: {
                                let name = a.name.clone();
                                move |_| nav.go(View::Artist(name.clone()))
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
            if active == Seg::Songs {
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
fn AlbumCard(album: Album, badge: String, nav: LibNav) -> Element {
    let key = album.key.clone();
    let ids = album.tracks.clone();
    rsx! {
        div { class: "album-card", onclick: move |_| nav.go(View::Album(key.clone())),
            div { class: "art-wrap",
                Cover { track_id: album.cover_track.clone().unwrap_or_default(), class: "cover card-cover" }
                if !badge.is_empty() {
                    span { class: "q-badge", "{badge}" }
                }
                button {
                    class: "art-play",
                    onclick: move |e| {
                        e.stop_propagation();
                        play_tracks(&ids, 0);
                    },
                    Icon { name: "play", size: 20 }
                }
            }
            div { class: "title", "{album.title}" }
            div { class: "subtitle", "{album.artist}" }
        }
    }
}

#[component]
fn EmptyLibrary() -> Element {
    let mut st = state();
    let has_perm = crate::platform::has_audio_permission();
    rsx! {
        div { class: "empty",
            div { class: "empty-icon", Icon { name: "library", size: 40 } }
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
pub fn SongRow(
    track: Track,
    ids: Vec<String>,
    index: usize,
    #[props(default = false)] show_cover: bool,
) -> Element {
    let st = state();
    let playing_id = st.player.read().current().map(|c| c.id.clone());
    let is_current = playing_id.as_deref() == Some(track.id.as_str());
    let mut menu = use_signal(|| false);
    let id = track.id.clone();
    let badge = track.quality_badge();
    let lossless = track.is_lossless();
    rsx! {
        div { class: if is_current { "row current" } else { "row" },
            div {
                class: "row-main",
                onclick: move |_| play_tracks(&ids, index),
                if show_cover {
                    div { class: "row-art",
                        Cover { track_id: track.id.clone(), class: "cover row-cover" }
                        if is_current {
                            div { class: "eq-overlay", Icon { name: "eq", size: 20 } }
                        }
                    }
                } else {
                    div { class: "track-no",
                        if is_current {
                            Icon { name: "eq", size: 18 }
                        } else {
                            "{track.track_no.map(|n| n.to_string()).unwrap_or_default()}"
                        }
                    }
                }
                div { class: "row-text",
                    div { class: "title", "{track.title}" }
                    div { class: "subtitle",
                        if lossless && !badge.is_empty() {
                            span { class: "mini-badge", "{badge}" }
                        }
                        if track.origin == loss_core::TrackOrigin::Downloaded {
                            span { class: "dl-dot", title: "Downloaded" }
                        }
                        "{track.artist}"
                        if show_cover { " · {track.album}" }
                    }
                }
                div { class: "muted small tabular", "{format_duration(track.duration_secs)}" }
            }
            button { class: "icon-btn small", onclick: move |_| menu.set(!menu()), Icon { name: "more", size: 20 } }
            if menu() {
                div { class: "menu-scrim", onclick: move |_| menu.set(false) }
                div { class: "row-menu",
                    button {
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                services::get().player.send(Command::PlayNext(queue_items(std::slice::from_ref(&id))));
                                menu.set(false);
                                st.toast("Playing next");
                            }
                        },
                        Icon { name: "play_next", size: 20 }
                        "Play next"
                    }
                    button {
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                services::get().player.send(Command::Enqueue(queue_items(std::slice::from_ref(&id))));
                                menu.set(false);
                                st.toast("Added to queue");
                            }
                        },
                        Icon { name: "queue", size: 20 }
                        "Add to queue"
                    }
                }
            }
        }
    }
}

#[component]
fn AlbumPage(album_key: String, nav: LibNav) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let lib = svc.library();
    let Some(album) = lib.album(&album_key) else {
        drop(lib);
        return rsx! {
            button { class: "back", onclick: move |_| { nav.back(); }, Icon { name: "back" } }
            p { class: "pad muted", "This album is no longer in your library." }
        };
    };
    let tracks: Vec<Track> = album
        .tracks
        .iter()
        .filter_map(|id| lib.get(id).cloned())
        .collect();
    drop(lib);
    let ids = album.tracks.clone();
    let ids2 = ids.clone();
    let ids3 = ids.clone();
    let artist = album.artist.clone();
    let badge = tracks
        .first()
        .map(|t| t.quality_badge())
        .unwrap_or_default();
    let fmt = tracks.first().map(|t| t.format.clone()).unwrap_or_default();
    let year = album.year.map(|y| format!("{y} · ")).unwrap_or_default();
    let playing_here = {
        let p = st.player.read();
        p.status == loss_audio::Status::Playing
            && p.current().map(|c| ids.contains(&c.id)).unwrap_or(false)
    };
    rsx! {
        button { class: "back", onclick: move |_| { nav.back(); }, Icon { name: "back" } }
        div { class: "album-hero",
            Cover { track_id: album.cover_track.clone().unwrap_or_default(), class: "cover hero-cover", big: true }
            h1 { "{album.title}" }
            button { class: "link", onclick: move |_| nav.go(View::Artist(artist.clone())), "{album.artist}" }
            p { class: "muted small", "{year}{tracks.len()} songs · {format_duration(album.duration_secs)}" }
            div { class: "tags center-tags",
                if !fmt.is_empty() { span { class: "tag", "{fmt}" } }
                if !badge.is_empty() && badge != fmt { span { class: "tag accent", "{badge}" } }
            }
            div { class: "hero-actions",
                button {
                    class: "icon-btn tonal big",
                    onclick: move |_| {
                        services::get().player.send(Command::Enqueue(queue_items(&ids3)));
                        st.toast("Album added to queue");
                    },
                    Icon { name: "queue" }
                }
                CookieButton {
                    playing: playing_here,
                    size: 76,
                    onclick: move |_| {
                        if playing_here {
                            services::get().player.send(Command::Pause);
                        } else {
                            play_tracks(&ids, 0);
                        }
                    },
                }
                button { class: "icon-btn tonal big", onclick: move |_| shuffle_tracks(&ids2), Icon { name: "shuffle" } }
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
fn ArtistPage(name: String, nav: LibNav) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let lib = svc.library();
    let norm = loss_core::normalize(&name);
    let albums: Vec<(Album, String)> = lib
        .albums()
        .into_iter()
        .filter(|a| loss_core::normalize(&a.artist) == norm)
        .map(|a| {
            let b = a
                .tracks
                .first()
                .and_then(|id| lib.get(id))
                .map(|t| t.quality_badge())
                .unwrap_or_default();
            (a, b)
        })
        .collect();
    let songs: Vec<Track> = lib
        .tracks
        .iter()
        .filter(|t| {
            loss_core::normalize(&t.artist) == norm || loss_core::normalize(&t.album_artist) == norm
        })
        .cloned()
        .collect();
    drop(lib);
    let ids: Vec<String> = songs.iter().map(|t| t.id.clone()).collect();
    let ids_play = ids.clone();
    let ids_shuffle = ids.clone();
    let initial = name.chars().next().unwrap_or('?');
    rsx! {
        button { class: "back", onclick: move |_| { nav.back(); }, Icon { name: "back" } }
        div { class: "artist-hero",
            div { class: "avatar huge", "{initial}" }
            h1 { "{name}" }
            p { class: "muted small", "{albums.len()} albums · {songs.len()} songs" }
            div { class: "hero-actions",
                button { class: "primary", onclick: move |_| play_tracks(&ids_play, 0), Icon { name: "play", size: 20 } "Play" }
                button { class: "pill", onclick: move |_| shuffle_tracks(&ids_shuffle), Icon { name: "shuffle", size: 18 } "Shuffle" }
            }
        }
        h2 { class: "section-title", "Albums" }
        div { class: "album-grid",
            for (a , badge) in albums.iter() {
                AlbumCard { key: "{a.key}", album: a.clone(), badge: badge.clone(), nav }
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
