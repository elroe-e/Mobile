//! Music haptics (M2-19): while a local song plays, the phone vibrates to its beat. The song is
//! analysed once on a background thread (`loss_audio::haptic_timeline`); every 150 ms the part
//! about to be heard goes to the vibrator through the glue. Android only.

use crate::services::Services;

#[cfg(target_os = "android")]
mod imp {
    use crate::services::Services;
    use loss_audio::haptics::Timeline;
    use loss_audio::Status;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const TICK: Duration = Duration::from_millis(150);
    /// How much of the plan each hand-over covers (the next one replaces it).
    const CHUNK_MS: u32 = 400;
    /// The vibrator starts a little after it is asked to.
    const LEAD_SECS: f64 = 0.03;

    type Key = (i64, i64);
    /// A finished analysis: song id, the settings it was made for, and the plan (None if the
    /// file couldn't be read).
    type Done = (String, Key, Option<Timeline>);

    pub fn run(svc: &'static Services) {
        let slot: Arc<Mutex<Option<Done>>> = Arc::default();
        let busy = Arc::new(AtomicBool::new(false));
        let mut plan: Option<Done> = None;
        let mut vibrating = false;
        loop {
            std::thread::sleep(TICK);
            let s = svc.settings();
            let st = svc.player.state();
            if !(s.music_haptics && st.status == Status::Playing) {
                if std::mem::take(&mut vibrating) {
                    crate::platform::vibrate(&[]);
                }
                continue;
            }
            let Some(cur) = st.current().cloned() else {
                continue;
            };
            let key: Key = (s.music_haptics_sensitivity, s.music_haptics_strength);
            if let Some(done) = slot.lock().ok().and_then(|mut g| g.take()) {
                plan = Some(done);
            }
            let fresh = plan.as_ref().is_some_and(|p| p.0 == cur.id && p.1 == key);
            if !fresh {
                // Local files only (streamed songs have no file to analyse).
                if cur.path.exists() && !busy.swap(true, Ordering::AcqRel) {
                    let (slot, busy) = (slot.clone(), busy.clone());
                    let (id, item) = (cur.id.clone(), cur.clone());
                    let _ = std::thread::Builder::new()
                        .name("loss-haptics-analysis".into())
                        .spawn(move || {
                            let tl = loss_audio::haptic_timeline(
                                &item,
                                key.0 as f64 / 100.0,
                                key.1 as f64 / 100.0,
                            );
                            if let Ok(mut g) = slot.lock() {
                                *g = Some((id, key, tl));
                            }
                            busy.store(false, Ordering::Release);
                        });
                }
                if std::mem::take(&mut vibrating) {
                    crate::platform::vibrate(&[]);
                }
                continue;
            }
            let Some(Some(tl)) = plan.as_ref().map(|p| &p.2) else {
                continue; // unreadable file: no haptics for this song
            };
            let steps = tl.chunk(st.position + LEAD_SECS, CHUNK_MS);
            crate::platform::vibrate(&steps);
            vibrating = true;
        }
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn start(svc: &'static Services) {
    #[cfg(target_os = "android")]
    {
        std::thread::Builder::new()
            .name("loss-haptics".into())
            .spawn(move || imp::run(svc))
            .expect("haptics thread");
    }
    #[cfg(not(target_os = "android"))]
    let _ = svc;
}
