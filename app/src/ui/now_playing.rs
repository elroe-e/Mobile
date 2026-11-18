//! Full-screen player with seek bar, synced lyrics and the queue.

use super::icons::Icon;
use super::{state, Cover};
use crate::services;
use dioxus::prelude::*;
use loss_audio::{Command, Repeat, Status};
use loss_core::{format_duration, lrc};

#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Cover,
    Lyrics,
    Queue,
}

#[component]
pub fn NowPlaying() -> Element {
    let mut st = state();
    let p = st.player.read().clone();
    let pane = use_signal(|| Pane::Cover);
    let mut drag: Signal<Option<f64>> = use_signal(|| None);
    let Some(cur) = p.current().cloned() else {
        return rsx! {};
    };
    let svc = services::get();
    let track = svc.library().get(&cur.id).cloned();
    let title = track.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| {
        cur.path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
    });
    let artist = track.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
    let album = track.as_ref().map(|t| t.album.clone()).unwrap_or_default();
    let pos = drag().unwrap_or(p.position);
    let dur = p.duration.max(0.0);
    let seek_pct = if dur > 0.0 { pos / dur * 100.0 } else { 0.0 };
    let playing = p.status == Status::Playing;
    let quality = p.info.label();
    let lossless = p.info.is_lossless();
    let vol = (p.volume * 100.0).round();
    let repeat_icon = if p.repeat == Repeat::One { "repeat_one" } else { "repeat" };
    let (shuffle, repeat) = (p.shuffle, p.repeat);
    let pn = pane();
    let toggle = move |target: Pane| {
        let mut pane = pane;
        pane.set(if pane() == target { Pane::Cover } else { target });
    };
    let next_up = p
        .index
        .and_then(|i| p.queue.get(i + 1))
        .and_then(|q| svc.library().get(&q.id).map(|t| t.title.clone()));

    rsx! {
        div { class: if pn == Pane::Cover { "now-playing" } else { "now-playing panel-open" },
            div { class: "np-art-bg",
                Cover { track_id: cur.id.clone(), class: "cover np-art", big: true }
                div { class: "np-art-fade" }
            }
            div { class: "np-top",
                button { class: "icon-btn glass", onclick: move |_| st.now_playing.set(false), Icon { name: "chevron_down", size: 28 } }
                div { class: "np-top-title",
                    div { class: "eyebrow", "Now playing" }
                    div { class: "np-top-album", "{album}" }
                }
                button { class: "icon-btn glass", onclick: move |_| toggle(Pane::Queue), Icon { name: "queue", size: 24 } }
            }
            div { class: "np-stage",
                match pn {
                    Pane::Cover => rsx! {},
                    Pane::Lyrics => rsx! { div { class: "np-panel", LyricsPane { key: "{cur.id}", track_id: cur.id.clone(), position: p.position } } },
                    Pane::Queue => rsx! { div { class: "np-panel", QueuePane {} } },
                }
            }
            div { class: "np-info",
                div { class: "np-title", "{title}" }
                div { class: "np-artist", "{artist}" }
                if let Some(err) = &p.error {
                    div { class: "error small", "{err}" }
                }
            }
            div { class: "np-seek",
                input {
                    r#type: "range",
                    class: "seek",
                    min: "0",
                    max: "{dur.max(1.0)}",
                    step: "0.5",
                    value: "{pos}",
                    style: "--pct: {seek_pct}%",
                    oninput: move |e| drag.set(e.value().parse().ok()),
                    onchange: move |e| {
                        if let Ok(v) = e.value().parse::<f64>() {
                            services::get().player.send(Command::Seek(v));
                        }
                        drag.set(None);
                    },
                }
                div { class: "times",
                    span { "{format_duration(pos)}" }
                    if !p.info.codec.is_empty() {
                        span { class: if lossless { "quality lossless" } else { "quality" },
                            Icon { name: "eq", size: 14 }
                            "{quality}"
                        }
                    }
                    span { "{format_duration(dur)}" }
                }
            }
            div { class: "np-controls",
                button {
                    class: if shuffle { "icon-btn on" } else { "icon-btn" },
                    onclick: move |_| services::get().player.send(Command::SetShuffle(!shuffle)),
                    Icon { name: "shuffle" }
                }
                button { class: "skip-btn", onclick: move |_| services::get().player.send(Command::Previous), Icon { name: "prev", size: 30 } }
                button { class: if playing { "play-btn playing" } else { "play-btn" }, onclick: move |_| services::get().player.send(Command::Toggle),
                    Icon { name: if playing { "pause" } else { "play" }, size: 36 }
                }
                button { class: "skip-btn", onclick: move |_| services::get().player.send(Command::Next), Icon { name: "next", size: 30 } }
                button {
                    class: if repeat != Repeat::Off { "icon-btn on" } else { "icon-btn" },
                    onclick: move |_| services::get().player.send(Command::SetRepeat(repeat.next())),
                    Icon { name: repeat_icon }
                }
            }
            div { class: "np-volume",
                Icon { name: "volume_down", size: 20 }
                input {
                    r#type: "range",
                    min: "0",
                    max: "100",
                    value: "{vol}",
                    style: "--pct: {vol}%",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() {
                            services::get().player.send(Command::SetVolume(v / 100.0));
                        }
                    },
                    onchange: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() {
                            services::get().update_settings(|s| s.volume = v / 100.0);
                        }
                    },
                }
                Icon { name: "volume_up", size: 20 }
            }
            div { class: "np-bottom",
                button { class: if pn == Pane::Lyrics { "np-chip on" } else { "np-chip" }, onclick: move |_| toggle(Pane::Lyrics),
                    Icon { name: "lyrics", size: 18 }
                    "Lyrics"
                }
                if let Some(n) = next_up {
                    button { class: "np-chip next-up", onclick: move |_| toggle(Pane::Queue),
                        span { class: "muted", "Up next" }
                        span { class: "ellipsis", "{n}" }
                    }
                }
            }
        }
    }
}

#[component]
fn LyricsPane(track_id: String, position: f64) -> Element {
    let st = state();
    let svc = services::get();
    let path = svc.library().get(&track_id).map(|t| t.path.clone());
    let mut text: Signal<Option<String>> = use_signal(|| path.as_ref().and_then(|p| services::get().local_lyrics(p)));
    let mut searching = use_signal(|| false);
    let mut status = use_signal(|| None::<String>);
    let lyrics = use_memo(move || text().map(|t| lrc::parse(&t)));
    let current = lyrics().and_then(|l| l.current_line(position + 0.2));

    use_effect(move || {
        let _ = current;
        document::eval("document.getElementById('lyr-cur')?.scrollIntoView({block:'center',behavior:'smooth'})");
    });

    let tid = track_id.clone();
    let fetch = move |_| {
        let svc = services::get();
        let Some(t) = svc.library().get(&tid).cloned() else { return };
        searching.set(true);
        status.set(None);
        let client = svc.downloads.client();
        spawn(async move {
            let (artist, title, album, dur) = (t.artist.clone(), t.title.clone(), t.album.clone(), t.duration_secs);
            let res = services::get()
                .downloads
                .runtime()
                .spawn(async move {
                    loss_download::lyrics::fetch(&client, &artist, &title, Some(&album), Some(dur)).await
                })
                .await;
            searching.set(false);
            match res {
                Ok(Ok(Some(found))) => {
                    if let Some(body) = found.best() {
                        let svc = services::get();
                        // Save next to the file if we can, otherwise in app storage.
                        if std::fs::write(t.path.with_extension("lrc"), body).is_err() {
                            let cache = svc.lyrics_cache_path(&t.path);
                            if let Some(d) = cache.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let _ = std::fs::write(cache, body);
                        }
                        text.set(Some(body.to_string()));
                    }
                }
                Ok(Ok(None)) => status.set(Some("No lyrics found for this song.".into())),
                Ok(Err(e)) => status.set(Some(format!("Couldn't search: {e}"))),
                Err(e) => status.set(Some(format!("Couldn't search: {e}"))),
            }
        });
    };
    let _ = st;

    rsx! {
        div { class: "lyrics",
            match lyrics() {
                Some(l) if l.is_synced() => rsx! {
                    for (i , line) in l.synced.iter().enumerate() {
                        p {
                            key: "{i}",
                            id: if Some(i) == current { "lyr-cur" } else { "" },
                            class: if Some(i) == current { "lyric active" } else if current.map(|c| i < c).unwrap_or(false) { "lyric past" } else { "lyric" },
                            onclick: {
                                let t = line.time;
                                move |_| services::get().player.send(Command::Seek(t))
                            },
                            if line.text.is_empty() { "♪" } else { "{line.text}" }
                        }
                    }
                    p { class: "muted small center", "Lyrics from LRCLIB or your files" }
                },
                Some(l) => rsx! {
                    pre { class: "plain-lyrics", "{l.plain}" }
                },
                None => rsx! {
                    div { class: "empty",
                        Icon { name: "lyrics", size: 48 }
                        p { "No lyrics for this song yet." }
                        button {
                            class: "primary",
                            disabled: searching(),
                            onclick: fetch,
                            if searching() { "Searching…" } else { "Search online" }
                        }
                        if let Some(s) = status() {
                            p { class: "muted small", "{s}" }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn QueuePane() -> Element {
    let st = state();
    let p = st.player.read().clone();
    let svc = services::get();
    let lib = svc.library();
    let rows: Vec<(usize, String, String, String)> = p
        .queue
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let (t, a) = lib
                .get(&q.id)
                .map(|t| (t.title.clone(), t.artist.clone()))
                .unwrap_or_else(|| (q.path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(), String::new()));
            (i, q.id.clone(), t, a)
        })
        .collect();
    drop(lib);
    let current = p.index;
    rsx! {
        div { class: "queue",
            p { class: "muted small", "{rows.len()} songs in queue" }
            for (i , id , title , artist) in rows {
                div { key: "{i}-{id}", class: if Some(i) == current { "row current" } else { "row" },
                    div { class: "row-main", onclick: move |_| services::get().player.send(Command::JumpTo(i)),
                        div { class: "track-no", "{i + 1}" }
                        div { class: "row-text",
                            div { class: "title", "{title}" }
                            div { class: "subtitle", "{artist}" }
                        }
                    }
                    if Some(i) != current {
                        button { class: "icon-btn small", onclick: move |_| services::get().player.send(Command::Remove(i)), Icon { name: "close", size: 18 } }
                    }
                }
            }
        }
    }
}
