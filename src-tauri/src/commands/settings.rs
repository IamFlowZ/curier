use crate::services::settings::AppSettings;
use crate::services::sync::{SyncService, SyncStatus};
use crate::state::AppState;
use log::debug;
use serde::Serialize;
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
pub struct CacheStats {
    pub email_count: i64,
    pub body_count: i64,
    pub db_size_bytes: u64,
}

#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    debug!("get_settings: Loading app settings");

    let settings_service = state.settings_service.lock().await;
    let settings = settings_service
        .load()
        .map_err(|e| format!("Failed to load settings: {}", e))?;

    debug!("get_settings: Settings loaded successfully");
    Ok(settings)
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    debug!("update_settings: Updating app settings");

    // Validate emails_per_page
    if settings.emails_per_page != 20 && settings.emails_per_page != 50 && settings.emails_per_page != 100 {
        return Err("emails_per_page must be 20, 50, or 100".to_string());
    }

    let settings_service = state.settings_service.lock().await;
    settings_service
        .update(settings)
        .map_err(|e| format!("Failed to update settings: {}", e))?;

    debug!("update_settings: Settings updated successfully");
    Ok(())
}

#[tauri::command]
pub async fn get_cache_stats(
    state: State<'_, AppState>,
) -> Result<CacheStats, String> {
    debug!("get_cache_stats: Getting cache statistics");

    let db = state.database.lock().await;
    let (email_count, body_count, db_size_bytes) = db
        .get_cache_stats()
        .map_err(|e| format!("Failed to get cache stats: {}", e))?;

    debug!("get_cache_stats: {} emails, {} bodies, {} bytes", email_count, body_count, db_size_bytes);
    Ok(CacheStats {
        email_count,
        body_count,
        db_size_bytes,
    })
}

#[tauri::command]
pub async fn clear_cache(
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!("clear_cache: Clearing all cache data");

    let db = state.database.lock().await;
    db.clear_all_cache()
        .map_err(|e| format!("Failed to clear cache: {}", e))?;

    debug!("clear_cache: Cache cleared successfully");
    Ok(())
}

#[tauri::command]
pub async fn get_account_colors(
    state: State<'_, AppState>,
) -> Result<Vec<(String, String)>, String> {
    debug!("get_account_colors: Getting account colors");

    let db = state.database.lock().await;
    let colors = db
        .get_account_colors()
        .map_err(|e| format!("Failed to get account colors: {}", e))?;

    Ok(colors)
}

#[tauri::command]
pub async fn set_account_color(
    state: State<'_, AppState>,
    account_email: String,
    color: String,
) -> Result<(), String> {
    debug!("set_account_color: Setting color for {}", account_email);

    let db = state.database.lock().await;
    db.set_account_color(&account_email, &color)
        .map_err(|e| format!("Failed to set account color: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn trigger_sync(app_handle: AppHandle) -> Result<(), String> {
    debug!("trigger_sync: Manual sync triggered");

    SyncService::sync_now(app_handle)
        .await
        .map_err(|e| format!("Failed to trigger sync: {}", e))?;

    debug!("trigger_sync: Sync completed successfully");
    Ok(())
}

#[tauri::command]
pub async fn get_sync_status(
    state: State<'_, AppState>,
) -> Result<SyncStatus, String> {
    debug!("get_sync_status: Getting sync status");

    let sync_service = state.sync_service.lock().await;

    if let Some(ref service) = *sync_service {
        let status = service.get_status().await;
        debug!("get_sync_status: Status retrieved");
        Ok(status)
    } else {
        debug!("get_sync_status: Sync service not initialized");
        Ok(SyncStatus::default())
    }
}
