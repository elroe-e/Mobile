//! App-wide services: the player, the download manager, the library and
//! settings. Created once at startup and shared by every screen.

use crate::platform::{self, Paths};
use base64::Engine as _;
use loss_audio::Player;
use loss_core::vault::{names, Vault};
use loss_core::{Library, Settings, TrackOrigin};
use loss_download::{DownloadManager, ManagerConfig};
use loss_store::Store;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub struct Services {
    pub paths: Paths,
    pub player: Player,
    pub downloads: DownloadManager,
    library: Mutex<Library>,
    /// The database (`loss.redb`); None only if the data folder can't be written.
    store: Option<Store>,
    /// Encrypted secrets; None if the platform couldn't provide a key.
    vault: Mutex<Option<Vault>>,
    /// Fallback when there is no vault: secrets kept only while the app runs.
    memory_secrets: Mutex<HashMap<String, String>>,
    settings: Mutex<Settings>,
    /// Bumped whenever the library changes, so the UI knows to refresh.
    pub library_version: AtomicU64,
    pub scanning: AtomicBool,
    /// Phone media volume (0..1) as f32 bits; NaN where not available.
    pub system_volume: std::sync::atomic::AtomicU32,
    /// The output music plays through (Android), for display and the engine's rate choice.
    pub output: Mutex<Option<loss_platform::route::Device>>,
    pub scan_progress: AtomicU64,
    covers: Covers,
}

static SERVICES: OnceLock<Services> = OnceLock::new();

pub fn get() -> &'static Services {
    SERVICES.get_or_init(Services::init)
}

/// The engine's conversion options from settings.
fn audio_options(s: &Settings) -> loss_audio::AudioOptions {
    loss_audio::AudioOptions {
        quality: loss_audio::Quality::parse(&s.resample_quality),
        max_rate: s.max_sample_rate,
    }
}

fn save_settings(store: Option<&Store>, s: &Settings) {
    if let Some(Err(e)) = store.map(|st| st.put_settings(s)) {
        log::error!("saving settings: {e}");
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Services {
    fn init() -> Services {
        let paths = platform::paths();
        let _ = std::fs::create_dir_all(&paths.data);
        crate::diag::set_dir(&paths.data.join("logs"));
        let mut vault = platform::vault_key(&paths.data)
            .map(|key| Vault::open(&paths.data.join("vault.bin"), key));
        if vault.is_none() {
            log::warn!("encrypted storage unavailable; passwords last until the app closes");
        }
        let store = match Store::open_or_reset(&paths.data.join("loss.redb")) {
            Ok((store, reset)) => {
                if let Some(e) = reset {
                    log::error!("started a new database: {e}");
                }
                // Library and settings from versions before the database (once).
                // Passwords go to the vault before the settings are written.
                let import = store.import_legacy(&paths.data, |s| {
                    move_secrets(s, vault.as_mut());
                });
                if let Err(e) = import {
                    log::error!("importing old data: {e}");
                }
                Some(store)
            }
            Err(e) => {
                log::error!("no database, nothing will be saved: {e}");
                None
            }
        };
        let stored = store.as_ref().and_then(|s| {
            s.settings()
                .map_err(|e| log::error!("reading settings: {e}"))
                .ok()
                .flatten()
        });
        let mut settings = stored.unwrap_or_else(|| {
            let s = Settings {
                download_dir: paths.downloads.clone(),
                music_folders: paths
                    .music_suggestions
                    .iter()
                    .filter(|p| p.is_dir())
                    .take(1)
                    .cloned()
                    .collect(),
                ..Default::default()
            };
            save_settings(store.as_ref(), &s);
            s
        });
        // Settings from older versions may have no download folder.
        if settings.download_dir.as_os_str().is_empty() {
            settings.download_dir = paths.downloads.clone();
            save_settings(store.as_ref(), &settings);
        }
        let _ = std::fs::create_dir_all(&settings.download_dir);
        crate::diag::set_detailed(settings.detailed_logging);
        loss_i18n::set_language(&settings.language, platform::system_locale().as_deref());

        // Secrets live in the encrypted vault, never in the database.
        if move_secrets(&mut settings, vault.as_mut()) {
            save_settings(store.as_ref(), &settings);
        }
        log::info!(
            "Loss {} starting; data in {}",
            env!("CARGO_PKG_VERSION"),
            paths.data.display()
        );
        let started = std::time::Instant::now();
        let library = store
            .as_ref()
            .and_then(|s| {
                s.library()
                    .map_err(|e| log::error!("reading library: {e}"))
                    .ok()
            })
            .unwrap_or_default();
        log::info!(
            "library: {} songs loaded in {} ms",
            library.tracks.len(),
            started.elapsed().as_millis()
        );
        let follows_system = crate::platform::system_volume();
        let player = Player::new(if follows_system.is_some() {
            1.0
        } else {
            settings.volume
        });
        player.send(loss_audio::Command::SetAudioOptions(audio_options(
            &settings,
        )));
        let downloads =
            DownloadManager::new(manager_config(&settings), paths.data.join("downloads.json"));
        let covers = Covers::new(paths.data.join("covers"));
        Services {
            paths,
            player,
            downloads,
            library: Mutex::new(library),
            store,
            settings: Mutex::new(settings),
            vault: Mutex::new(vault),
            memory_secrets: Mutex::new(Default::default()),
            library_version: AtomicU64::new(1),
            scanning: AtomicBool::new(false),
            output: Mutex::new(None),
            system_volume: std::sync::atomic::AtomicU32::new(
                follows_system.unwrap_or(f32::NAN).to_bits(),
            ),
            scan_progress: AtomicU64::new(0),
            covers,
        }
    }

    pub fn library(&self) -> MutexGuard<'_, Library> {
        lock(&self.library)
    }

    /// Settings with secrets filled in from the vault.
    pub fn settings(&self) -> Settings {
        let mut s = lock(&self.settings).clone();
        if let Some(sub) = s.subsonic.as_mut() {
            sub.password = self.secret(names::SUBSONIC_PASSWORD).unwrap_or_default();
        }
        s
    }

    fn secret(&self, name: &str) -> Option<String> {
        match lock(&self.vault).as_ref() {
            Some(v) => v.get(name).map(str::to_string),
            None => lock(&self.memory_secrets).get(name).cloned(),
        }
    }

    /// Changes settings and saves them. Secrets set by `f` go to the vault,
    /// never to the database.
    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        let s = {
            let mut s = lock(&self.settings);
            f(&mut s);
            let mut vault = lock(&self.vault);
            if vault.is_some() {
                move_secrets(&mut s, vault.as_mut());
            } else {
                let mut mem = lock(&self.memory_secrets);
                match s.subsonic.as_mut() {
                    Some(sub) if !sub.password.is_empty() => {
                        mem.insert(
                            names::SUBSONIC_PASSWORD.into(),
                            std::mem::take(&mut sub.password),
                        );
                    }
                    Some(_) => {}
                    None => {
                        mem.remove(names::SUBSONIC_PASSWORD);
                    }
                }
            }
            s.clone()
        };
        save_settings(self.store.as_ref(), &s);
        let _ = std::fs::create_dir_all(&s.download_dir);
        self.player
            .send(loss_audio::Command::SetAudioOptions(audio_options(&s)));
        self.downloads.set_config(manager_config(&s));
    }

    /// Reads the outputs again and tells the engine what the active one takes, so Loss converts
    /// sample rates itself (M2-03). Called at start and whenever outputs come or go.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub fn refresh_route(&self) {
        let Some(outputs) = platform::outputs() else {
            return;
        };
        let active = outputs.active().cloned();
        let caps = active.as_ref().and_then(|d| {
            (!d.rates.is_empty()).then(|| loss_audio::sink::Caps {
                rates: d.rates.clone(),
                any_rate: false,
                channels: if d.channels.is_empty() || d.channels.contains(&2) {
                    2
                } else {
                    d.channels[0] as usize
                },
            })
        });
        match &active {
            Some(d) => log::info!("output: {} ({:?}), rates {:?}", d.name, d.kind(), d.rates),
            None => log::info!("output: unknown"),
        }
        self.player.send(loss_audio::Command::SetOutputCaps(caps));
        *lock(&self.output) = active;
    }

    /// Saves the whole library (after a scan): only changed songs are written.
    pub fn save_library(&self) {
        let Some(store) = &self.store else { return };
        let tracks = self.library().tracks.clone();
        match store.save_tracks(&tracks) {
            Ok(r) => log::debug!("library saved: {r:?}"),
            Err(e) => log::error!("saving library: {e}"),
        }
    }

    /// Saves these songs only (a play counted, a download added).
    fn save_tracks(&self, ids: &[String]) {
        let Some(store) = &self.store else { return };
        let tracks: Vec<loss_core::Track> = {
            let lib = self.library();
            ids.iter().filter_map(|id| lib.get(id).cloned()).collect()
        };
        if let Err(e) = store.put_tracks(&tracks) {
            log::error!("saving songs: {e}");
        }
    }

    fn bump(&self) {
        self.library_version.fetch_add(1, Ordering::SeqCst);
    }

    /// Rescans all music folders and the download folder in the background.
    pub fn rescan(&'static self) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(move || {
            let s = self.settings();
            let mut lib = self.library().clone();
            let report = lib.scan(&s.music_folders, Some(&s.download_dir), |n| {
                self.scan_progress.store(n as u64, Ordering::Relaxed);
            });
            log::info!("scan: {report:?}");
            *self.library() = lib;
            self.save_library();
            self.scanning.store(false, Ordering::SeqCst);
            self.bump();
        });
    }

    /// Called from the UI tick: adds finished downloads to the library and
    /// records plays.
    pub fn sync(&self) {
        let done = self.downloads.take_completed();
        let plays = self.player.take_play_events();
        if done.is_empty() && plays.is_empty() {
            return;
        }
        let mut changed = Vec::new();
        {
            let mut lib = self.library();
            for p in &done {
                match lib.add_file(p, TrackOrigin::Downloaded) {
                    Ok(id) => changed.push(id),
                    Err(e) => log::warn!("adding {}: {e}", p.display()),
                }
            }
            for id in &plays {
                lib.record_play(id);
                changed.push(id.clone());
            }
        }
        self.save_tracks(&changed);
        self.bump();
    }

    /// Small cover for lists (data: URI), loaded in the background.
    pub fn thumb(&self, track_id: &str) -> Option<String> {
        self.cover(track_id, 240)
    }

    /// Large cover for Now Playing.
    pub fn big_cover(&self, track_id: &str) -> Option<String> {
        self.cover(track_id, 720)
    }

    fn cover(&self, track_id: &str, size: u32) -> Option<String> {
        let path = {
            let lib = self.library();
            let t = lib.get(track_id)?;
            if !t.has_cover {
                return None;
            }
            t.path.clone()
        };
        self.covers.get(track_id, &path, size)
    }

    /// Main colour of a track's cover (loads the cover in the background
    /// the first time; returns None until it's ready).
    pub fn cover_color(&self, track_id: &str) -> Option<[u8; 3]> {
        if let Some(c) = lock(&self.covers.colors).get(track_id) {
            return Some(*c);
        }
        let _ = self.thumb(track_id);
        None
    }

    /// The phone's media volume if the app follows it (Android).
    pub fn system_volume(&self) -> Option<f32> {
        let v = f32::from_bits(self.system_volume.load(Ordering::Relaxed));
        (!v.is_nan()).then_some(v)
    }

    /// Volume slider: on Android this is the phone's media volume (same as
    /// the volume buttons); elsewhere it's the app's own volume.
    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        if self.system_volume().is_some() {
            crate::platform::set_system_volume(v);
            self.system_volume.store(v.to_bits(), Ordering::Relaxed);
        } else {
            self.player.send(loss_audio::Command::SetVolume(v));
        }
    }

    pub fn covers_version(&self) -> u64 {
        self.covers.version.load(Ordering::Relaxed)
    }

    /// Lyrics for a track: a .lrc next to the file, or lyrics in the tags.
    pub fn local_lyrics(&self, path: &Path) -> Option<String> {
        for ext in ["lrc", "txt"] {
            if let Ok(s) = std::fs::read_to_string(path.with_extension(ext)) {
                if !s.trim().is_empty() {
                    return Some(s);
                }
            }
        }
        if let Ok(s) = std::fs::read_to_string(self.lyrics_cache_path(path)) {
            return Some(s);
        }
        loss_core::tags::read_meta(path).ok()?.embedded_lyrics
    }

    /// Where fetched lyrics are cached when the music folder isn't writable.
    pub fn lyrics_cache_path(&self, path: &Path) -> PathBuf {
        self.paths.data.join("lyrics").join(format!(
            "{}.lrc",
            loss_core::stable_id(&path.to_string_lossy())
        ))
    }
}

/// Moves secrets out of `s` into the vault. Removing the server removes its
/// password. Returns whether `s` changed.
fn move_secrets(s: &mut Settings, vault: Option<&mut Vault>) -> bool {
    let Some(vault) = vault else { return false };
    match s.subsonic.as_mut() {
        Some(sub) if !sub.password.is_empty() => {
            if let Err(e) = vault.set(names::SUBSONIC_PASSWORD, &sub.password) {
                log::error!("can't store the server password: {e}");
                return false;
            }
            sub.password.clear();
            true
        }
        Some(_) => false,
        None => {
            if vault.get(names::SUBSONIC_PASSWORD).is_some() {
                let _ = vault.set(names::SUBSONIC_PASSWORD, "");
            }
            false
        }
    }
}

fn manager_config(s: &Settings) -> ManagerConfig {
    ManagerConfig {
        dir: s.download_dir.clone(),
        max_parallel: s.max_parallel_downloads.clamp(1, 6),
        max_retries: s.max_retries,
        auto_tag: s.auto_tag,
        fetch_lyrics: s.fetch_lyrics,
        extract_zips: s.extract_zips,
    }
}

/// Cover art cache: resized JPEGs on disk + data URIs in memory, generated
/// on a background thread so scrolling never waits on tag parsing.
/// Cover data URIs by (track id, size); `None` means the track has no cover.
type CoverMap = Arc<Mutex<HashMap<(String, u32), Option<String>>>>;

struct Covers {
    mem: CoverMap,
    /// Main colour of each cover, for dynamic theming.
    colors: Arc<Mutex<HashMap<String, [u8; 3]>>>,
    pending: Arc<Mutex<HashSet<(String, u32)>>>,
    tx: std::sync::mpsc::Sender<(String, PathBuf, u32)>,
    version: Arc<AtomicU64>,
}

const MEM_LIMIT: usize = 400;

impl Covers {
    fn new(dir: PathBuf) -> Covers {
        let _ = std::fs::create_dir_all(&dir);
        let (tx, rx) = std::sync::mpsc::channel::<(String, PathBuf, u32)>();
        let mem: CoverMap = Default::default();
        let order: Arc<Mutex<VecDeque<(String, u32)>>> = Default::default();
        let pending: Arc<Mutex<HashSet<(String, u32)>>> = Default::default();
        let version = Arc::new(AtomicU64::new(0));
        let colors: Arc<Mutex<HashMap<String, [u8; 3]>>> = Default::default();
        let (m, o, p, v, d, c) = (
            mem.clone(),
            order,
            pending.clone(),
            version.clone(),
            dir,
            colors.clone(),
        );
        std::thread::Builder::new()
            .name("loss-covers".into())
            .spawn(move || {
                while let Ok((id, path, size)) = rx.recv() {
                    let need_color = !lock(&c).contains_key(&id);
                    let (uri, color) = load_cover(&d, &id, &path, size, need_color);
                    if let Some(col) = color {
                        lock(&c).insert(id.clone(), col);
                    }
                    let key = (id, size);
                    {
                        let mut mem = lock(&m);
                        let mut order = lock(&o);
                        mem.insert(key.clone(), uri);
                        order.push_back(key.clone());
                        while order.len() > MEM_LIMIT {
                            if let Some(old) = order.pop_front() {
                                mem.remove(&old);
                            }
                        }
                    }
                    lock(&p).remove(&key);
                    v.fetch_add(1, Ordering::Relaxed);
                }
            })
            .expect("cover thread");
        Covers {
            mem,
            colors,
            pending,
            tx,
            version,
        }
    }

    fn get(&self, id: &str, path: &Path, size: u32) -> Option<String> {
        let key = (id.to_string(), size);
        if let Some(v) = lock(&self.mem).get(&key) {
            return v.clone();
        }
        if lock(&self.pending).insert(key) {
            let _ = self.tx.send((id.to_string(), path.to_path_buf(), size));
        }
        None
    }
}

fn load_cover(
    dir: &Path,
    id: &str,
    path: &Path,
    size: u32,
    want_color: bool,
) -> (Option<String>, Option<[u8; 3]>) {
    let cache = dir.join(format!("{id}-{size}.jpg"));
    let bytes = match std::fs::read(&cache) {
        Ok(b) => b,
        Err(_) => {
            let Some(raw) = loss_core::tags::read_cover(path)
                .map(|(b, _)| b)
                .or_else(|| {
                    loss_core::tags::find_folder_cover(path).and_then(|p| std::fs::read(p).ok())
                })
            else {
                return (None, None);
            };
            let Ok(img) = image::load_from_memory(&raw) else {
                return (None, None);
            };
            let thumb = img.thumbnail(size, size).to_rgb8();
            let mut out = Vec::new();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
            if enc.encode_image(&thumb).is_err() {
                return (None, None);
            }
            let _ = std::fs::write(&cache, &out);
            out
        }
    };
    let color = if want_color {
        image::load_from_memory(&bytes)
            .ok()
            .map(|img| crate::ui::theme::seed_from_image(&img))
    } else {
        None
    };
    let uri = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    (Some(uri), color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loss_core::SubsonicConfig;

    #[test]
    fn passwords_move_to_the_vault_and_never_reach_the_database() {
        let dir = std::env::temp_dir().join(format!("loss-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut vault = Vault::open(&dir.join("vault.bin"), loss_core::vault::new_key().unwrap());
        let mut s = Settings {
            subsonic: Some(SubsonicConfig {
                url: "https://music.example".into(),
                username: "me".into(),
                password: "hunter2".into(),
            }),
            ..Default::default()
        };
        assert!(move_secrets(&mut s, Some(&mut vault)));
        assert_eq!(vault.get(names::SUBSONIC_PASSWORD), Some("hunter2"));
        let store = Store::open(&dir.join("loss.redb")).unwrap();
        save_settings(Some(&store), &s);
        drop(store);
        let bytes = std::fs::read(dir.join("loss.redb")).unwrap();
        let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(!has(b"hunter2") && !has(b"password"));

        // Removing the server removes its password.
        s.subsonic = None;
        move_secrets(&mut s, Some(&mut vault));
        assert_eq!(vault.get(names::SUBSONIC_PASSWORD), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
