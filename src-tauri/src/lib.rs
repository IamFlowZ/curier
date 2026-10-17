mod commands;
mod models;
mod services;
mod state;

use commands::auth::{is_authenticated, logout, start_google_oauth, start_microsoft_oauth, get_accounts, check_provider_credentials};
use commands::email::{fetch_emails, get_cached_emails, get_email_body, get_folders, fetch_emails_by_label, fetch_all_emails, list_attachments, download_attachment, send_email, import_emails};
use commands::feeds::{create_feed, get_feeds, get_feed, update_feed, delete_feed, reorder_feeds, fetch_feed_emails};
use commands::settings::{get_settings, update_settings, get_cache_stats, clear_cache, get_account_colors, set_account_color, trigger_sync, get_sync_status};
use commands::system::get_memory_info;
use commands::system::MemoryState;
use commands::tags::{
    create_tag, get_tags, update_tag, delete_tag,
    add_tag_to_email, remove_tag_from_email, get_email_tags,
    create_tagging_rule, get_tagging_rules, update_tagging_rule, delete_tagging_rule,
    run_tagging_rules,
};
use state::AppState;
use tauri::Manager;
use log::{info, warn};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            info!("App setup: Initializing services...");

            // Get app data directory
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Failed to get app data dir: {}", e))?;

            // Clone for later use
            let app_data_dir_clone = app_data_dir.clone();

            // Initialize AppState with database and settings services
            let app_state = AppState::new(app_data_dir)
                .map_err(|e| format!("Failed to initialize AppState: {}", e))?;

            // Store AppState in Tauri state
            app.manage(app_state);
            app.manage(MemoryState::new());

            info!("App setup: Services initialized");

            // Initialize sync service in the background
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                info!("App setup: Starting sync service...");
                let state: tauri::State<AppState> = app_handle.state();

                // Initialize accounts for syncing
                if let Err(e) = state.init_accounts_for_sync(app_data_dir_clone).await {
                    warn!("App setup: Failed to initialize accounts: {}", e);
                }

                // Start sync service
                if let Err(e) = state.init_sync_service(app_handle.clone()).await {
                    warn!("App setup: Failed to start sync service: {}", e);
                } else {
                    info!("App setup: Sync service started successfully");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            start_google_oauth,
            start_microsoft_oauth,
            is_authenticated,
            logout,
            get_accounts,
            check_provider_credentials,
            // Email commands
            fetch_emails,
            fetch_all_emails,
            get_cached_emails,
            get_email_body,
            get_folders,
            fetch_emails_by_label,
            list_attachments,
            download_attachment,
            send_email,
            import_emails,
            // Tag commands
            create_tag,
            get_tags,
            update_tag,
            delete_tag,
            add_tag_to_email,
            remove_tag_from_email,
            get_email_tags,
            // Tagging rule commands
            create_tagging_rule,
            get_tagging_rules,
            update_tagging_rule,
            delete_tagging_rule,
            run_tagging_rules,
            // Feed commands
            create_feed,
            get_feeds,
            get_feed,
            update_feed,
            delete_feed,
            reorder_feeds,
            fetch_feed_emails,
            // System commands
            get_memory_info,
            get_settings,
            update_settings,
            get_cache_stats,
            clear_cache,
            get_account_colors,
            set_account_color,
            trigger_sync,
            get_sync_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
