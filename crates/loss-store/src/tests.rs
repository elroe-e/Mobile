use super::*;
use loss_core::TrackOrigin;
use std::time::Instant;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let p = std::env::temp_dir().join(format!(
            "loss-store-{name}-{}-{}",
            std::process::id(),
            loss_core::now_secs()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn track(i: usize) -> Track {
    Track {
        id: format!("t{i:06}"),
        path: PathBuf::from(format!(
            "/music/Artist {}/Album {}/{i:02}.flac",
            i % 500,
            i % 4000
        )),
        title: format!("Song {i}"),
        artist: format!("Artist {}", i % 500),
        album: format!("Album {}", i % 4000),
        album_artist: format!("Artist {}", i % 500),
        genre: Some("Rock".into()),
        year: Some(1990 + (i % 30) as u32),
        track_no: Some((i % 12) as u32 + 1),
        disc_no: Some(1),
        duration_secs: 200.0 + (i % 100) as f64,
        has_cover: i.is_multiple_of(3),
        origin: TrackOrigin::Local,
        added_at: 1_700_000_000 + i as u64,
        mtime: 1_700_000_000,
        play_count: (i % 7) as u32,
        format: "FLAC".into(),
        bit_depth: Some(if i.is_multiple_of(5) { 24 } else { 16 }),
        sample_rate: Some(if i.is_multiple_of(5) { 96_000 } else { 44_100 }),
        last_played: 0,
    }
}

#[test]
fn new_database_has_current_schema() {
    let d = TempDir::new("schema");
    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert!(s.tracks().unwrap().is_empty());
    assert_eq!(s.settings().unwrap(), None);
}

#[test]
fn data_survives_reopening() {
    let d = TempDir::new("reopen");
    let path = d.0.join("loss.redb");
    {
        let s = Store::open(&path).unwrap();
        s.put_tracks(&[track(1), track(2)]).unwrap();
        s.put_settings(&Settings {
            theme: "dark".into(),
            ..Default::default()
        })
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let lib = s.library().unwrap();
    assert_eq!(lib.tracks.len(), 2);
    assert_eq!(lib.get("t000002"), Some(&track(2)));
    assert_eq!(s.settings().unwrap().unwrap().theme, "dark");
}

#[test]
fn save_tracks_writes_only_changes() {
    let d = TempDir::new("sync");
    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    let mut tracks: Vec<Track> = (0..3).map(track).collect();
    assert_eq!(
        s.save_tracks(&tracks).unwrap(),
        SyncReport {
            written: 3,
            removed: 0
        }
    );
    // Unchanged: nothing written.
    assert_eq!(s.save_tracks(&tracks).unwrap(), SyncReport::default());
    // One play counted, one track gone.
    tracks[0].play_count += 1;
    tracks.remove(2);
    assert_eq!(
        s.save_tracks(&tracks).unwrap(),
        SyncReport {
            written: 1,
            removed: 1
        }
    );
    let mut stored = s.tracks().unwrap();
    stored.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(stored, tracks);
}

#[test]
fn newer_schema_is_refused_and_can_be_reset() {
    let d = TempDir::new("newer");
    let path = d.0.join("loss.redb");
    {
        let s = Store::open(&path).unwrap();
        s.put_tracks(&[track(1)]).unwrap();
        s.set_kv(KEY_SCHEMA, &(SCHEMA_VERSION + 1)).unwrap();
    }
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::NewerSchema(v)) if v == SCHEMA_VERSION + 1
    ));
    let (s, err) = Store::open_or_reset(&path).unwrap();
    assert!(err.is_some());
    assert!(s.tracks().unwrap().is_empty());
    // The old file is kept next to the new one.
    let kept = std::fs::read_dir(&d.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains("unreadable"));
    assert!(kept);
}

#[test]
fn imports_json_from_older_versions_once() {
    let d = TempDir::new("import");
    // library.json as older Loss versions wrote it: no format, bit depth, rate or last_played.
    let old_track = serde_json::json!({
        "id": "abc", "path": "/music/a.flac", "title": "A", "artist": "X", "album": "Y",
        "album_artist": "X", "genre": null, "year": 2001, "track_no": 1, "disc_no": 1,
        "duration_secs": 180.5, "has_cover": false, "origin": "Local",
        "added_at": 1, "mtime": 2
    });
    std::fs::write(
        d.0.join("library.json"),
        serde_json::to_vec(&serde_json::json!({ "tracks": [old_track] })).unwrap(),
    )
    .unwrap();
    Settings {
        accent: "#123456".into(),
        ..Default::default()
    }
    .save(&d.0.join("settings.json"))
    .unwrap();

    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    let report = s
        .import_legacy(&d.0, |s| s.theme = "prepared".into())
        .unwrap();
    assert_eq!(
        report,
        ImportReport {
            tracks: 1,
            settings: true
        }
    );
    let t = &s.tracks().unwrap()[0];
    assert_eq!(
        (t.title.as_str(), t.play_count, t.format.as_str()),
        ("A", 0, "")
    );
    let settings = s.settings().unwrap().unwrap();
    assert_eq!(
        (settings.accent.as_str(), settings.theme.as_str()),
        ("#123456", "prepared")
    );
    // The files are kept as backups, and a second import does nothing.
    assert!(d.0.join("library.json.imported").exists());
    assert!(!d.0.join("library.json").exists());
    assert_eq!(
        s.import_legacy(&d.0, |_| {}).unwrap(),
        ImportReport::default()
    );
}

#[test]
fn unreadable_json_is_left_alone() {
    let d = TempDir::new("badjson");
    std::fs::write(d.0.join("library.json"), b"{ not json").unwrap();
    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    assert_eq!(
        s.import_legacy(&d.0, |_| {}).unwrap(),
        ImportReport::default()
    );
    assert!(d.0.join("library.json").exists());
}

/// M0-07's target: 50,000 tracks load in under a second (release build on a desktop).
/// `cargo test -p loss-store --release -- --ignored`
#[test]
#[ignore]
fn loads_50k_tracks_quickly() {
    let d = TempDir::new("50k");
    let path = d.0.join("loss.redb");
    {
        let s = Store::open(&path).unwrap();
        let tracks: Vec<Track> = (0..50_000).map(track).collect();
        s.save_tracks(&tracks).unwrap();
    }
    let start = Instant::now();
    let s = Store::open(&path).unwrap();
    let lib = s.library().unwrap();
    let took = start.elapsed();
    eprintln!("50k tracks: open + load + index in {took:?}");
    assert_eq!(lib.tracks.len(), 50_000);
    assert!(lib.get("t049999").is_some());
    assert!(took.as_secs_f64() < 1.0, "took {took:?}");
}

#[test]
fn settings_are_one_row_per_setting() {
    let d = TempDir::new("perkey");
    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    let mut settings = Settings {
        theme: "light".into(),
        ..Default::default()
    };
    s.put_settings(&settings).unwrap();
    assert_eq!(
        s.setting("theme").unwrap(),
        Some(serde_json::json!("light"))
    );
    settings.auto_tag = false;
    s.put_settings(&settings).unwrap();
    assert_eq!(s.settings().unwrap(), Some(settings));
    assert_eq!(
        s.setting("auto_tag").unwrap(),
        Some(serde_json::json!(false))
    );
}

#[test]
fn a_bad_setting_value_falls_back_to_its_default() {
    let d = TempDir::new("badsetting");
    let s = Store::open(&d.0.join("loss.redb")).unwrap();
    s.put_settings(&Settings {
        theme: "dark".into(),
        ..Default::default()
    })
    .unwrap();
    {
        let txn = s.db.begin_write().unwrap();
        txn.open_table(SETTINGS)
            .unwrap()
            .insert("max_retries", b"\"lots\"".as_slice())
            .unwrap();
        txn.commit().unwrap();
    }
    let got = s.settings().unwrap().unwrap();
    assert_eq!(got.theme, "dark");
    assert_eq!(got.max_retries, Settings::default().max_retries);
}

#[test]
fn schema_1_settings_row_is_split_into_keys() {
    let d = TempDir::new("migrate12");
    let path = d.0.join("loss.redb");
    {
        // Build a schema-1 database: everything in one "app" row.
        let s = Store::open(&path).unwrap();
        let old = Settings {
            accent: "#7c8cff".into(),
            max_retries: 2,
            ..Default::default()
        };
        let txn = s.db.begin_write().unwrap();
        {
            let mut t = txn.open_table(SETTINGS).unwrap();
            let keys: Vec<String> = t
                .iter()
                .unwrap()
                .map(|r| r.unwrap().0.value().to_string())
                .collect();
            for k in keys {
                t.remove(k.as_str()).unwrap();
            }
            t.insert(LEGACY_SETTINGS_ROW, to_json(&old).unwrap().as_slice())
                .unwrap();
        }
        txn.open_table(KV)
            .unwrap()
            .insert(KEY_SCHEMA, b"1".as_slice())
            .unwrap();
        txn.commit().unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(s.setting(LEGACY_SETTINGS_ROW).unwrap(), None);
    assert_eq!(
        s.setting("accent").unwrap(),
        Some(serde_json::json!("#7c8cff"))
    );
    let got = s.settings().unwrap().unwrap();
    assert_eq!((got.accent.as_str(), got.max_retries), ("#7c8cff", 2));
}
