//! Keeps Android's media notification, lock screen and Bluetooth/headset
//! buttons in sync with the player. Runs on its own thread so it keeps
//! working while the app is in the background.
//!
//! Events from Android (buttons, audio focus, unplugging, volume) arrive through
//! `loss-platform` as calls and are handled on the `loss-events` thread, which
//! sleeps until one comes. The `loss-media` thread pushes the player's state out
//! to the notification. The Kotlin side is `android/glue` (LossBridge,
//! LossPlaybackService). On other platforms this does nothing.

use crate::services::Services;

#[cfg(target_os = "android")]
mod imp {
    use crate::services::Services;
    use loss_audio::{Command, Status};
    use loss_platform::android::{self as bridge, NowPlaying};
    use loss_platform::{Event, FocusPolicy, Reaction};
    use std::sync::{mpsc, Mutex};
    use std::time::{Duration, Instant};

    /// Audio-focus and unplug decisions (shared by both threads).
    static POLICY: Mutex<FocusPolicy> = Mutex::new(FocusPolicy::new());

    fn is_playing(svc: &Services) -> bool {
        matches!(svc.player.state().status, Status::Playing | Status::Loading)
    }

    fn handle(svc: &Services, event: Event) {
        let playing = is_playing(svc);
        let reaction = POLICY
            .lock()
            .map(|mut p| p.on_event(&event, playing))
            .unwrap_or_default();
        match &event {
            Event::Media(cmd) => super::apply(svc, *cmd),
            Event::Volume { level, max } => {
                let v = *level as f32 / *max as f32;
                svc.system_volume
                    .store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
            }
            Event::OutputsAdded(t) | Event::OutputsRemoved(t) => {
                log::info!("audio outputs changed: {event:?} {t:?}");
                svc.refresh_route();
                svc.usb_version
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Event::UsbPermission { name, granted } => {
                log::info!("USB permission for {name}: {granted}");
                svc.usb_version
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Event::Focus(_) | Event::Noisy => log::info!("{event:?} -> {reaction:?}"),
        }
        match reaction {
            Reaction::Pause => svc.player.send(Command::Pause),
            Reaction::Resume => svc.player.send(Command::Play),
            Reaction::Nothing => {}
        }
    }

    /// Registers the natives and starts the event thread. Events that arrived
    /// before this are delivered by the glue right after registration.
    pub fn start_events(svc: &'static Services) {
        let (tx, rx) = mpsc::channel::<Event>();
        std::thread::Builder::new()
            .name("loss-events".into())
            .spawn(move || {
                while let Ok(event) = rx.recv() {
                    handle(svc, event);
                }
            })
            .expect("events thread");
        bridge::start(bridge::Handlers {
            event: Box::new(move |e| {
                let _ = tx.send(e);
            }),
            back: Box::new(crate::nav::request_back),
        });
        crate::platform::install_bit_perfect_hook();
        svc.refresh_route();
    }

    fn push_downloads(n: &crate::notify::DownloadsNotice) {
        use crate::notify::DownloadsState;
        let state = match n.state {
            DownloadsState::Running => 1,
            DownloadsState::Paused => 2,
            DownloadsState::Finished | DownloadsState::Idle => 0,
        };
        bridge::update_downloads(state, &n.title, &n.text, n.progress);
    }

    pub fn run(svc: &'static Services) {
        // Attach once: this thread lives as long as the app.
        if let Some(vm) = bridge::vm() {
            let _ = vm.attach_current_thread_permanently();
        }
        // Visible fields last sent (position excluded: it moves by itself).
        let mut shown: Option<NowPlaying> = None;
        // Downloads notification: last sent value, when, and whether any ran.
        let mut dl_sent: Option<crate::notify::DownloadsNotice> = None;
        let mut dl_sent_at = Instant::now();
        let mut dl_ran = false;
        let mut last_sent = Instant::now();
        let mut last_pos = 0.0;
        loop {
            // Only outgoing state: commands and other events arrive on loss-events.
            std::thread::sleep(Duration::from_millis(300));
            svc.sync();

            let notice = crate::notify::downloads_notice(&svc.downloads.jobs(), dl_ran);
            let running = notice.state == crate::notify::DownloadsState::Running;
            let state_changed = dl_sent
                .as_ref()
                .map(|d| d.state != notice.state)
                .unwrap_or(true);
            let content_changed = dl_sent.as_ref() != Some(&notice);
            // State changes go out at once; progress at most once a second.
            if state_changed || (content_changed && dl_sent_at.elapsed() >= Duration::from_secs(1))
            {
                if state_changed || running {
                    push_downloads(&notice);
                }
                dl_sent = Some(notice);
                dl_sent_at = Instant::now();
            }
            dl_ran |= running;

            let st = svc.player.state();
            let Some(cur) = st.current().cloned() else {
                if shown.take().is_some() {
                    bridge::stop_playback();
                }
                continue;
            };
            if st.status == Status::Stopped {
                if shown.take().is_some() {
                    bridge::stop_playback();
                }
                continue;
            }
            let playing = matches!(st.status, Status::Playing | Status::Loading);
            let keep_focus = POLICY
                .lock()
                .map(|mut p| {
                    p.observe(playing);
                    p.keep_focus()
                })
                .unwrap_or(false);
            let track = svc.library().get(&cur.id).cloned();
            let art_path = match &track {
                Some(t) if t.has_cover => {
                    let _ = svc.big_cover(&t.id); // makes sure the file gets created
                    let p = svc
                        .paths
                        .data
                        .join("covers")
                        .join(format!("{}-720.jpg", t.id));
                    if p.exists() {
                        p.to_string_lossy().into_owned()
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };
            let now = NowPlaying {
                title: track.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| {
                    cur.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string()
                }),
                artist: track.as_ref().map(|t| t.artist.clone()).unwrap_or_default(),
                album: track.as_ref().map(|t| t.album.clone()).unwrap_or_default(),
                art_path,
                playing,
                keep_focus,
                position_ms: 0,
                duration_ms: (st.duration * 1000.0) as i64,
            };
            // Resend when something visible changed, after a seek, or every
            // 10 s so the lock-screen position stays accurate.
            let expected = last_pos
                + if now.playing {
                    last_sent.elapsed().as_secs_f64()
                } else {
                    0.0
                };
            let jumped = (st.position - expected).abs() > 2.0;
            if shown.as_ref() != Some(&now)
                || jumped
                || last_sent.elapsed() > Duration::from_secs(10)
            {
                bridge::update_playback(&NowPlaying {
                    position_ms: (st.position * 1000.0) as i64,
                    ..now.clone()
                });
                shown = Some(now);
                last_sent = Instant::now();
                last_pos = st.position;
            }
        }
    }
}

/// Applies a command from the notification, lock screen, headset or Bluetooth
/// buttons, or the downloads notification.
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // desktop media keys come with M9-08
pub fn apply(svc: &Services, cmd: loss_platform::MediaCommand) {
    use loss_audio::Command;
    use loss_platform::MediaCommand as M;
    let p = &svc.player;
    match cmd {
        M::Play => p.send(Command::Play),
        M::Pause => p.send(Command::Pause),
        M::Toggle => p.send(Command::Toggle),
        M::Next => p.send(Command::Next),
        M::Previous => p.send(Command::Previous),
        M::Seek(secs) => p.send(Command::Seek(secs)),
        M::PauseAllDownloads => svc.downloads.pause_all(),
        M::ResumeAllDownloads => svc.downloads.resume_all(),
    }
}

pub fn start(svc: &'static Services) {
    #[cfg(target_os = "android")]
    {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            imp::start_events(svc);
            std::thread::Builder::new()
                .name("loss-media".into())
                .spawn(move || imp::run(svc))
                .expect("media thread");
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = svc;
}
