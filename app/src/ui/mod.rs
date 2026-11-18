//! The Loss user interface (Dioxus).

mod discover;
mod downloads;
mod home;
mod icons;
mod library;
mod now_playing;
mod settings;
pub mod theme;

use crate::services;
use base64::Engine as _;
use dioxus::prelude::*;
use icons::Icon;
use loss_audio::{Command, PlayerState, QueueItem, Status};
use loss_download::JobSnapshot;
use std::sync::OnceLock;
use std::time::Duration;

/// Embedded so it works the same under `dx`, `cargo run` and in the APK.
const STYLE: &str = include_str!("../../assets/style.css");
const FONT_LATIN: &[u8] = include_bytes!("../../assets/fonts/google-sans-flex-latin.woff2");
const FONT_LATIN_EXT: &[u8] = include_bytes!("../../assets/fonts/google-sans-flex-latin-ext.woff2");

/// Google Sans Flex (SIL Open Font License), embedded as data URIs.
fn font_css() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| {
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        format!(
            "@font-face{{font-family:'Loss Sans';font-weight:300 900;font-display:swap;src:url(data:font/woff2;base64,{}) format('woff2');unicode-range:U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD;}}\
             @font-face{{font-family:'Loss Sans';font-weight:300 900;font-display:swap;src:url(data:font/woff2;base64,{}) format('woff2');unicode-range:U+0100-02BA,U+02BD-02C5,U+02C7-02CC,U+02CE-02D7,U+02DD-02FF,U+1D00-1DBF,U+1E00-1E9F,U+1EF2-1EFF,U+2020,U+20A0-20AB,U+20AD-20C4,U+2113,U+2C60-2C7F,U+A720-A7FF;}}",
            b64(FONT_LATIN),
            b64(FONT_LATIN_EXT)
        )
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Home,
    Library,
    Discover,
    Downloads,
    Settings,
}

/// Reactive app-wide state, provided as context.
#[derive(Clone, Copy)]
pub struct AppState {
    pub player: Signal<PlayerState>,
    pub jobs: Signal<Vec<JobSnapshot>>,
    pub lib_version: Signal<u64>,
    pub covers_version: Signal<u64>,
    pub scanning: Signal<Option<u64>>,
    pub tab: Signal<Tab>,
    pub now_playing: Signal<bool>,
    pub toast: Signal<Option<(String, std::time::Instant)>>,
    /// Bumped when settings that affect the look change.
    pub settings_rev: Signal<u64>,
}

impl AppState {
    /// Show a short message; the app tick hides it after a moment.
    pub fn toast(mut self, msg: impl Into<String>) {
        self.toast.set(Some((msg.into(), std::time::Instant::now())));
    }

    pub fn settings_changed(mut self) {
        let v = *self.settings_rev.peek();
        self.settings_rev.set(v + 1);
    }
}

pub fn state() -> AppState {
    use_context::<AppState>()
}

/// Play a list of library tracks starting at `start`.
pub fn play_tracks(ids: &[String], start: usize) {
    let items = queue_items(ids);
    if !items.is_empty() {
        services::get().player.send(Command::SetQueue {
            start: start.min(items.len() - 1),
            items,
        });
    }
}

/// Play a list in random order, starting from a random song.
pub fn shuffle_tracks(ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let p = &services::get().player;
    p.send(Command::SetShuffle(false));
    play_tracks(ids, nanos % ids.len());
    p.send(Command::SetShuffle(true));
}

pub fn queue_items(ids: &[String]) -> Vec<QueueItem> {
    let lib = services::get().library();
    ids.iter()
        .filter_map(|id| lib.get(id))
        .map(|t| QueueItem {
            id: t.id.clone(),
            path: t.path.clone(),
        })
        .collect()
}

/// Resume "Continue listening": the current queue, or the last song's album.
pub fn resume_last() {
    let svc = services::get();
    let st = svc.player.state();
    if st.current().is_some() {
        svc.player.send(Command::Play);
        return;
    }
    let s = svc.settings();
    let Some(id) = s.last_track else { return };
    let (ids, idx) = {
        let lib = svc.library();
        let Some(t) = lib.get(&id) else { return };
        match lib.album(&t.album_key()) {
            Some(a) => {
                let i = a.tracks.iter().position(|x| x == &id).unwrap_or(0);
                (a.tracks, i)
            }
            None => (vec![id.clone()], 0),
        }
    };
    play_tracks(&ids, idx);
    if s.last_position > 5.0 {
        svc.player.send(Command::Seek(s.last_position));
    }
}

#[component]
pub fn App() -> Element {
    let svc = services::get();
    let mut player = use_signal(|| svc.player.state());
    let mut jobs = use_signal(|| svc.downloads.jobs());
    let mut lib_version = use_signal(|| svc.library_version.load(std::sync::atomic::Ordering::SeqCst));
    let mut covers_version = use_signal(|| svc.covers_version());
    let mut scanning = use_signal(|| None);
    let tab = use_signal(|| Tab::Home);
    let now_playing = use_signal(|| false);
    let mut toast = use_signal(|| None::<(String, std::time::Instant)>);
    let settings_rev = use_signal(|| 0u64);
    use_context_provider(|| AppState {
        player,
        jobs,
        lib_version,
        covers_version,
        scanning,
        tab,
        now_playing,
        toast,
        settings_rev,
    });

    library::use_provide_nav();

    use_hook(|| {
        crate::media::start(services::get());
        std::thread::spawn(|| {
            // On Android the activity shows the permission prompt at launch;
            // wait for the answer before the first scan.
            if !crate::platform::has_audio_permission() {
                for _ in 0..120 {
                    std::thread::sleep(Duration::from_millis(500));
                    if crate::platform::has_audio_permission() {
                        break;
                    }
                }
            }
            services::get().rescan();
        });
    });

    // UI tick: copy engine state into signals (only when it changed).
    use_future(move || async move {
        let svc = services::get();
        let mut last_saved_pos = 0.0f64;
        loop {
            svc.sync();
            let p = svc.player.state();
            // Remember the song for "Continue listening".
            if let Some(cur) = p.current() {
                let s = svc.settings();
                let moved = (p.position - last_saved_pos).abs() > 15.0;
                if s.last_track.as_deref() != Some(cur.id.as_str()) || moved {
                    let (id, pos) = (cur.id.clone(), p.position);
                    last_saved_pos = pos;
                    svc.update_settings(move |s| {
                        s.last_track = Some(id);
                        s.last_position = pos;
                    });
                }
            }
            if *player.peek() != p {
                player.set(p);
            }
            let j = svc.downloads.jobs();
            if *jobs.peek() != j {
                jobs.set(j);
            }
            let v = svc.library_version.load(std::sync::atomic::Ordering::SeqCst);
            if *lib_version.peek() != v {
                lib_version.set(v);
            }
            let c = svc.covers_version();
            if *covers_version.peek() != c {
                covers_version.set(c);
            }
            let sc = svc
                .scanning
                .load(std::sync::atomic::Ordering::SeqCst)
                .then(|| svc.scan_progress.load(std::sync::atomic::Ordering::Relaxed));
            if *scanning.peek() != sc {
                scanning.set(sc);
            }
            let expired = matches!(&*toast.peek(), Some((_, t)) if t.elapsed() > Duration::from_millis(2600));
            if expired {
                toast.set(None);
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });

    // Theme: colours come from the playing (or last played) song's cover.
    let _ = settings_rev();
    let _ = covers_version();
    let settings = svc.settings();
    let current_id = player.read().current().map(|c| c.id.clone());
    let seed_track = current_id.clone().or(settings.last_track.clone());
    let accent = theme::parse_hex(&settings.accent).unwrap_or([232, 97, 90]);
    let seed = if settings.dynamic_color {
        seed_track.and_then(|id| svc.cover_color(&id)).unwrap_or(accent)
    } else {
        accent
    };
    let theme_css = theme::theme_css(seed, &settings.theme);
    let has_player = current_id.is_some();

    rsx! {
        style { {font_css()} }
        style { {STYLE} }
        style { {theme_css} }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        div { class: if has_player { "app with-player" } else { "app" },
            main { class: "content",
                match tab() {
                    Tab::Home => rsx! { home::HomeTab {} },
                    Tab::Library => rsx! { library::LibraryTab {} },
                    Tab::Discover => rsx! { discover::DiscoverTab {} },
                    Tab::Downloads => rsx! { downloads::DownloadsTab {} },
                    Tab::Settings => rsx! { settings::SettingsTab {} },
                }
            }
            if has_player {
                MiniPlayer {}
            }
            BottomNav {}
            if now_playing() && has_player {
                now_playing::NowPlaying {}
            }
            if let Some((msg, _)) = toast() {
                div { class: "toast", "{msg}" }
            }
        }
    }
}

#[component]
fn BottomNav() -> Element {
    let mut st = state();
    let active = (st.tab)();
    let running = st
        .jobs
        .read()
        .iter()
        .filter(|j| j.status.is_active() || j.status == loss_download::JobStatus::Queued)
        .count();
    let item = |t: Tab, icon: &'static str, label: &'static str| {
        rsx! {
            button {
                class: if active == t { "nav-item active" } else { "nav-item" },
                onclick: move |_| st.tab.set(t),
                div { class: "nav-pill",
                    Icon { name: icon }
                    if t == Tab::Downloads && running > 0 {
                        span { class: "badge", "{running}" }
                    }
                }
                span { class: "nav-label", "{label}" }
            }
        }
    };
    rsx! {
        nav { class: "bottom-nav",
            {item(Tab::Home, "home", "Home")}
            {item(Tab::Library, "library", "Library")}
            {item(Tab::Discover, "explore", "Discover")}
            {item(Tab::Downloads, "download", "Downloads")}
            {item(Tab::Settings, "settings", "Settings")}
        }
    }
}

#[component]
fn MiniPlayer() -> Element {
    let mut st = state();
    let p = st.player.read().clone();
    let svc = services::get();
    let Some(cur) = p.current().cloned() else {
        return rsx! {};
    };
    let track = svc.library().get(&cur.id).cloned();
    let (title, artist) = match &track {
        Some(t) => (t.title.clone(), t.artist.clone()),
        None => (
            cur.path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            String::new(),
        ),
    };
    let pct = if p.duration > 0.0 { p.position / p.duration * 100.0 } else { 0.0 };
    let playing = p.status == Status::Playing;
    rsx! {
        div { class: "mini-player", onclick: move |_| st.now_playing.set(true),
            Cover { track_id: cur.id.clone(), class: "cover mini-cover" }
            div { class: "mini-text",
                div { class: "title", "{title}" }
                div { class: "subtitle", "{artist}" }
            }
            button {
                class: "mini-play",
                onclick: move |e| {
                    e.stop_propagation();
                    services::get().player.send(Command::Toggle);
                },
                Icon { name: if playing { "pause" } else { "play" }, size: 26 }
            }
            button {
                class: "icon-btn",
                onclick: move |e| {
                    e.stop_propagation();
                    services::get().player.send(Command::Next);
                },
                Icon { name: "next", size: 26 }
            }
            div { class: "mini-track", div { class: "mini-progress", style: "width: {pct}%;" } }
        }
    }
}

/// A scalloped "cookie" shape (Material 3 Expressive style) for hero buttons.
fn cookie_path() -> &'static str {
    static P: OnceLock<String> = OnceLock::new();
    P.get_or_init(|| {
        let mut d = String::new();
        for i in 0..=180 {
            let a = i as f64 / 180.0 * std::f64::consts::TAU;
            let r = 45.0 + 4.0 * (9.0 * a).cos();
            let (x, y) = (50.0 + r * a.cos(), 50.0 + r * a.sin());
            d.push_str(&format!("{}{x:.2},{y:.2}", if i == 0 { "M" } else { "L" }));
        }
        d.push('Z');
        d
    })
}

/// Big play/pause button on a cookie shape.
#[component]
pub fn CookieButton(playing: bool, #[props(default = 64)] size: u32, onclick: EventHandler<MouseEvent>) -> Element {
    let icon = (size as f64 * 0.42) as u32;
    rsx! {
        button {
            class: "cookie",
            style: "width:{size}px;height:{size}px",
            onclick: move |e| onclick.call(e),
            svg { view_box: "0 0 100 100", class: "cookie-bg", path { d: cookie_path() } }
            span { class: "cookie-icon", Icon { name: if playing { "pause" } else { "play" }, size: icon } }
        }
    }
}

/// Album art for a library track (falls back to a music note).
#[component]
pub fn Cover(track_id: String, #[props(default = "cover".to_string())] class: String, #[props(default = false)] big: bool) -> Element {
    let st = state();
    let _ = (st.covers_version)(); // re-render when covers load
    let svc = services::get();
    let uri = if big { svc.big_cover(&track_id) } else { svc.thumb(&track_id) };
    rsx! {
        div { class: "{class}",
            if let Some(src) = uri {
                img { src: "{src}", alt: "" }
            } else {
                div { class: "cover-placeholder", Icon { name: "note", size: if big { 96 } else { 22 } } }
            }
        }
    }
}

/// A cover image from a URL (search results).
#[component]
pub fn RemoteCover(url: Option<String>, #[props(default = "cover".to_string())] class: String) -> Element {
    rsx! {
        div { class: "{class}",
            if let Some(src) = url {
                img { src: "{src}", alt: "", loading: "lazy" }
            } else {
                div { class: "cover-placeholder", Icon { name: "note", size: 22 } }
            }
        }
    }
}

pub fn format_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{b:.0} B")
    }
}
