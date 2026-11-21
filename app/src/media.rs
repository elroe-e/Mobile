//! Keeps Android's media notification, lock screen and Bluetooth/headset
//! buttons in sync with the player. Runs on its own thread so it keeps
//! working while the app is in the background.
//!
//! The Android side lives in `app/android/MainActivity.kt` (LossBridge and
//! LossPlaybackService). On other platforms this does nothing.

use crate::services::Services;

#[cfg(target_os = "android")]
mod imp {
    use crate::platform::android::with_env;
    use crate::services::Services;
    use jni::objects::{JString, JValue};
    use loss_audio::Status;
    use std::time::{Duration, Instant};

    fn poll_commands() -> Vec<String> {
        with_env(|env, act| {
            let cls = crate::platform::android::bridge_class(env, act)?;
            let s = env
                .call_static_method(&cls, "pollCommands", "()Ljava/lang/String;", &[])?
                .l()?;
            let s: String = env.get_string(&JString::from(s))?.into();
            Ok(s)
        })
        .map(|s| {
            s.lines()
                .map(str::to_string)
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
    }

    #[derive(PartialEq, Clone, Default)]
    struct Shown {
        title: String,
        artist: String,
        album: String,
        art: String,
        playing: bool,
    }

    fn update(s: &Shown, position: f64, duration: f64) {
        with_env(|env, act| {
            let cls = crate::platform::android::bridge_class(env, act)?;
            let (t, a, al, art) = (
                env.new_string(&s.title)?,
                env.new_string(&s.artist)?,
                env.new_string(&s.album)?,
                env.new_string(&s.art)?,
            );
            env.call_static_method(
                &cls,
                "update",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;ZJJ)V",
                &[
                    JValue::Object(act),
                    JValue::Object(&t),
                    JValue::Object(&a),
                    JValue::Object(&al),
                    JValue::Object(&art),
                    JValue::Bool(s.playing as u8),
                    JValue::Long((position * 1000.0) as i64),
                    JValue::Long((duration * 1000.0) as i64),
                ],
            )?;
            Ok(())
        });
    }

    fn push_downloads(n: &crate::notify::DownloadsNotice) {
        use crate::notify::DownloadsState;
        let state = match n.state {
            DownloadsState::Running => 1,
            DownloadsState::Paused => 2,
            DownloadsState::Finished | DownloadsState::Idle => 0,
        };
        with_env(|env, act| {
            let cls = crate::platform::android::bridge_class(env, act)?;
            let (t, x) = (env.new_string(&n.title)?, env.new_string(&n.text)?);
            env.call_static_method(
                &cls,
                "downloads",
                "(Landroid/content/Context;ILjava/lang/String;Ljava/lang/String;I)V",
                &[
                    JValue::Object(act),
                    JValue::Int(state),
                    JValue::Object(&t),
                    JValue::Object(&x),
                    JValue::Int(n.progress),
                ],
            )?;
            Ok(())
        });
    }

    fn stop() {
        with_env(|env, act| {
            let cls = crate::platform::android::bridge_class(env, act)?;
            env.call_static_method(&cls, "stop", "()V", &[])?;
            Ok(())
        });
    }

    pub fn run(svc: &'static Services) {
        // Attach once: this thread lives as long as the app.
        let ctx = ndk_context::android_context();
        if let Ok(vm) = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
            let _ = vm.attach_current_thread_permanently();
        }
        let mut shown: Option<Shown> = None;
        // Downloads notification: last sent value, when, and whether any ran.
        let mut dl_sent: Option<crate::notify::DownloadsNotice> = None;
        let mut dl_sent_at = Instant::now();
        let mut dl_ran = false;
        let mut last_sent = Instant::now();
        let mut last_pos = 0.0;
        loop {
            std::thread::sleep(Duration::from_millis(300));
            for cmd in poll_commands() {
                super::apply_command(svc, &cmd);
            }
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
            // Follow the phone's volume (hardware buttons, other apps).
            if let Some(v) = crate::platform::system_volume() {
                svc.system_volume
                    .store(v.to_bits(), std::sync::atomic::Ordering::Relaxed);
            }

            let st = svc.player.state();
            let Some(cur) = st.current().cloned() else {
                if shown.take().is_some() {
                    stop();
                }
                continue;
            };
            if st.status == Status::Stopped {
                if shown.take().is_some() {
                    stop();
                }
                continue;
            }
            let track = svc.library().get(&cur.id).cloned();
            let art = match &track {
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
            let now = Shown {
                title: track.as_ref().map(|t| t.title.clone()).unwrap_or_else(|| {
                    cur.path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string()
                }),
                artist: track.as_ref().map(|t| t.artist.clone()).unwrap_or_default(),
                album: track.as_ref().map(|t| t.album.clone()).unwrap_or_default(),
                art,
                playing: matches!(st.status, Status::Playing | Status::Loading),
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
                update(&now, st.position, st.duration);
                shown = Some(now);
                last_sent = Instant::now();
                last_pos = st.position;
            }
        }
    }
}

/// Applies a command from the notification, lock screen, headset buttons,
/// audio focus or unplug events: play, pause, toggle, next, prev, "seek <ms>".
#[cfg_attr(not(target_os = "android"), allow(dead_code))] // desktop media keys come with M9-08
pub fn apply_command(svc: &Services, cmd: &str) {
    use loss_audio::Command;
    let p = &svc.player;
    match cmd.split_once(' ') {
        Some(("seek", ms)) => {
            if let Ok(ms) = ms.trim().parse::<f64>() {
                p.send(Command::Seek(ms / 1000.0));
            }
        }
        _ => match cmd {
            "play" => p.send(Command::Play),
            "pause" => p.send(Command::Pause),
            "toggle" => p.send(Command::Toggle),
            "next" => p.send(Command::Next),
            "prev" => p.send(Command::Previous),
            "dl_pause_all" => svc.downloads.pause_all(),
            "dl_resume_all" => svc.downloads.resume_all(),
            other => log::warn!("unknown media command {other}"),
        },
    }
}

pub fn start(svc: &'static Services) {
    #[cfg(target_os = "android")]
    {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            std::thread::Builder::new()
                .name("loss-media".into())
                .spawn(move || imp::run(svc))
                .expect("media thread");
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = svc;
}
