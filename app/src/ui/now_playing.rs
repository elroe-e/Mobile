//! Full-screen player with seek bar, synced lyrics and the queue.

use super::audio_info::AudioInfoSheet;
use super::icons::Icon;
use super::kit::Dialog;
use super::{state, Cover, Slider};
use crate::services;
use dioxus::prelude::*;
use loss_audio::{Command, Repeat, Status};
use loss_core::{format_duration, lrc};
use loss_i18n::t;

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
    // After a seek, keep showing the target time until the player gets there.
    let mut seek_hold: Signal<Option<(f64, std::time::Instant)>> = use_signal(|| None);
    let mut closing = use_signal(|| false);
    // A volume above the loud limit waiting for the user's OK.
    let mut info_open = use_signal(|| false);
    let loud = use_signal(|| false);
    let loud_target = use_signal(|| 0.0f32);
    let Some(cur) = p.current().cloned() else {
        return rsx! {};
    };
    let svc = services::get();
    let track = svc.library().get(&cur.id).cloned();
    let title = track.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| {
        cur.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    });
    let artist = track.as_ref().map(|t| t.artist.clone()).unwrap_or_default();
    let album = track.as_ref().map(|t| t.album.clone()).unwrap_or_default();
    let pos = match (drag(), seek_hold()) {
        (Some(v), _) => v,
        (None, Some((v, t))) if (v - p.position).abs() > 1.0 && t.elapsed().as_millis() < 1500 => v,
        _ => p.position,
    };
    let dur = p.duration.max(0.0);
    let playing = p.status == Status::Playing;
    let quality = p.info.label();
    let lossless = p.info.is_lossless();
    let vol_now = (st.volume)();
    let repeat_icon = if p.repeat == Repeat::One {
        "repeat_one"
    } else {
        "repeat"
    };
    let (shuffle, repeat) = (p.shuffle, p.repeat);
    let pn = pane();
    let np_class = format!(
        "now-playing{}{}",
        if pn == Pane::Cover { "" } else { " panel-open" },
        if closing() { " closing" } else { "" }
    );
    // Slide down, then unmount.
    let mut close = move || {
        if closing() {
            return;
        }
        closing.set(true);
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(230)).await;
            st.now_playing.set(false);
        });
    };
    let toggle = move |target: Pane| {
        let mut pane = pane;
        pane.set(if pane() == target {
            Pane::Cover
        } else {
            target
        });
    };
    let next_up = p
        .index
        .and_then(|i| p.queue.get(i + 1))
        .and_then(|q| svc.library().get(&q.id).map(|t| t.title.clone()));

    rsx! {
        div { class: "{np_class}",
            // Keyed by track: a new song's art and title animate in.
            for id in std::iter::once(cur.id.clone()) {
                div { key: "art-{id}", class: "np-art-bg",
                    Cover { track_id: id.clone(), class: "cover np-art", big: true }
                    div { class: "np-art-fade" }
                }
            }
            div { class: "np-top",
                button { class: "icon-btn glass", onclick: move |_| close(), Icon { name: "chevron_down", size: 28 } }
                div { class: "np-top-title",
                    div { class: "eyebrow", {t!("player-now-playing")} }
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
            for id in std::iter::once(cur.id.clone()) {
                div { key: "info-{id}", class: "np-info",
                    super::kit::Marquee { text: title.clone(), class: "np-title" }
                    div { class: "np-artist", "{artist}" }
                    if let Some(err) = &p.error {
                        div { class: "error small",
                            {match err.kind {
                                loss_audio::PlayErrorKind::Output => t!("player-error-output"),
                                loss_audio::PlayErrorKind::Missing => t!("player-error-missing"),
                                loss_audio::PlayErrorKind::Unsupported => t!("player-error-unsupported"),
                            }}
                        }
                    }
                }
            }
            div { class: "np-seek",
                Slider {
                    class: "seek",
                    value: p.position,
                    max: dur.max(1.0),
                    on_input: move |v| drag.set(Some(v)),
                    on_commit: move |v| {
                        services::get().player.send(Command::Seek(v));
                        seek_hold.set(Some((v, std::time::Instant::now())));
                        drag.set(None);
                    },
                }
                div { class: "times",
                    span { "{format_duration(pos)}" }
                    if !p.info.codec.is_empty() {
                        button { class: if lossless { "quality lossless" } else { "quality" },
                            onclick: move |_| info_open.set(true),
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
                    for pl in std::iter::once(playing) {
                        span { key: "{pl}", class: "icon-swap", Icon { name: if pl { "pause" } else { "play" }, size: 36 } }
                    }
                }
                button { class: "skip-btn", onclick: move |_| services::get().player.send(Command::Next), Icon { name: "next", size: 30 } }
                button {
                    class: if repeat != Repeat::Off { "icon-btn on" } else { "icon-btn" },
                    onclick: move |_| services::get().player.send(Command::SetRepeat(repeat.next())),
                    Icon { name: repeat_icon }
                }
            }
            if !svc.settings().hide_volume {
                div { class: "np-volume",
                    button {
                        class: "icon-btn small",
                        onclick: move |_| services::get().set_volume((vol_now - 0.1).max(0.0)),
                        Icon { name: "volume_down", size: 20 }
                    }
                    Slider {
                        value: vol_now as f64,
                        max: 1.0,
                        on_input: move |v: f64| {
                            if loud() || !try_set_volume(v as f32, vol_now, loud, loud_target) {
                                return;
                            }
                            // Update the % label instantly, not on the next tick.
                            let mut vol = st.volume;
                            vol.set(v as f32);
                        },
                        on_commit: move |v: f64| {
                            if loud() || !try_set_volume(v as f32, vol_now, loud, loud_target) {
                                return;
                            }
                            let mut vol = st.volume;
                            vol.set(v as f32);
                            let svc = services::get();
                            if svc.system_volume().is_none() {
                                svc.update_settings(move |s| s.volume = v as f32);
                            }
                        },
                    }
                    span { class: "vol-pct", "{(vol_now * 100.0).round()}%" }
                }
            }
            AudioInfoSheet { open: info_open }
            Dialog {
                open: loud,
                title: t!("loud-volume-title"),
                text: t!("loud-volume-text", percent = (loud_target() * 100.0).round() as i64),
                confirm: t!("loud-volume-confirm"),
                on_confirm: move |_| {
                    let v = loud_target();
                    let mut vol = st.volume;
                    vol.set(v);
                    services::get().set_volume(v);
                },
            }
            div { class: "np-bottom",
                button { class: if pn == Pane::Lyrics { "np-chip on" } else { "np-chip" }, onclick: move |_| toggle(Pane::Lyrics),
                    Icon { name: "lyrics", size: 18 }
                    {t!("player-lyrics")}
                }
                if let Some(n) = next_up {
                    button { class: "np-chip next-up", onclick: move |_| toggle(Pane::Queue),
                        span { class: "muted", {t!("player-up-next")} }
                        span { class: "ellipsis", "{n}" }
                    }
                }
            }
        }
    }
}

/// Sets the volume unless it crosses the loud limit going up: then it keeps the target for the
/// confirmation dialog and returns false.
fn try_set_volume(v: f32, current: f32, mut loud: Signal<bool>, mut target: Signal<f32>) -> bool {
    let svc = services::get();
    let limit = svc.settings().loud_volume_limit;
    if loss_platform::policy::needs_loud_confirmation(current, v, limit) {
        target.set(v);
        loud.set(true);
        return false;
    }
    svc.set_volume(v);
    true
}

#[component]
fn LyricsPane(track_id: String, position: f64) -> Element {
    let st = state();
    let svc = services::get();
    let path = svc.library().get(&track_id).map(|t| t.path.clone());
    let mut text: Signal<Option<String>> =
        use_signal(|| path.as_ref().and_then(|p| services::get().local_lyrics(p)));
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
        let Some(t) = svc.library().get(&tid).cloned() else {
            return;
        };
        searching.set(true);
        status.set(None);
        let client = svc.downloads.client();
        spawn(async move {
            let (artist, title, album, dur) = (
                t.artist.clone(),
                t.title.clone(),
                t.album.clone(),
                t.duration_secs,
            );
            let res = services::get()
                .downloads
                .runtime()
                .spawn(async move {
                    loss_download::lyrics::fetch(&client, &artist, &title, Some(&album), Some(dur))
                        .await
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
                Ok(Ok(None)) => status.set(Some(t!("lyrics-none-found"))),
                Ok(Err(e)) => status.set(Some(t!("lyrics-search-failed", error = e.to_string()))),
                Err(e) => status.set(Some(t!("lyrics-search-failed", error = e.to_string()))),
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
                    p { class: "muted small center", {t!("lyrics-source")} }
                },
                Some(l) => rsx! {
                    pre { class: "plain-lyrics", "{l.plain}" }
                },
                None => rsx! {
                    div { class: "empty",
                        Icon { name: "lyrics", size: 48 }
                        p { {t!("lyrics-none-yet")} }
                        button {
                            class: "primary",
                            disabled: searching(),
                            onclick: fetch,
                            if searching() { {t!("lyrics-searching")} } else { {t!("lyrics-search-online")} }
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
                .unwrap_or_else(|| {
                    (
                        q.path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string(),
                        String::new(),
                    )
                });
            (i, q.id.clone(), t, a)
        })
        .collect();
    drop(lib);
    let current = p.index;
    rsx! {
        div { class: "queue",
            p { class: "muted small", {t!("queue-count", count = rows.len())} }
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
