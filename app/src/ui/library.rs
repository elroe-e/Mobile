//! Library: songs, albums and artists from your folders and downloads.

use super::icons::Icon;
use super::{play_tracks, queue_items, shuffle_tracks, state, CookieButton, Cover, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_audio::Command;
use loss_core::{format_duration, Album, Track};
use loss_i18n::t;

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
    fn label(self) -> String {
        match self {
            Sort::Latest => t!("library-sort-latest"),
            Sort::Title => t!("library-sort-title"),
            Sort::Artist => t!("library-sort-artist"),
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

/// A list of song ids shared by every row of a list (cloning the handle is cheap, and two
/// handles are equal only if they are the same list).
#[derive(Clone)]
pub struct Ids(pub std::rc::Rc<Vec<String>>);

impl PartialEq for Ids {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl From<Vec<String>> for Ids {
    fn from(v: Vec<String>) -> Self {
        Ids(std::rc::Rc::new(v))
    }
}

/// Song rows with a cover are this tall (8 + 52 + 8 px); the virtual list needs it fixed.
const SONG_ROW_HEIGHT: f64 = 68.0;

#[component]
fn LibraryMain(nav: LibNav) -> Element {
    let st = state();
    let _ = (st.lib_version)();
    let mut seg = use_signal(|| Seg::Albums);
    let mut sort = use_signal(|| Sort::Latest);
    let mut query = use_signal(String::new);
    let svc = services::get();
    let mut grid_size = use_signal(|| svc.settings().album_grid_size.clamp(100, 280) as f64);
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
    let album_info: std::rc::Rc<Vec<(Album, String)>> = albums
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
        .collect::<Vec<_>>()
        .into();
    drop(lib);

    let song_ids: Ids = songs
        .iter()
        .map(|t| t.id.clone())
        .collect::<Vec<_>>()
        .into();
    let songs = std::rc::Rc::new(songs);
    let active = if searching { Seg::Songs } else { seg() };
    let count_line = match active {
        Seg::Songs => t!("count-songs", count = songs.len()),
        Seg::Albums => t!("count-albums", count = n_albums),
        Seg::Artists => t!("count-artists", count = n_artists),
    };

    rsx! {
        header { class: "page-header",
            h1 { {t!("tab-library")} }
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
                placeholder: t!("library-search"),
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
                for (s , icon , label , n) in [(Seg::Songs, "note", t!("library-songs"), n_songs), (Seg::Albums, "album", t!("library-albums"), n_albums), (Seg::Artists, "person", t!("library-artists"), n_artists)] {
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
                super::kit::VirtualGrid {
                    id: "library-albums",
                    len: album_info.len(),
                    min_item: grid_size(),
                    // Title and artist lines under the cover.
                    extra: 54.0,
                    render: {
                        let album_info = album_info.clone();
                        move |i: usize| {
                            let (a, badge) = &album_info[i];
                            rsx! { AlbumCard { key: "{a.key}", album: a.clone(), badge: badge.clone(), nav } }
                        }
                    },
                    // Pinch (or Ctrl+wheel) makes covers bigger or smaller; remembered.
                    on_zoom: move |f: f64| {
                        let size = (grid_size() * f).clamp(100.0, 280.0);
                        if (size - grid_size()).abs() >= 1.0 {
                            grid_size.set(size);
                            super::kit::haptic(super::kit::Haptic::Tick);
                            services::get().update_settings(move |s| s.album_grid_size = size as u32);
                        }
                    },
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
                                div { class: "subtitle", {t!("artist-meta", albums = a.album_keys.len(), songs = a.track_count)} }
                            }
                            Icon { name: "chevron_right" }
                        }
                    }
                }
            }
            if active == Seg::Songs {
                if songs.is_empty() {
                    p { class: "muted pad", {t!("library-no-match", query = q.clone())} }
                }
                super::kit::VirtualList {
                    id: "library-songs",
                    len: songs.len(),
                    row_height: SONG_ROW_HEIGHT,
                    render: {
                        let songs = songs.clone();
                        let ids = song_ids.clone();
                        move |i: usize| {
                            let t = &songs[i];
                            rsx! { SongRow { key: "{t.id}", track: t.clone(), ids: ids.clone(), index: i, show_cover: true } }
                        }
                    },
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
            h2 { {t!("library-empty-title")} }
            p { {t!("library-empty-text")} }
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
                    {t!("permission-allow-music")}
                }
                button { class: "pill", onclick: move |_| crate::platform::open_app_settings(), {t!("permission-open-settings")} }
            }
            button { class: "primary", onclick: move |_| services::get().rescan(), Icon { name: "refresh", size: 18 } " Scan again" }
            button { class: "pill", onclick: move |_| st.tab.set(Tab::Settings), {t!("library-choose-folders")} }
            button { class: "pill", onclick: move |_| st.tab.set(Tab::Discover), {t!("library-find-free")} }
        }
    }
}

#[component]
pub fn SongRow(
    track: Track,
    ids: Ids,
    index: usize,
    #[props(default = false)] show_cover: bool,
) -> Element {
    let st = state();
    let playing_id = st.player.read().current().map(|c| c.id.clone());
    let is_current = playing_id.as_deref() == Some(track.id.as_str());
    let mut menu = use_signal(|| false);
    // Swipe towards the end edge (right, or left in right-to-left languages) to play next.
    let mut swipe_start = use_signal(|| None::<(f64, f64)>);
    let mut swipe_x = use_signal(|| 0.0f64);
    let mut swiped = use_signal(|| false);
    let rtl = loss_i18n::is_rtl();
    let id = track.id.clone();
    let id_swipe = track.id.clone();
    let ids_key = ids.clone();
    let badge = track.quality_badge();
    let lossless = track.is_lossless();
    rsx! {
        div {
            class: if is_current { "row current" } else { "row" },
            // Long press (Android) or right click (desktop) opens the song's menu.
            oncontextmenu: move |e| {
                e.prevent_default();
                super::kit::haptic(super::kit::Haptic::LongPress);
                menu.set(true);
            },
            if swipe_x() != 0.0 {
                div { class: "swipe-hint", Icon { name: "play_next", size: 22 } }
            }
            div {
                class: "row-main",
                tabindex: "0",
                style: if swipe_x() != 0.0 { "transform: translateX({swipe_x()}px); transition: none" } else { "" },
                onclick: move |_| {
                    // The end of a swipe isn't a tap.
                    if !swiped() {
                        play_tracks(&ids.0, index);
                    }
                    swiped.set(false);
                },
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        play_tracks(&ids_key.0, index);
                    }
                },
                onpointerdown: move |e: PointerEvent| {
                    let p = e.client_coordinates();
                    swipe_start.set(Some((p.x, p.y)));
                },
                onpointermove: move |e: PointerEvent| {
                    let Some((x0, y0)) = swipe_start() else { return };
                    let p = e.client_coordinates();
                    let (dx, dy) = (p.x - x0, p.y - y0);
                    let forward = if rtl { -dx } else { dx };
                    if swipe_x() != 0.0 || (forward > 12.0 && forward > dy.abs() * 1.5) {
                        swipe_x.set(dx.clamp(if rtl { -140.0 } else { 0.0 }, if rtl { 0.0 } else { 140.0 }));
                    }
                },
                onpointerup: move |_| {
                    let moved = swipe_x().abs();
                    if moved > 90.0 {
                        super::kit::haptic(super::kit::Haptic::Confirm);
                        services::get().player.send(Command::PlayNext(queue_items(std::slice::from_ref(&id_swipe))));
                        st.toast(t!("toast-playing-next"));
                    }
                    swiped.set(moved > 12.0);
                    swipe_x.set(0.0);
                    swipe_start.set(None);
                },
                onpointercancel: move |_| {
                    swipe_x.set(0.0);
                    swipe_start.set(None);
                },
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
                            span { class: "dl-dot", title: t!("library-downloaded") }
                        }
                        "{track.artist}"
                        if show_cover { " · {track.album}" }
                    }
                }
                div { class: "muted small tabular", "{format_duration(track.duration_secs)}" }
            }
            button {
                class: "icon-btn small",
                "aria-label": t!("action-more"),
                onclick: move |_| menu.set(true),
                Icon { name: "more", size: 20 }
            }
            super::kit::Sheet { open: menu, title: track.title.clone(),
                super::kit::SheetItem {
                    icon: "play_next",
                    label: t!("action-play-next"),
                    onclick: {
                        let id = id.clone();
                        move |_| {
                            services::get().player.send(Command::PlayNext(queue_items(std::slice::from_ref(&id))));
                            menu.set(false);
                            st.toast(t!("toast-playing-next"));
                        }
                    },
                }
                super::kit::SheetItem {
                    icon: "queue",
                    label: t!("action-add-queue"),
                    onclick: {
                        let id = id.clone();
                        move |_| {
                            services::get().player.send(Command::Enqueue(queue_items(std::slice::from_ref(&id))));
                            menu.set(false);
                            st.toast(t!("toast-added-queue"));
                        }
                    },
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
            p { class: "pad muted", {t!("album-gone")} }
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
    let album_ids: Ids = ids.clone().into();
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
            p { class: "muted small", "{year}" {t!("album-meta", songs = tracks.len(), length = format_duration(album.duration_secs))} }
            div { class: "tags center-tags",
                if !fmt.is_empty() { span { class: "tag", "{fmt}" } }
                if !badge.is_empty() && badge != fmt { span { class: "tag accent", "{badge}" } }
            }
            div { class: "hero-actions",
                button {
                    class: "icon-btn tonal big",
                    onclick: move |_| {
                        services::get().player.send(Command::Enqueue(queue_items(&ids3)));
                        st.toast(t!("toast-album-queued"));
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
                SongRow { key: "{t.id}", track: t.clone(), ids: album_ids.clone(), index: i }
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
    let row_ids: Ids = ids.clone().into();
    let ids_shuffle = ids.clone();
    let initial = name.chars().next().unwrap_or('?');
    rsx! {
        button { class: "back", onclick: move |_| { nav.back(); }, Icon { name: "back" } }
        div { class: "artist-hero",
            div { class: "avatar huge", "{initial}" }
            h1 { "{name}" }
            p { class: "muted small", {t!("artist-meta", albums = albums.len(), songs = songs.len())} }
            div { class: "hero-actions",
                button { class: "primary", onclick: move |_| play_tracks(&ids_play, 0), Icon { name: "play", size: 20 } {t!("action-play")} }
                button { class: "pill", onclick: move |_| shuffle_tracks(&ids_shuffle), Icon { name: "shuffle", size: 18 } {t!("action-shuffle")} }
            }
        }
        h2 { class: "section-title", {t!("library-albums")} }
        div { class: "album-grid",
            for (a , badge) in albums.iter() {
                AlbumCard { key: "{a.key}", album: a.clone(), badge: badge.clone(), nav }
            }
        }
        h2 { class: "section-title", {t!("library-songs")} }
        div { class: "list",
            for (i , t) in songs.iter().enumerate() {
                SongRow { key: "{t.id}", track: t.clone(), ids: row_ids.clone(), index: i, show_cover: true }
            }
        }
    }
}
