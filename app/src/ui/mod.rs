//! The Loss user interface (Dioxus).

mod discover;
mod downloads;
mod icons;
mod library;
mod now_playing;
mod settings;

use crate::services;
use dioxus::prelude::*;
use icons::Icon;
use loss_audio::{Command, PlayerState, QueueItem, Status};
use loss_download::JobSnapshot;
use std::time::Duration;

/// Embedded so it works the same under `dx`, `cargo run` and in the APK.
const STYLE: &str = include_str!("../../assets/style.css");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
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
}

impl AppState {
    /// Show a short message; the app tick hides it after a moment.
    pub fn toast(mut self, msg: impl Into<String>) {
        self.toast.set(Some((msg.into(), std::time::Instant::now())));
    }
}

pub fn state() -> AppState {
    use_context::<AppState>()
}

/// Play a list of library tracks starting at `start`.
pub fn play_tracks(ids: &[String], start: usize) {
    let svc = services::get();
    let items: Vec<QueueItem> = {
        let lib = svc.library();
        ids.iter()
            .filter_map(|id| lib.get(id))
            .map(|t| QueueItem {
                id: t.id.clone(),
                path: t.path.clone(),
            })
            .collect()
    };
    if !items.is_empty() {
        svc.player.send(Command::SetQueue {
            items,
            start: start.min(ids.len().saturating_sub(1)),
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

#[component]
pub fn App() -> Element {
    let svc = services::get();
    let mut player = use_signal(|| svc.player.state());
    let mut jobs = use_signal(|| svc.downloads.jobs());
    let mut lib_version = use_signal(|| svc.library_version.load(std::sync::atomic::Ordering::SeqCst));
    let mut covers_version = use_signal(|| svc.covers_version());
    let mut scanning = use_signal(|| None);
    let tab = use_signal(|| Tab::Library);
    let now_playing = use_signal(|| false);
    let mut toast = use_signal(|| None::<(String, std::time::Instant)>);
    use_context_provider(|| AppState {
        player,
        jobs,
        lib_version,
        covers_version,
        scanning,
        tab,
        now_playing,
        toast,
    });

    // First launch: ask for permission to read music, then scan.
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
        loop {
            svc.sync();
            let p = svc.player.state();
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

    let accent = svc.settings().accent;
    let has_player = player.read().current().is_some();

    rsx! {
        style { {STYLE} }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1, viewport-fit=cover" }
        div {
            class: if has_player { "app with-player" } else { "app" },
            style: "--accent: {accent};",
            main { class: "content",
                match tab() {
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
                div { class: "nav-icon",
                    Icon { name: icon }
                    if t == Tab::Downloads && running > 0 {
                        span { class: "badge", "{running}" }
                    }
                }
                span { "{label}" }
            }
        }
    };
    rsx! {
        nav { class: "bottom-nav",
            {item(Tab::Library, "library", "Library")}
            {item(Tab::Discover, "search", "Discover")}
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
            div { class: "mini-progress", style: "width: {pct}%;" }
            Cover { track_id: cur.id.clone(), class: "cover mini-cover" }
            div { class: "mini-text",
                div { class: "title", "{title}" }
                div { class: "subtitle", "{artist}" }
            }
            button {
                class: "icon-btn",
                onclick: move |e| {
                    e.stop_propagation();
                    services::get().player.send(Command::Toggle);
                },
                Icon { name: if playing { "pause" } else { "play" }, size: 30 }
            }
            button {
                class: "icon-btn",
                onclick: move |e| {
                    e.stop_propagation();
                    services::get().player.send(Command::Next);
                },
                Icon { name: "next", size: 28 }
            }
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
