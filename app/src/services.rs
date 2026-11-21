//! App-wide services: the player, the download manager, the library and
//! settings. Created once at startup and shared by every screen.

use crate::platform::{self, Paths};
use base64::Engine as _;
use loss_audio::Player;
use loss_core::{Library, Settings, TrackOrigin};
use loss_download::{DownloadManager, ManagerConfig};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

pub struct Services {
    pub paths: Paths,
    pub player: Player,
    pub downloads: DownloadManager,
    library: Mutex<Library>,
    settings: Mutex<Settings>,
    /// Bumped whenever the library changes, so the UI knows to refresh.
    pub library_version: AtomicU64,
    pub scanning: AtomicBool,
    /// Phone media volume (0..1) as f32 bits; NaN where not available.
    pub system_volume: std::sync::atomic::AtomicU32,
    pub scan_progress: AtomicU64,
    covers: Covers,
}

static SERVICES: OnceLock<Services> = OnceLock::new();

pub fn get() -> &'static Services {
    SERVICES.get_or_init(Services::init)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Services {
    fn init() -> Services {
        let paths = platform::paths();
        let _ = std::fs::create_dir_all(&paths.data);
        crate::diag::set_dir(&paths.data.join("logs"));
        let settings_path = paths.data.join("settings.json");
        let settings = Settings::load(&settings_path).unwrap_or_else(|| {
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
            let _ = s.save(&settings_path);
            s
        });
        let _ = std::fs::create_dir_all(&settings.download_dir);
        crate::diag::set_detailed(settings.detailed_logging);
        log::info!(
            "Loss {} starting; data in {}",
            env!("CARGO_PKG_VERSION"),
            paths.data.display()
        );
        let library = Library::load(&paths.data.join("library.json"));
        let follows_system = crate::platform::system_volume();
        let player = Player::new(if follows_system.is_some() {
            1.0
        } else {
            settings.volume
        });
        let downloads =
            DownloadManager::new(manager_config(&settings), paths.data.join("downloads.json"));
        let covers = Covers::new(paths.data.join("covers"));
        Services {
            paths,
            player,
            downloads,
            library: Mutex::new(library),
            settings: Mutex::new(settings),
            library_version: AtomicU64::new(1),
            scanning: AtomicBool::new(false),
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

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn update_settings(&self, f: impl FnOnce(&mut Settings)) {
        let s = {
            let mut s = lock(&self.settings);
            f(&mut s);
            s.clone()
        };
        let _ = s.save(&self.paths.data.join("settings.json"));
        let _ = std::fs::create_dir_all(&s.download_dir);
        self.downloads.set_config(manager_config(&s));
    }

    pub fn save_library(&self) {
        let lib = self.library().clone();
        if let Err(e) = lib.save(&self.paths.data.join("library.json")) {
            log::error!("saving library: {e}");
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
        {
            let mut lib = self.library();
            for p in &done {
                if let Err(e) = lib.add_file(p, TrackOrigin::Downloaded) {
                    log::warn!("adding {}: {e}", p.display());
                }
            }
            for id in &plays {
                lib.record_play(id);
            }
        }
        self.save_library();
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
