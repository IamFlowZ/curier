use anyhow::{Context, Result};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::models::auth::StoredCredentials;

const KEYRING_SERVICE: &str = "com.curier.credentials";
const KEYRING_USER: &str = "accounts";

/// Process-wide credential cache.
/// - `credentials`: the in-memory copy (None = not yet loaded from keychain)
/// - `dirty`: true when the cache has unsaved changes that need flushing to keychain
struct CacheState {
    credentials: Option<HashMap<String, StoredCredentials>>,
    dirty: bool,
}

static CACHE: OnceLock<Mutex<CacheState>> = OnceLock::new();

fn cache() -> &'static Mutex<CacheState> {
    CACHE.get_or_init(|| Mutex::new(CacheState {
        credentials: None,
        dirty: false,
    }))
}

pub struct CredentialStore {
    app_data_dir: PathBuf,
}

impl CredentialStore {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        let store = Self { app_data_dir };
        store.migrate_from_encrypted_file()?;
        Ok(store)
    }

    /// Get a keyring entry handle for the accounts store.
    fn entry() -> Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
            .map_err(|e| anyhow::anyhow!("Failed to access keychain: {}", e))
    }

    /// Flush any pending changes to the keychain.
    /// Call this after a batch of mutations to persist in one keychain access.
    pub fn flush(&self) -> Result<()> {
        let mut guard = cache().lock().unwrap();
        if !guard.dirty {
            return Ok(());
        }

        if let Some(ref credentials) = guard.credentials {
            Self::write_to_keychain(credentials)?;
        }

        guard.dirty = false;
        debug!("CREDENTIAL_STORE: Flushed credentials to keychain");
        Ok(())
    }

    /// Write credentials to the keychain (the only place we actually touch the keychain for writes).
    fn write_to_keychain(credentials: &HashMap<String, StoredCredentials>) -> Result<()> {
        let entry = Self::entry()?;

        if credentials.is_empty() {
            match entry.delete_credential() {
                Ok(()) => {}
                Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(anyhow::anyhow!("Failed to delete keychain entry: {}", e)),
            }
        } else {
            let json = serde_json::to_string(credentials)
                .context("Failed to serialize credentials")?;
            entry.set_password(&json)
                .map_err(|e| anyhow::anyhow!("Failed to store credentials in keychain: {}", e))?;
        }

        Ok(())
    }

    /// Migrate credentials from the old encrypted file to the OS keychain.
    fn migrate_from_encrypted_file(&self) -> Result<()> {
        let old_path = self.app_data_dir.join("credentials").join("accounts.json.enc");
        if !old_path.exists() {
            return Ok(());
        }

        let entry = Self::entry()?;
        if entry.get_password().is_ok() {
            debug!("CREDENTIAL_STORE: Keychain already populated, removing old encrypted file");
            let _ = std::fs::remove_file(&old_path);
            return Ok(());
        }

        info!("CREDENTIAL_STORE: Migrating credentials from encrypted file to OS keychain");
        match Self::read_legacy_encrypted_file(&old_path) {
            Ok(accounts) if !accounts.is_empty() => {
                let json = serde_json::to_string(&accounts)
                    .context("Failed to serialize credentials for migration")?;
                entry.set_password(&json)
                    .map_err(|e| anyhow::anyhow!("Failed to store migrated credentials in keychain: {}", e))?;
                info!("CREDENTIAL_STORE: Successfully migrated {} accounts to keychain", accounts.len());

                let mut guard = cache().lock().unwrap();
                guard.credentials = Some(accounts);
                guard.dirty = false;

                let _ = std::fs::remove_file(&old_path);
            }
            Ok(_) => {
                debug!("CREDENTIAL_STORE: Old file was empty, nothing to migrate");
                let _ = std::fs::remove_file(&old_path);
            }
            Err(e) => {
                warn!("CREDENTIAL_STORE: Could not decrypt old file for migration: {}. Users will need to re-authenticate.", e);
            }
        }

        Ok(())
    }

    /// Attempt to read and decrypt the legacy AES-256-GCM encrypted file.
    fn read_legacy_encrypted_file(path: &std::path::Path) -> Result<HashMap<String, StoredCredentials>> {
        use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
        use hkdf::Hkdf;
        use sha2::Sha256;

        let machine_id = machine_uid::get()
            .unwrap_or_else(|_| "fallback-machine-id".to_string());
        let username = whoami::username();
        let input_key_material = format!("{}:{}", machine_id, username);
        let salt = b"curier-credential-store-v1";

        let hk = Hkdf::<Sha256>::new(Some(salt), input_key_material.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"aes-256-gcm-key", &mut key)
            .map_err(|_| anyhow::anyhow!("HKDF expansion failed"))?;

        let data = std::fs::read(path).context("Failed to read legacy credentials file")?;
        if data.len() < 12 {
            anyhow::bail!("Legacy file too short to contain valid encrypted data");
        }

        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;
        let nonce = Nonce::from_slice(&data[..12]);
        let plaintext = cipher
            .decrypt(nonce, &data[12..])
            .map_err(|_| anyhow::anyhow!("Decryption failed — credentials may be from another machine"))?;

        let accounts: HashMap<String, StoredCredentials> = serde_json::from_slice(&plaintext)
            .context("Failed to parse legacy credentials JSON")?;

        Ok(accounts)
    }

    /// Read from cache if populated, otherwise read from keychain once and cache the result.
    fn load_all_credentials(&self) -> Result<HashMap<String, StoredCredentials>> {
        let mut guard = cache().lock().unwrap();

        if let Some(ref cached) = guard.credentials {
            debug!("CREDENTIAL_STORE: Returning credentials from in-memory cache");
            return Ok(cached.clone());
        }

        // First access — read from keychain and populate cache
        debug!("CREDENTIAL_STORE: Cache miss, reading from keychain");
        let entry = Self::entry()?;

        let credentials = match entry.get_password() {
            Ok(json) => {
                serde_json::from_str(&json)
                    .context("Failed to parse credentials from keychain")?
            }
            Err(keyring::Error::NoEntry) => HashMap::new(),
            Err(e) => return Err(anyhow::anyhow!("Failed to read from keychain: {}", e)),
        };

        guard.credentials = Some(credentials.clone());
        guard.dirty = false;
        Ok(credentials)
    }

    /// Update the in-memory cache only. Call `flush()` to persist to keychain.
    fn save_all_credentials(&self, credentials: &HashMap<String, StoredCredentials>) -> Result<()> {
        let mut guard = cache().lock().unwrap();
        guard.credentials = Some(credentials.clone());
        guard.dirty = true;
        Ok(())
    }

    pub fn store_account(&self, email: &str, credentials: &StoredCredentials) -> Result<()> {
        debug!("CREDENTIAL_STORE: Storing credentials for account: {}", email);

        let mut all_credentials = self.load_all_credentials()?;
        all_credentials.insert(email.to_string(), credentials.clone());
        self.save_all_credentials(&all_credentials)?;

        debug!("CREDENTIAL_STORE: Successfully stored credentials for: {}", email);
        Ok(())
    }

    pub fn load_account(&self, email: &str) -> Result<Option<StoredCredentials>> {
        debug!("CREDENTIAL_STORE: Loading credentials for account: {}", email);

        let all_credentials = self.load_all_credentials()?;
        let result = all_credentials.get(email).cloned();

        if result.is_some() {
            debug!("CREDENTIAL_STORE: Found credentials for: {}", email);
        } else {
            debug!("CREDENTIAL_STORE: No credentials found for: {}", email);
        }

        Ok(result)
    }

    pub fn remove_account(&self, email: &str) -> Result<()> {
        debug!("CREDENTIAL_STORE: Removing credentials for account: {}", email);

        let mut all_credentials = self.load_all_credentials()?;
        all_credentials.remove(email);
        self.save_all_credentials(&all_credentials)?;

        debug!("CREDENTIAL_STORE: Successfully removed credentials for: {}", email);
        Ok(())
    }

    pub fn load_all(&self) -> Result<Vec<(String, StoredCredentials)>> {
        debug!("CREDENTIAL_STORE: Loading all accounts");

        let all_credentials = self.load_all_credentials()?;
        let result: Vec<(String, StoredCredentials)> = all_credentials
            .into_iter()
            .collect();

        debug!("CREDENTIAL_STORE: Loaded {} accounts", result.len());
        Ok(result)
    }
}
