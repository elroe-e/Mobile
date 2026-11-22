//! Loss' database: one redb file (`loss.redb`) in the app's data folder.
//!
//! Values are JSON so that adding a field never breaks an existing database (new fields use
//! `#[serde(default)]`). The schema version lives in the `kv` table; [`Store::open`] runs any
//! migrations it needs, in order, and refuses a database written by a newer Loss.
//!
//! Tables so far (ARCHITECTURE §5 lists the rest; later tasks add them as migrations):
//!
//! | table | key → value |
//! |---|---|
//! | `tracks` | track id → [`Track`] |
//! | `settings` | setting key (a [`Settings`] field name, see `loss_core::registry`) → its JSON value |
//! | `kv` | name → small values (schema version, import marker) |

use loss_core::{Library, Settings, Track};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The schema this build writes. Bump it and add a step to [`MIGRATIONS`] when tables change.
pub const SCHEMA_VERSION: u32 = 2;

const TRACKS: TableDefinition<&str, &[u8]> = TableDefinition::new("tracks");
const SETTINGS: TableDefinition<&str, &[u8]> = TableDefinition::new("settings");
const KV: TableDefinition<&str, &[u8]> = TableDefinition::new("kv");

const KEY_SCHEMA: &str = "schema_version";
const KEY_IMPORTED: &str = "imported_json";
/// Schema 1 kept all settings in this one row.
const LEGACY_SETTINGS_ROW: &str = "app";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] redb::Error),
    #[error("stored value: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error(
        "this database was written by a newer Loss (schema {0}, this build knows {SCHEMA_VERSION})"
    )]
    NewerSchema(u32),
}

macro_rules! from_redb {
    ($($t:ty),*) => {$(
        impl From<$t> for StoreError {
            fn from(e: $t) -> Self {
                StoreError::Db(e.into())
            }
        }
    )*};
}
from_redb!(
    redb::DatabaseError,
    redb::TransactionError,
    redb::TableError,
    redb::StorageError,
    redb::CommitError
);

pub type Result<T> = std::result::Result<T, StoreError>;

/// One migration step: brings the database from version `n` to `n + 1`.
type Migration = fn(&redb::WriteTransaction) -> Result<()>;

/// `MIGRATIONS[n]` upgrades schema `n` to `n + 1`.
const MIGRATIONS: &[Migration] = &[
    // 0 → 1: the first tables.
    |txn| {
        txn.open_table(TRACKS)?;
        txn.open_table(SETTINGS)?;
        txn.open_table(KV)?;
        Ok(())
    },
    // 1 → 2: settings move from one row to one row per setting.
    |txn| {
        let mut t = txn.open_table(SETTINGS)?;
        let old = t.remove(LEGACY_SETTINGS_ROW)?.map(|g| g.value().to_vec());
        if let Some(bytes) = old {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(serde_json::Value::Object(map)) => {
                    for (k, v) in map {
                        t.insert(k.as_str(), to_json(&v)?.as_slice())?;
                    }
                }
                // Unreadable: drop it; the app falls back to default settings.
                _ => log::error!("database: settings row unreadable, using defaults"),
            }
        }
        Ok(())
    },
];

/// What [`Store::import_legacy`] brought over from the JSON files older versions wrote.
#[derive(Debug, Default, PartialEq)]
pub struct ImportReport {
    pub tracks: usize,
    pub settings: bool,
}

/// What [`Store::save_tracks`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct SyncReport {
    pub written: usize,
    pub removed: usize,
}

pub struct Store {
    db: Database,
    path: PathBuf,
}

fn to_json<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(v)?)
}

fn from_json<T: DeserializeOwned>(b: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(b)?)
}

impl Store {
    /// Opens (or creates) the database and brings it to [`SCHEMA_VERSION`].
    pub fn open(path: &Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Database::create(path)?;
        let store = Store {
            db,
            path: path.to_path_buf(),
        };
        store.migrate()?;
        Ok(store)
    }

    /// Like [`Store::open`], but a database that can't be opened (damaged, or from a newer Loss)
    /// is moved aside as `<name>.unreadable-<time>` and a fresh one is created, so the app always
    /// starts. Returns the store and the error that caused the reset, if any.
    pub fn open_or_reset(path: &Path) -> Result<(Store, Option<StoreError>)> {
        match Store::open(path) {
            Ok(s) => Ok((s, None)),
            Err(e) => {
                let aside =
                    path.with_extension(format!("redb.unreadable-{}", loss_core::now_secs()));
                log::error!(
                    "database unreadable ({e}); moving it to {}",
                    aside.display()
                );
                std::fs::rename(path, &aside)?;
                Ok((Store::open(path)?, Some(e)))
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        let mut version = {
            let kv = txn.open_table(KV)?;
            let v = kv.get(KEY_SCHEMA)?.map(|g| from_json::<u32>(g.value()));
            v.transpose()?.unwrap_or(0)
        };
        if version > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema(version));
        }
        while (version as usize) < MIGRATIONS.len() {
            log::info!("database: migrating schema {version} → {}", version + 1);
            MIGRATIONS[version as usize](&txn)?;
            version += 1;
        }
        txn.open_table(KV)?
            .insert(KEY_SCHEMA, to_json(&version)?.as_slice())?;
        txn.commit()?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self.kv::<u32>(KEY_SCHEMA)?.unwrap_or(0))
    }

    // ---- kv ----

    pub fn kv<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(KV)?;
        let v = t.get(key)?.map(|g| from_json(g.value()));
        v.transpose()
    }

    pub fn set_kv<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(KV)?
            .insert(key, to_json(value)?.as_slice())?;
        txn.commit()?;
        Ok(())
    }

    // ---- tracks ----

    /// Every track, in no particular order.
    pub fn tracks(&self) -> Result<Vec<Track>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(TRACKS)?;
        let mut out = Vec::with_capacity(t.len()? as usize);
        for row in t.iter()? {
            let (id, v) = row?;
            match from_json::<Track>(v.value()) {
                Ok(track) => out.push(track),
                // One bad row must not lose the library; it's rewritten on the next scan.
                Err(e) => log::warn!("database: skipping track {}: {e}", id.value()),
            }
        }
        Ok(out)
    }

    /// The library, ready to use (sorted and indexed).
    pub fn library(&self) -> Result<Library> {
        Ok(Library::from_tracks(self.tracks()?))
    }

    /// Writes these tracks (insert or replace), in one transaction.
    pub fn put_tracks<'a>(&self, tracks: impl IntoIterator<Item = &'a Track>) -> Result<usize> {
        let txn = self.db.begin_write()?;
        let mut n = 0;
        {
            let mut t = txn.open_table(TRACKS)?;
            for track in tracks {
                t.insert(track.id.as_str(), to_json(track)?.as_slice())?;
                n += 1;
            }
        }
        txn.commit()?;
        Ok(n)
    }

    /// Makes the `tracks` table equal to `tracks`: writes rows that changed and removes rows
    /// that are gone, in one transaction.
    pub fn save_tracks(&self, tracks: &[Track]) -> Result<SyncReport> {
        let mut wanted: HashMap<&str, Vec<u8>> = HashMap::with_capacity(tracks.len());
        for t in tracks {
            wanted.insert(t.id.as_str(), to_json(t)?);
        }
        let txn = self.db.begin_write()?;
        let mut report = SyncReport::default();
        {
            let mut t = txn.open_table(TRACKS)?;
            let mut stale = Vec::new();
            for row in t.iter()? {
                let (id, v) = row?;
                match wanted.get(id.value()) {
                    Some(bytes) if bytes.as_slice() == v.value() => {
                        wanted.remove(id.value());
                    }
                    Some(_) => {}
                    None => stale.push(id.value().to_string()),
                }
            }
            for id in &stale {
                t.remove(id.as_str())?;
            }
            report.removed = stale.len();
            for (id, bytes) in &wanted {
                t.insert(*id, bytes.as_slice())?;
            }
            report.written = wanted.len();
        }
        txn.commit()?;
        Ok(report)
    }

    // ---- settings ----

    /// All settings, or None if none were ever saved. Rows that are missing or unreadable get
    /// their default value.
    pub fn settings(&self) -> Result<Option<Settings>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SETTINGS)?;
        let mut map = serde_json::Map::new();
        for row in t.iter()? {
            let (k, v) = row?;
            match serde_json::from_slice::<serde_json::Value>(v.value()) {
                Ok(v) => {
                    map.insert(k.value().to_string(), v);
                }
                Err(e) => log::warn!("database: setting {} unreadable: {e}", k.value()),
            }
        }
        if map.is_empty() {
            return Ok(None);
        }
        // A value of the wrong type (e.g. after a hand edit) must not lose every setting: each
        // stored value is checked on its own, and a bad one keeps its default.
        let serde_json::Value::Object(mut good) = serde_json::to_value(Settings::default())? else {
            unreachable!("Settings serialises to an object")
        };
        for (k, v) in map {
            let mut trial = good.clone();
            trial.insert(k.clone(), v.clone());
            if serde_json::from_value::<Settings>(serde_json::Value::Object(trial)).is_ok() {
                good.insert(k, v);
            } else {
                log::warn!("database: setting {k} has a bad value; using its default");
            }
        }
        Ok(Some(serde_json::from_value(serde_json::Value::Object(
            good,
        ))?))
    }

    /// One setting's stored value.
    pub fn setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SETTINGS)?;
        let v = t.get(key)?.map(|g| from_json(g.value()));
        v.transpose()
    }

    /// Saves all settings, writing only the rows that changed and removing rows for fields that
    /// no longer exist.
    pub fn put_settings(&self, s: &Settings) -> Result<()> {
        let serde_json::Value::Object(map) = serde_json::to_value(s)? else {
            unreachable!("Settings serialises to an object")
        };
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(SETTINGS)?;
            let mut stale = Vec::new();
            let mut same = Vec::new();
            for row in t.iter()? {
                let (k, v) = row?;
                match map.get(k.value()) {
                    None => stale.push(k.value().to_string()),
                    Some(new) if to_json(new)?.as_slice() == v.value() => {
                        same.push(k.value().to_string())
                    }
                    Some(_) => {}
                }
            }
            for k in &stale {
                t.remove(k.as_str())?;
            }
            for (k, v) in &map {
                if !same.contains(k) {
                    t.insert(k.as_str(), to_json(v)?.as_slice())?;
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    // ---- import from older versions ----

    /// Brings over `library.json` and `settings.json` from `dir` (written by Loss before the
    /// database), once. Imported files are renamed to `*.json.imported` and kept as a backup.
    /// A file that can't be read is left where it is and reported in the log. `prepare` runs on
    /// the imported settings before they are written (the app moves passwords to the vault there,
    /// so they never reach the database).
    pub fn import_legacy(
        &self,
        dir: &Path,
        prepare: impl FnOnce(&mut Settings),
    ) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        if self.kv::<bool>(KEY_IMPORTED)?.unwrap_or(false) {
            return Ok(report);
        }
        let lib_path = dir.join("library.json");
        if lib_path.exists() {
            match std::fs::read(&lib_path)
                .map_err(StoreError::from)
                .and_then(|b| from_json::<LegacyLibrary>(&b))
            {
                Ok(lib) => {
                    report.tracks = self.put_tracks(&lib.tracks)?;
                    backup(&lib_path);
                }
                Err(e) => log::error!("importing {}: {e}", lib_path.display()),
            }
        }
        let settings_path = dir.join("settings.json");
        if settings_path.exists() {
            match Settings::load(&settings_path) {
                Some(mut s) => {
                    prepare(&mut s);
                    self.put_settings(&s)?;
                    report.settings = true;
                    backup(&settings_path);
                }
                None => log::error!("importing {}: unreadable", settings_path.display()),
            }
        }
        self.set_kv(KEY_IMPORTED, &true)?;
        if report != ImportReport::default() {
            log::info!("database: imported {report:?}");
        }
        Ok(report)
    }
}

#[derive(serde::Deserialize)]
struct LegacyLibrary {
    #[serde(default)]
    tracks: Vec<Track>,
}

fn backup(path: &Path) {
    let to = path.with_extension("json.imported");
    if let Err(e) = std::fs::rename(path, &to) {
        log::warn!("keeping {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests;
