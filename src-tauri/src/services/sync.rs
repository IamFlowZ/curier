use anyhow::{Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, RwLock};
use tokio::time::{sleep, Duration};

use crate::services::oauth::OAuthService;
use crate::services::rule_engine::evaluate_email_against_rule;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub is_running: bool,
    pub last_sync_time: Option<i64>,
    pub last_sync_error: Option<String>,
    pub syncing_account: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            is_running: false,
            last_sync_time: None,
            last_sync_error: None,
            syncing_account: None,
        }
    }
}

pub struct SyncService {
    status: Arc<RwLock<SyncStatus>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl SyncService {
    pub fn new() -> Self {
        Self {
            status: Arc::new(RwLock::new(SyncStatus::default())),
            shutdown_tx: None,
        }
    }

    /// Start the background sync service
    pub async fn start(&mut self, app_handle: AppHandle) -> Result<()> {
        info!("SYNC: Starting background sync service");

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        self.shutdown_tx = Some(shutdown_tx);

        let status = Arc::clone(&self.status);

        // Spawn background task
        tauri::async_runtime::spawn(async move {
            loop {
                // Check for shutdown signal (non-blocking)
                if shutdown_rx.try_recv().is_ok() {
                    info!("SYNC: Received shutdown signal");
                    break;
                }

                // Load settings to get sync interval
                let state: tauri::State<AppState> = app_handle.state();
                let settings_service = state.settings_service.lock().await;
                let settings = match settings_service.load() {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("SYNC: Failed to load settings: {}", e);
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }
                };
                drop(settings_service);

                // Check if background sync is enabled
                if !settings.enable_background_sync {
                    debug!("SYNC: Background sync is disabled");
                    sleep(Duration::from_secs(30)).await;
                    continue;
                }

                // Perform sync
                info!("SYNC: Starting sync cycle");
                let sync_result = Self::sync_all_accounts_internal(
                    app_handle.clone(),
                    Arc::clone(&status),
                )
                .await;

                match sync_result {
                    Ok(_) => {
                        info!("SYNC: Sync cycle completed successfully");
                        let mut s = status.write().await;
                        s.last_sync_time = Some(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64,
                        );
                        s.last_sync_error = None;
                    }
                    Err(e) => {
                        warn!("SYNC: Sync cycle failed: {}", e);
                        let mut s = status.write().await;
                        s.last_sync_error = Some(e.to_string());
                    }
                }

                // Sleep for configured interval
                let sleep_duration = Duration::from_secs(settings.sync_interval_seconds);
                debug!(
                    "SYNC: Sleeping for {} seconds until next sync",
                    settings.sync_interval_seconds
                );
                sleep(sleep_duration).await;
            }

            info!("SYNC: Background sync service stopped");
        });

        info!("SYNC: Background sync service started");
        Ok(())
    }

    /// Stop the background sync service
    pub fn stop(&mut self) -> Result<()> {
        info!("SYNC: Stopping background sync service");

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        Ok(())
    }

    /// Get current sync status
    pub async fn get_status(&self) -> SyncStatus {
        self.status.read().await.clone()
    }

    /// Manual sync trigger (static method for use in commands)
    pub async fn sync_now(app_handle: AppHandle) -> Result<()> {
        info!("SYNC: Manual sync triggered");

        let state: tauri::State<AppState> = app_handle.state();
        let sync_service = state.sync_service.lock().await;

        if let Some(ref service) = *sync_service {
            let status = Arc::clone(&service.status);
            drop(sync_service);

            Self::sync_all_accounts_internal(app_handle, status).await?;

            info!("SYNC: Manual sync completed");
            Ok(())
        } else {
            anyhow::bail!("Sync service not initialized")
        }
    }

    /// Internal method to sync all accounts
    async fn sync_all_accounts_internal(
        app_handle: AppHandle,
        status: Arc<RwLock<SyncStatus>>,
    ) -> Result<()> {
        let state: tauri::State<AppState> = app_handle.state();

        // Mark sync as running
        {
            let mut s = status.write().await;
            s.is_running = true;
            s.syncing_account = None;
        }

        // Get all accounts
        let accounts = {
            let accounts_map = state.accounts.lock().await;
            accounts_map.keys().cloned().collect::<Vec<_>>()
        };

        debug!("SYNC: Found {} accounts to sync", accounts.len());

        // Sync each account
        for account_email in accounts {
            // Skip accounts that need re-authentication
            if state.is_reauth_required(&account_email) {
                warn!("SYNC: Skipping {} (re-authentication required)", account_email);
                continue;
            }

            debug!("SYNC: Syncing account: {}", account_email);

            // Update status
            {
                let mut s = status.write().await;
                s.syncing_account = Some(account_email.clone());
            }

            // Sync INBOX folder
            if let Err(e) = Self::sync_folder(
                app_handle.clone(),
                &account_email,
                "INBOX",
            )
            .await
            {
                if OAuthService::is_reauth_required_error(&e) {
                    warn!(
                        "SYNC: Account {} needs re-authentication, marking and emitting event",
                        account_email
                    );
                    state.mark_reauth_required(&account_email);
                    let _ = app_handle.emit("reauth-required", account_email.clone());
                } else {
                    warn!(
                        "SYNC: Failed to sync {} for {}: {}",
                        "INBOX", account_email, e
                    );
                }
                // Continue with other accounts even if one fails
            }
        }

        // Mark sync as complete
        {
            let mut s = status.write().await;
            s.is_running = false;
            s.syncing_account = None;
        }

        Ok(())
    }

    /// Sync a specific folder for an account (delta sync).
    /// Lock scope is minimized: DB lock is held only for reads and writes,
    /// released during network calls to avoid blocking UI operations.
    async fn sync_folder(
        app_handle: AppHandle,
        account_email: &str,
        folder_id: &str,
    ) -> Result<()> {
        debug!(
            "SYNC: Syncing folder {} for {}",
            folder_id, account_email
        );

        let state: tauri::State<AppState> = app_handle.state();

        // Phase 1: Read sync metadata (short lock)
        let last_message_date = {
            let db = state.database.lock().await;
            db.get_last_sync_timestamp(account_email, folder_id)
                .context("Failed to get last sync timestamp")?
        };

        debug!(
            "SYNC: Last message date for {}: {:?}",
            folder_id, last_message_date
        );

        // Phase 2: Network calls (no DB lock held)
        let email_service = {
            let accounts = state.accounts.lock().await;
            accounts
                .get(account_email)
                .cloned()
                .context("Account not found")?
        };

        let emails = if let Some(after_date) = last_message_date {
            email_service
                .fetch_emails_after_date(folder_id, after_date, 100)
                .await
                .context("Failed to fetch emails after date")?
        } else {
            email_service
                .fetch_emails(folder_id, 100)
                .await
                .context("Failed to fetch emails")?
        };

        // Write refreshed tokens back to the AppState cache so other callers
        // (commands, other sync cycles) see fresh tokens without re-refreshing.
        {
            let token_handle = email_service.oauth_token_state_handle();
            let ts = token_handle.read().await;
            state.update_account_tokens(
                account_email,
                ts.access_token.clone(),
                ts.refresh_token.clone(),
                ts.expires_at,
            );
        }

        debug!("SYNC: Fetched {} new emails", emails.len());

        // Load settings (short lock) before the write phase
        let max_headers = {
            let settings_service = state.settings_service.lock().await;
            let settings = settings_service.load()?;
            settings.max_headers_per_folder
        };

        // Phase 3: Write results to DB (short lock)
        {
            let db = state.database.lock().await;

            let mut newest_date = last_message_date;
            let mut stored_count = 0;
            for email in &emails {
                if last_message_date.is_none() || email.date > last_message_date.unwrap() {
                    db.upsert_email_with_context(email, account_email, folder_id)
                        .context("Failed to store email")?;
                    stored_count += 1;

                    if newest_date.is_none() || email.date > newest_date.unwrap() {
                        newest_date = Some(email.date);
                    }
                }
            }

            debug!("SYNC: Stored {} new emails (filtered {} duplicates)", stored_count, emails.len() - stored_count);

            // Collect email IDs for tagging
            let new_email_ids: Vec<i64> = emails.iter()
                .filter(|e| last_message_date.is_none() || e.date > last_message_date.unwrap())
                .filter_map(|e| {
                    db.get_email_by_uid_and_account(e.uid, account_email, folder_id)
                        .ok()
                        .flatten()
                        .and_then(|email| email.id)
                })
                .collect();

            // Update sync metadata
            db.update_sync_metadata(account_email, folder_id, newest_date)
                .context("Failed to update sync metadata")?;

            // Run tagging rules on newly synced emails
            if !new_email_ids.is_empty() {
                debug!("SYNC: Running tagging rules on {} new emails", new_email_ids.len());

                match db.get_enabled_tagging_rules() {
                    Ok(rules) if !rules.is_empty() => {
                        let mut tags_applied = 0;

                        for email_id in &new_email_ids {
                            if let Ok(Some(email)) = db.get_email_by_id(*email_id) {
                                for rule in &rules {
                                    if evaluate_email_against_rule(&email, rule) {
                                        let applied_by = format!("rule:{}", rule.id);
                                        if db.add_tag_to_email(*email_id, rule.tag_id, &applied_by).is_ok() {
                                            tags_applied += 1;
                                        }
                                    }
                                }
                            }
                        }

                        debug!("SYNC: Applied {} tags to {} emails using {} rules", tags_applied, new_email_ids.len(), rules.len());
                    }
                    Ok(_) => {
                        debug!("SYNC: No enabled tagging rules found");
                    }
                    Err(e) => {
                        warn!("SYNC: Failed to get tagging rules: {}", e);
                    }
                }
            }

            // Evict old headers
            db.evict_old_headers(account_email, folder_id, max_headers)
                .context("Failed to evict old headers")?;
        }

        info!(
            "SYNC: Completed syncing folder {} for {}",
            folder_id, account_email
        );

        Ok(())
    }
}

impl Drop for SyncService {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
