use crate::models::auth::{AccountType, OAuthConfig};
use crate::services::database::DatabaseService;
use crate::services::email_provider::EmailProvider;
use crate::services::gmail_api::GmailApiService;
use crate::services::oauth::OAuthService;
use crate::services::outlook_api::OutlookApiService;
use crate::services::settings::SettingsService;
use crate::services::sync::SyncService;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use log::{debug, info, warn};
use tokio::sync::Mutex;

/// Multi-account state storage
struct MultiAccountCache {
    /// Map of account_id (email) -> OAuthConfig
    accounts: HashMap<String, OAuthConfig>,
    /// Whether we've loaded from Keychain
    loaded: bool,
}

pub struct AppState {
    /// Multi-account credentials cache
    multi_account_cache: Arc<std::sync::Mutex<MultiAccountCache>>,
    /// Shared database service
    pub database: Arc<Mutex<DatabaseService>>,
    /// Settings service
    pub settings_service: Arc<Mutex<SettingsService>>,
    /// Sync service
    pub sync_service: Arc<Mutex<Option<SyncService>>>,
    /// Email provider services by account email
    pub accounts: Arc<Mutex<HashMap<String, Arc<dyn EmailProvider>>>>,
    /// Accounts that need re-authentication (e.g., revoked refresh tokens)
    reauth_required_accounts: Arc<std::sync::Mutex<HashSet<String>>>,
}

impl AppState {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, String> {
        // Initialize database service
        let db_path = app_data_dir.join("emails.db");
        let database = DatabaseService::new(db_path)
            .map_err(|e| format!("Failed to initialize database: {}", e))?;

        // Initialize settings service
        let settings_service = SettingsService::new(app_data_dir)
            .map_err(|e| format!("Failed to initialize settings service: {}", e))?;

        Ok(Self {
            multi_account_cache: Arc::new(std::sync::Mutex::new(MultiAccountCache {
                accounts: HashMap::new(),
                loaded: false,
            })),
            database: Arc::new(Mutex::new(database)),
            settings_service: Arc::new(Mutex::new(settings_service)),
            sync_service: Arc::new(Mutex::new(None)),
            accounts: Arc::new(Mutex::new(HashMap::new())),
            reauth_required_accounts: Arc::new(std::sync::Mutex::new(HashSet::new())),
        })
    }

    /// Initialize the sync service
    pub async fn init_sync_service(&self, app_handle: tauri::AppHandle) -> Result<(), String> {
        info!("AppState: Initializing sync service");

        let mut sync_service_guard = self.sync_service.lock().await;

        if sync_service_guard.is_some() {
            warn!("AppState: Sync service already initialized");
            return Ok(());
        }

        let mut sync_service = SyncService::new();
        sync_service
            .start(app_handle)
            .await
            .map_err(|e| format!("Failed to start sync service: {}", e))?;

        *sync_service_guard = Some(sync_service);

        info!("AppState: Sync service initialized");
        Ok(())
    }

    /// Initialize all accounts for syncing on app startup
    pub async fn init_accounts_for_sync(&self, app_data_dir: PathBuf) -> Result<(), String> {
        info!("AppState: Initializing accounts for sync...");

        // Load all OAuth configs from credential store
        let account_configs = self.get_all_account_configs(&app_data_dir)?;

        info!("AppState: Found {} accounts to initialize", account_configs.len());

        // Register each account with the appropriate email provider
        for (account_email, config) in account_configs {
            debug!("AppState: Registering account: {} (type: {:?})", account_email, config.account_type);

            let provider: Arc<dyn EmailProvider> = match config.account_type {
                AccountType::Outlook => {
                    let oauth_service = OAuthService::new(config);
                    Arc::new(OutlookApiService::new(oauth_service, app_data_dir.clone()))
                }
                _ => {
                    let oauth_service = OAuthService::new(config);
                    Arc::new(GmailApiService::new(oauth_service, app_data_dir.clone()))
                }
            };

            self.register_email_provider(account_email.clone(), provider).await;

            debug!("AppState: Registered account: {}", account_email);
        }

        info!("AppState: All accounts initialized for sync");
        Ok(())
    }

    /// Register an email provider (Gmail, Outlook, etc.) for an account
    pub async fn register_email_provider(&self, account_email: String, provider: Arc<dyn EmailProvider>) {
        let mut accounts = self.accounts.lock().await;
        accounts.insert(account_email, provider);
    }

    // ============ Multi-Account Methods ============

    /// Get OAuth config for a specific account
    pub fn get_account_config(&self, account_id: &str, app_data_dir: &PathBuf) -> Result<Option<OAuthConfig>, String> {
        debug!("AppState: get_account_config called for account: {}", account_id);

        let mut cache = self.multi_account_cache.lock().unwrap();

        // Load all accounts from credential store if not loaded yet
        if !cache.loaded {
            debug!("AppState: Loading all accounts from credential store...");
            match OAuthService::load_all_accounts(app_data_dir) {
                Ok(accounts) => {
                    for (id, config) in accounts {
                        debug!("AppState: Loaded account: {}", id);
                        cache.accounts.insert(id, config);
                    }
                    cache.loaded = true;
                }
                Err(e) => {
                    warn!("AppState: Error loading accounts: {}", e);
                    // Don't fail - just mark as loaded with empty accounts
                    cache.loaded = true;
                }
            }
        }

        Ok(cache.accounts.get(account_id).cloned())
    }

    /// Replace the account cache with the given configs and mark it loaded, so
    /// no later call consults the credential store. Used by demo mode to run
    /// with fake accounts that have no Keychain entry.
    pub fn preload_accounts(&self, configs: Vec<(String, OAuthConfig)>) {
        let mut cache = self.multi_account_cache.lock().unwrap();
        cache.accounts.clear();
        for (account_id, config) in configs {
            info!("AppState: Preloading account (no credential store): {}", account_id);
            cache.accounts.insert(account_id, config);
        }
        cache.loaded = true;
    }

    /// Set OAuth config for a specific account
    pub fn set_account_config(&self, account_id: &str, config: OAuthConfig) {
        debug!("AppState: set_account_config for account: {}", account_id);
        let mut cache = self.multi_account_cache.lock().unwrap();
        cache.accounts.insert(account_id.to_string(), config);
    }

    /// Get all account configs
    pub fn get_all_account_configs(&self, app_data_dir: &PathBuf) -> Result<HashMap<String, OAuthConfig>, String> {
        let mut cache = self.multi_account_cache.lock().unwrap();

        // Load all accounts from credential store if not loaded yet
        if !cache.loaded {
            debug!("AppState: Loading all accounts from credential store...");
            match OAuthService::load_all_accounts(app_data_dir) {
                Ok(accounts) => {
                    for (id, config) in accounts {
                        cache.accounts.insert(id, config);
                    }
                    cache.loaded = true;
                }
                Err(e) => {
                    warn!("AppState: Error loading accounts: {}", e);
                    cache.loaded = true;
                }
            }
        }

        Ok(cache.accounts.clone())
    }

    /// Remove an account
    pub fn remove_account(&self, account_id: &str) {
        debug!("AppState: remove_account: {}", account_id);
        let mut cache = self.multi_account_cache.lock().unwrap();
        cache.accounts.remove(account_id);
    }

    /// Clear all account caches
    pub fn clear_all_accounts(&self) {
        debug!("AppState: clear_all_accounts");
        let mut cache = self.multi_account_cache.lock().unwrap();
        cache.accounts.clear();
        cache.loaded = false;
    }

    // ============ Re-auth Tracking Methods ============

    /// Mark an account as needing re-authentication
    pub fn mark_reauth_required(&self, email: &str) {
        warn!("AppState: Marking account as needing re-auth: {}", email);
        let mut set = self.reauth_required_accounts.lock().unwrap();
        set.insert(email.to_string());
    }

    /// Check if an account needs re-authentication
    pub fn is_reauth_required(&self, email: &str) -> bool {
        let set = self.reauth_required_accounts.lock().unwrap();
        set.contains(email)
    }

    /// Clear the re-authentication flag for an account (e.g., after successful re-auth)
    pub fn clear_reauth_required(&self, email: &str) {
        debug!("AppState: Clearing re-auth flag for: {}", email);
        let mut set = self.reauth_required_accounts.lock().unwrap();
        set.remove(email);
    }

    /// Get all accounts that need re-authentication
    pub fn get_reauth_required_accounts(&self) -> Vec<String> {
        let set = self.reauth_required_accounts.lock().unwrap();
        set.iter().cloned().collect()
    }

    /// Update just the token fields for a specific account in the cache.
    /// Called after API operations that may have triggered an OAuth token refresh,
    /// so subsequent operations see the fresh tokens without hitting disk.
    pub fn update_account_tokens(
        &self,
        account_id: &str,
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
    ) {
        let mut cache = self.multi_account_cache.lock().unwrap();
        if let Some(config) = cache.accounts.get_mut(account_id) {
            config.access_token = access_token;
            config.refresh_token = refresh_token;
            config.expires_at = expires_at;
            debug!("AppState: Updated token cache for: {}", account_id);
        }
    }
}
