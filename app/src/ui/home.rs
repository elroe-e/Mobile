//! Home: greeting, continue listening, shortcuts, recent albums, top songs.

use super::icons::Icon;
use super::{play_tracks, resume_last, shuffle_tracks, state, CookieButton, Cover, Tab};
use crate::services;
use dioxus::prelude::*;
use loss_audio::{Command, Status};
use loss_core::{format_duration, Track};

fn greeting() -> &'static str {
    use chrono::Timelike;
    match chrono::Local::now().hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        18..=22 => "Good evening",
        _ => "Late night",
    }
}

#[component]
pub fn HomeTab() -> Element {
    let mut st = state();
    let _ = (st.lib_version)();
    let svc = services::get();
    let p = st.player.read().clone();
    let settings = svc.settings();
    let lib = svc.library();
    let total = lib.len();
    let total_secs: f64 = lib.tracks.iter().map(|t| t.duration_secs).sum();
    let plays: u32 = lib.tracks.iter().map(|t| t.play_count).sum();

    // "Continue listening": what's loaded in the player, else the last song.
    let cont_id = p
        .current()
        .map(|c| c.id.clone())
        .or(settings.last_track.clone());
    let cont: Option<Track> = cont_id.and_then(|id| lib.get(&id).cloned());
    let (pos, dur) = if p.current().is_some() {
        (p.position, p.duration)
    } else {
        (
            settings.last_position,
            cont.as_ref().map(|t| t.duration_secs).unwrap_or(0.0),
        )
    };
    let playing = p.status == Status::Playing;
    let pct = if dur > 0.0 {
        (pos / dur * 100.0).min(100.0)
    } else {
        0.0
    };
    let has_queue = p.current().is_some();

    let mut seen = std::collections::HashSet::new();
    let recent_albums: Vec<Track> = lib
        .recently_added(300)
        .into_iter()
        .filter(|t| seen.insert(t.album_key()))
        .take(12)
        .cloned()
        .collect();
    let top: Vec<Track> = lib.most_played(5).into_iter().cloned().collect();
    let mut seen2 = std::collections::HashSet::new();
    let jump_back: Vec<Track> = lib
        .recently_played(60)
        .into_iter()
        .filter(|t| seen2.insert(t.album_key()))
        .take(8)
        .cloned()
        .collect();
    let all_ids: Vec<String> = lib.tracks.iter().map(|t| t.id.clone()).collect();
    drop(lib);

    let running = st
        .jobs
        .read()
        .iter()
        .filter(|j| !j.status.is_finished())
        .count();
    let top_ids: Vec<String> = top.iter().map(|t| t.id.clone()).collect();

    rsx! {
        header { class: "home-header",
            h1 { class: "display", "{greeting()}" }
            p { class: "muted", "Your music, on your phone." }
        }

        if let Some(t) = cont.clone() {
            h2 { class: "section-title", "Continue listening" }
            div { class: "hero-card", onclick: move |_| {
                    if has_queue { st.now_playing.set(true) } else { resume_last() }
                },
                Cover { track_id: t.id.clone(), class: "cover hero-art" }
                div { class: "hero-text",
                    div { class: "hero-title", "{t.title}" }
                    div { class: "hero-sub", "{t.artist}" }
                    div { class: "hero-progress",
                        div { style: "width: {pct}%" }
                    }
                }
                CookieButton {
                    playing,
                    size: 68,
                    onclick: move |e: MouseEvent| {
                        e.stop_propagation();
                        if playing {
                            services::get().player.send(Command::Pause);
                        } else {
                            resume_last();
                        }
                    },
                }
            }
        }

        h2 { class: "section-title", "Shortcuts" }
        div { class: "tiles",
            button { class: "tile tile-primary", disabled: total == 0, onclick: move |_| shuffle_tracks(&all_ids),
                span { class: "tile-icon", Icon { name: "shuffle" } }
                span { class: "tile-label", "Shuffle library" }
            }
            button { class: "tile tile-tertiary", onclick: move |_| st.tab.set(Tab::Discover),
                span { class: "tile-icon", Icon { name: "sparkle" } }
                span { class: "tile-label", "Free music" }
            }
            button { class: "tile tile-secondary", onclick: move |_| st.tab.set(Tab::Downloads),
                span { class: "tile-icon", Icon { name: "download" } }
                span { class: "tile-label",
                    if running > 0 { "Downloads · {running}" } else { "Downloads" }
                }
            }
            button { class: "tile tile-neutral", onclick: move |_| st.tab.set(Tab::Settings),
                span { class: "tile-icon", Icon { name: "stats" } }
                span { class: "tile-label", "{total} songs · {plays} plays" }
            }
        }

        if total == 0 {
            div { class: "empty-card",
                Icon { name: "library", size: 40 }
                div {
                    div { class: "title", "Your library is empty" }
                    div { class: "subtitle", "Add a music folder in Settings, or download free music from Discover." }
                }
            }
        }

        if !jump_back.is_empty() {
            h2 { class: "section-title", "Jump back in" }
            div { class: "h-scroll",
                for t in jump_back.iter() {
                    AlbumTile { key: "jb-{t.id}", track: t.clone() }
                }
            }
        }

        if !recent_albums.is_empty() {
            h2 { class: "section-title", "Recently added" }
            div { class: "h-scroll",
                for t in recent_albums.iter() {
                    AlbumTile { key: "ra-{t.id}", track: t.clone() }
                }
            }
        }

        if !top.is_empty() {
            div { class: "section-row",
                h2 { class: "section-title", "On repeat" }
                button { class: "text-btn", onclick: move |_| play_tracks(&top_ids, 0), "Play all" }
            }
            div { class: "card list-card",
                for (i , t) in top.iter().enumerate() {
                    div {
                        key: "{t.id}",
                        class: "row",
                        onclick: {
                            let ids: Vec<String> = top.iter().map(|t| t.id.clone()).collect();
                            move |_| play_tracks(&ids, i)
                        },
                        span { class: "rank", "{i + 1}" }
                        Cover { track_id: t.id.clone(), class: "cover row-cover" }
                        div { class: "row-text",
                            div { class: "title", "{t.title}" }
                            div { class: "subtitle", "{t.artist}" }
                        }
                        span { class: "muted small", "{t.play_count} plays" }
                    }
                }
            }
        }

        if total > 0 {
            p { class: "muted small center-text", "{total} songs · {format_duration(total_secs)} of music" }
        }
    }
}

/// Square album tile that plays the album.
#[component]
fn AlbumTile(track: Track) -> Element {
    let key = track.album_key();
    let badge = track.quality_badge();
    rsx! {
        div { class: "h-card",
            onclick: move |_| {
                let lib = services::get().library();
                if let Some(a) = lib.album(&key) {
                    let ids = a.tracks.clone();
                    drop(lib);
                    play_tracks(&ids, 0);
                }
            },
            div { class: "art-wrap",
                Cover { track_id: track.id.clone(), class: "cover card-cover" }
                if !badge.is_empty() {
                    span { class: "q-badge", "{badge}" }
                }
                span { class: "art-play", Icon { name: "play", size: 20 } }
            }
            div { class: "title", "{track.album}" }
            div { class: "subtitle", "{track.album_artist}" }
        }
    }
}
