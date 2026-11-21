//! Encrypted storage for secrets (server passwords, API keys, tokens).
//!
//! Secrets live in one AES-256-GCM encrypted file. The 32-byte key comes from
//! the platform: on Android it is itself encrypted by a key held in the
//! Android Keystore, so the vault file alone is useless to anyone who copies it.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Secret names used by the app.
pub mod names {
    pub const SUBSONIC_PASSWORD: &str = "subsonic.password";
}

#[derive(Debug)]
pub struct Vault {
    path: PathBuf,
    key: [u8; KEY_LEN],
    secrets: BTreeMap<String, String>,
}

/// A fresh random key.
pub fn new_key() -> Result<[u8; KEY_LEN], String> {
    let mut key = [0u8; KEY_LEN];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    Ok(key)
}

fn encrypt(key: &[u8; KEY_LEN], plain: &[u8]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(Nonce::from_slice(&nonce), plain)
            .map_err(|_| "encryption failed".to_string())?,
    );
    Ok(out)
}

fn decrypt(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_LEN {
        return Err("vault file is too short".into());
    }
    let (nonce, body) = data.split_at(NONCE_LEN);
    Aes256Gcm::new(key.into())
        .decrypt(Nonce::from_slice(nonce), body)
        .map_err(|_| "vault can't be decrypted with this key".into())
}

impl Vault {
    /// Opens (or starts) the vault at `path`. A file that can't be decrypted
    /// (e.g. the key was lost after a reinstall) is set aside, not deleted.
    pub fn open(path: &Path, key: [u8; KEY_LEN]) -> Vault {
        let mut secrets = BTreeMap::new();
        if let Ok(data) = std::fs::read(path) {
            match decrypt(&key, &data).and_then(|plain| {
                serde_json::from_slice::<BTreeMap<String, String>>(&plain)
                    .map_err(|e| e.to_string())
            }) {
                Ok(s) => secrets = s,
                Err(e) => {
                    log::warn!("vault unreadable ({e}); starting a new one");
                    let _ = std::fs::rename(path, path.with_extension("unreadable"));
                }
            }
        }
        Vault {
            path: path.to_path_buf(),
            key,
            secrets,
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.secrets.get(name).map(String::as_str)
    }

    /// Stores a secret; an empty value removes it.
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        if value.is_empty() {
            self.secrets.remove(name);
        } else {
            self.secrets.insert(name.to_string(), value.to_string());
        }
        self.save()
    }

    fn save(&self) -> Result<(), String> {
        let plain = serde_json::to_vec(&self.secrets).map_err(|e| e.to_string())?;
        let data = encrypt(&self.key, &plain)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_wrong_key() {
        let dir = std::env::temp_dir().join(format!("loss-vault-{}", std::process::id()));
        let path = dir.join("vault.bin");
        let key = new_key().unwrap();
        let mut v = Vault::open(&path, key);
        v.set(names::SUBSONIC_PASSWORD, "hunter2").unwrap();

        // Encrypted on disk.
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.windows(7).any(|w| w == b"hunter2"));

        let v2 = Vault::open(&path, key);
        assert_eq!(v2.get(names::SUBSONIC_PASSWORD), Some("hunter2"));

        // Another key can't read it; the file is set aside, not lost.
        let v3 = Vault::open(&path, new_key().unwrap());
        assert_eq!(v3.get(names::SUBSONIC_PASSWORD), None);
        assert!(path.with_extension("unreadable").exists());

        let mut v4 = Vault::open(&path, key);
        v4.set("x", "1").unwrap();
        v4.set("x", "").unwrap();
        assert_eq!(v4.get("x"), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
