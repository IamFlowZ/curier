use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{Context, Result};
use log::{debug, warn};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;

const SETTINGS_FILE: &str = "settings.json.enc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub sync_interval_seconds: u64,
    pub enable_background_sync: bool,
    pub max_headers_per_folder: usize,
    pub max_bodies_total: usize,
    pub emails_per_page: u32,
    #[serde(default)]
    pub approved_image_domains: Vec<String>,
    #[serde(default)]
    pub account_tab_order: Vec<String>,
    /// Default view mode per inbox: key is account email or "all", value is "feeds" or "list"
    #[serde(default)]
    pub default_view_mode: HashMap<String, String>,
    pub updated_at: i64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            sync_interval_seconds: 180, // 3 minutes
            enable_background_sync: true,
            max_headers_per_folder: 1000,
            max_bodies_total: 100,
            emails_per_page: 20,
            approved_image_domains: Vec::new(),
            account_tab_order: Vec::new(),
            default_view_mode: HashMap::new(),
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        }
    }
}

pub struct SettingsService {
    settings_path: PathBuf,
    encryption_key: [u8; 32],
}

impl SettingsService {
    pub fn new(app_data_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&app_data_dir)
            .context("Failed to create app data directory")?;

        let settings_path = app_data_dir.join(SETTINGS_FILE);
        let encryption_key = Self::derive_key()?;

        Ok(Self {
            settings_path,
            encryption_key,
        })
    }

    fn derive_key() -> Result<[u8; 32]> {
        let machine_id = machine_uid::get()
            .unwrap_or_else(|_| "fallback-machine-id".to_string());
        let username = whoami::username();

        let input_key_material = format!("{}:{}", machine_id, username);
        let salt = b"curier-settings-store-v1";

        let hk = Hkdf::<Sha256>::new(Some(salt), input_key_material.as_bytes());
        let mut okm = [0u8; 32];
        hk.expand(b"aes-256-gcm-key", &mut okm)
            .map_err(|_| anyhow::anyhow!("HKDF expansion failed"))?;

        Ok(okm)
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 {
            anyhow::bail!("Invalid encrypted data: too short");
        }

        let cipher = Aes256Gcm::new_from_slice(&self.encryption_key)
            .map_err(|_| anyhow::anyhow!("Invalid key length"))?;

        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed - settings may be corrupted or from another machine"))?;

        Ok(plaintext)
    }

    /// Load settings from disk, returns default if not found
    pub fn load(&self) -> Result<AppSettings> {
        debug!("SETTINGS: Loading app settings");

        if !self.settings_path.exists() {
            debug!("SETTINGS: No settings file found, using defaults");
            return Ok(AppSettings::default());
        }

        let mut file = File::open(&self.settings_path)
            .context("Failed to open settings file")?;

        let mut encrypted_data = Vec::new();
        file.read_to_end(&mut encrypted_data)
            .context("Failed to read settings file")?;

        if encrypted_data.is_empty() {
            debug!("SETTINGS: Empty settings file, using defaults");
            return Ok(AppSettings::default());
        }

        let decrypted = self.decrypt(&encrypted_data)
            .unwrap_or_else(|e| {
                warn!("SETTINGS: Failed to decrypt settings: {}, using defaults", e);
                serde_json::to_vec(&AppSettings::default()).unwrap()
            });

        let settings: AppSettings = serde_json::from_slice(&decrypted)
            .unwrap_or_else(|e| {
                warn!("SETTINGS: Failed to parse settings: {}, using defaults", e);
                AppSettings::default()
            });

        debug!("SETTINGS: Loaded successfully");
        Ok(settings)
    }

    /// Save settings to disk
    pub fn save(&self, settings: &AppSettings) -> Result<()> {
        debug!("SETTINGS: Saving app settings");

        let json = serde_json::to_string_pretty(settings)
            .context("Failed to serialize settings")?;

        let encrypted = self.encrypt(json.as_bytes())?;

        let temp_path = self.settings_path.with_extension("enc.tmp");
        {
            let mut file = File::create(&temp_path)
                .context("Failed to create temp settings file")?;
            file.write_all(&encrypted)
                .context("Failed to write settings")?;
            file.sync_all()
                .context("Failed to sync settings file")?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&temp_path, permissions)
                .context("Failed to set file permissions")?;
        }

        fs::rename(&temp_path, &self.settings_path)
            .context("Failed to rename settings file")?;

        debug!("SETTINGS: Saved successfully");
        Ok(())
    }

    /// Update sync interval
    /// (Only exercised by tests; the UI goes through `update`.)
    #[cfg(test)]
    pub fn update_sync_interval(&self, interval_seconds: u64) -> Result<()> {
        let mut settings = self.load()?;
        settings.sync_interval_seconds = interval_seconds;
        settings.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        self.save(&settings)
    }

    /// Toggle background sync
    /// (Only exercised by tests; the UI goes through `update`.)
    #[cfg(test)]
    pub fn toggle_background_sync(&self, enable: bool) -> Result<()> {
        let mut settings = self.load()?;
        settings.enable_background_sync = enable;
        settings.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        self.save(&settings)
    }

    /// Update all settings
    pub fn update(&self, settings: AppSettings) -> Result<()> {
        let mut updated_settings = settings;
        updated_settings.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        self.save(&updated_settings)
    }

    /// Update emails per page setting (only allow 20, 50, 100)
    /// (Only exercised by tests; the UI goes through `update`.)
    #[cfg(test)]
    pub fn update_emails_per_page(&self, count: u32) -> Result<()> {
        if count != 20 && count != 50 && count != 100 {
            anyhow::bail!("emails_per_page must be 20, 50, or 100");
        }

        let mut settings = self.load()?;
        settings.emails_per_page = count;
        settings.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        self.save(&settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> (tempfile::TempDir, SettingsService) {
        let dir = tempfile::tempdir().unwrap();
        let service = SettingsService::new(dir.path().join("nested").join("app")).unwrap();
        (dir, service)
    }

    #[test]
    fn settings_new_creates_app_data_dir() {
        let (dir, _service) = service();
        assert!(dir.path().join("nested").join("app").is_dir());
    }

    #[test]
    fn settings_defaults_are_sane() {
        let d = AppSettings::default();
        assert_eq!(d.sync_interval_seconds, 180);
        assert!(d.enable_background_sync);
        assert_eq!(d.max_headers_per_folder, 1000);
        assert_eq!(d.max_bodies_total, 100);
        assert_eq!(d.emails_per_page, 20);
        assert!(d.approved_image_domains.is_empty());
        assert!(d.default_view_mode.is_empty());
    }

    #[test]
    fn settings_load_returns_defaults_when_file_missing() {
        let (_dir, service) = service();
        let loaded = service.load().unwrap();
        assert_eq!(loaded.sync_interval_seconds, AppSettings::default().sync_interval_seconds);
    }

    #[test]
    fn settings_save_then_load_round_trips() {
        let (_dir, service) = service();
        let mut settings = AppSettings::default();
        settings.sync_interval_seconds = 999;
        settings.approved_image_domains = vec!["cdn.example.com".to_string()];
        settings.account_tab_order = vec!["b@x.com".to_string(), "a@x.com".to_string()];
        settings.default_view_mode.insert("all".to_string(), "feeds".to_string());
        service.save(&settings).unwrap();

        let loaded = service.load().unwrap();
        assert_eq!(loaded.sync_interval_seconds, 999);
        assert_eq!(loaded.approved_image_domains, vec!["cdn.example.com"]);
        assert_eq!(loaded.account_tab_order, vec!["b@x.com", "a@x.com"]);
        assert_eq!(loaded.default_view_mode.get("all").map(String::as_str), Some("feeds"));
    }

    #[test]
    fn settings_file_on_disk_is_not_plaintext_json() {
        let (_dir, service) = service();
        let mut settings = AppSettings::default();
        settings.approved_image_domains = vec!["needle.example.com".to_string()];
        service.save(&settings).unwrap();
        let raw = std::fs::read(&service.settings_path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("needle.example.com"));
        assert!(!service.settings_path.with_extension("enc.tmp").exists(), "temp file must be renamed away");
    }

    #[cfg(unix)]
    #[test]
    fn settings_file_is_owner_read_write_only() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, service) = service();
        service.save(&AppSettings::default()).unwrap();
        let mode = std::fs::metadata(&service.settings_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn settings_empty_file_falls_back_to_defaults() {
        let (_dir, service) = service();
        std::fs::write(&service.settings_path, b"").unwrap();
        assert_eq!(service.load().unwrap().emails_per_page, 20);
    }

    #[test]
    fn settings_corrupt_file_falls_back_to_defaults() {
        let (_dir, service) = service();
        std::fs::write(&service.settings_path, b"definitely not ciphertext at all").unwrap();
        assert_eq!(service.load().unwrap().emails_per_page, 20);
    }

    #[test]
    fn settings_tampered_ciphertext_falls_back_to_defaults() {
        let (_dir, service) = service();
        let mut settings = AppSettings::default();
        settings.emails_per_page = 100;
        service.save(&settings).unwrap();

        let mut raw = std::fs::read(&service.settings_path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&service.settings_path, &raw).unwrap();

        assert_eq!(service.load().unwrap().emails_per_page, 20, "AES-GCM auth failure must not yield tampered data");
    }

    #[test]
    fn settings_decrypt_rejects_data_shorter_than_nonce() {
        let (_dir, service) = service();
        assert!(service.decrypt(&[0u8; 11]).is_err());
    }

    #[test]
    fn settings_encrypt_uses_fresh_nonce_each_time() {
        let (_dir, service) = service();
        let a = service.encrypt(b"same").unwrap();
        let b = service.encrypt(b"same").unwrap();
        assert_ne!(a[..12], b[..12]);
        assert_eq!(service.decrypt(&a).unwrap(), b"same");
        assert_eq!(service.decrypt(&b).unwrap(), b"same");
    }

    #[test]
    fn settings_update_emails_per_page_accepts_only_allowed_values() {
        let (_dir, service) = service();
        for ok in [20, 50, 100] {
            service.update_emails_per_page(ok).unwrap();
            assert_eq!(service.load().unwrap().emails_per_page, ok);
        }
        assert!(service.update_emails_per_page(25).is_err());
        assert!(service.update_emails_per_page(0).is_err());
        assert_eq!(service.load().unwrap().emails_per_page, 100, "rejected value must not be persisted");
    }

    #[test]
    fn settings_update_sync_interval_and_toggle_background_sync_persist() {
        let (_dir, service) = service();
        service.update_sync_interval(42).unwrap();
        service.toggle_background_sync(false).unwrap();
        let loaded = service.load().unwrap();
        assert_eq!(loaded.sync_interval_seconds, 42);
        assert!(!loaded.enable_background_sync);
    }

    #[test]
    fn settings_update_bumps_updated_at() {
        let (_dir, service) = service();
        let mut settings = AppSettings::default();
        settings.updated_at = 1;
        service.update(settings).unwrap();
        assert!(service.load().unwrap().updated_at > 1);
    }

    #[test]
    fn settings_deserialize_tolerates_missing_optional_fields() {
        let json = r#"{"sync_interval_seconds":1,"enable_background_sync":true,"max_headers_per_folder":2,"max_bodies_total":3,"emails_per_page":20,"updated_at":0}"#;
        let parsed: AppSettings = serde_json::from_str(json).unwrap();
        assert!(parsed.approved_image_domains.is_empty());
        assert!(parsed.account_tab_order.is_empty());
        assert!(parsed.default_view_mode.is_empty());
    }
}
