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
    pub fn update_sync_interval(&self, interval_seconds: u64) -> Result<()> {
        let mut settings = self.load()?;
        settings.sync_interval_seconds = interval_seconds;
        settings.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        self.save(&settings)
    }

    /// Toggle background sync
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
