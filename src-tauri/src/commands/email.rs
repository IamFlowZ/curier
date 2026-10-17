use crate::models::auth::AccountType;
use crate::models::email::{AttachmentInfo, EmailSummary, EmailBody, PaginatedEmailResponse};
use crate::services::database::DatabaseService;
use crate::services::email_provider::{EmailFolder, EmailProvider};
use crate::services::gmail_api::GmailApiService;
use crate::services::oauth::{OAuthService, TokenState};
use crate::services::outlook_api::OutlookApiService;
use crate::services::rule_engine::evaluate_email_against_rule;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::RwLock;
use log::{debug, info, warn};

/// Create the appropriate email provider for an account based on its type.
fn create_email_provider(config: crate::models::auth::OAuthConfig, app_data_dir: PathBuf) -> (Arc<dyn EmailProvider>, Arc<RwLock<TokenState>>) {
    let oauth_service = OAuthService::new(config.clone());
    let token_handle = oauth_service.token_state_handle();
    let provider: Arc<dyn EmailProvider> = match config.account_type {
        AccountType::Outlook => Arc::new(OutlookApiService::new(oauth_service, app_data_dir)),
        _ => Arc::new(GmailApiService::new(oauth_service, app_data_dir)),
    };
    (provider, token_handle)
}

/// Sanitize email HTML using a permissive ammonia::Builder configuration.
///
/// Allows `<style>` tags, `style`/`class` attributes, and common email layout
/// attributes (width, height, align, bgcolor, etc.) so that newsletters and
/// marketing emails retain their visual formatting. Scripts and event handlers
/// are still stripped by ammonia's defaults.
fn sanitize_email_html(html: &str) -> String {
    let mut builder = ammonia::Builder::default();

    // Remove <style> from clean_content_tags (which strips tag AND content)
    // before adding it to the allowed tags list. Ammonia panics if a tag
    // appears in both sets simultaneously.
    builder.rm_clean_content_tags(&["style"]);

    // Allow <style> tags so embedded CSS works
    builder.add_tags(&["style"]);

    // Allow style and class on all elements
    builder.add_generic_attributes(&["style", "class", "id", "dir", "lang"]);

    // Common email layout attributes on table-related elements
    let table_attrs: &[&str] = &[
        "width", "height", "align", "valign", "bgcolor", "background",
        "cellpadding", "cellspacing", "border", "role", "colspan", "rowspan",
    ];
    for tag in &["table", "td", "tr", "th", "tbody", "thead", "tfoot"] {
        builder.add_tag_attributes(tag, table_attrs);
    }

    // Allow sizing/alignment on images and divs
    builder.add_tag_attributes("img", &["width", "height", "alt", "title"]);
    builder.add_tag_attributes("div", &["align"]);
    builder.add_tag_attributes("p", &["align"]);
    // span is already in the default tag allowlist; no extra attributes needed
    builder.add_tag_attributes("a", &["target", "title"]);

    // Allow font tag attributes (common in older email HTML)
    builder.add_tags(&["font", "center"]);
    builder.add_tag_attributes("font", &["color", "size", "face"]);

    builder.clean(html).to_string()
}

fn get_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))
}

/// Helper to get OAuth config for a specific account
fn get_account_oauth_config(state: &State<'_, AppState>, account_email: &str, app_data_dir: &PathBuf) -> Result<crate::models::auth::OAuthConfig, String> {
    // First try multi-account cache
    if let Ok(Some(config)) = state.get_account_config(account_email, app_data_dir) {
        debug!("Using multi-account config for: {}", account_email);
        return Ok(config);
    }

    // Fall back to loading directly from credential store
    if let Ok(Some(config)) = OAuthService::load_account_credentials(account_email, app_data_dir) {
        debug!("Loaded config from credential store for: {}", account_email);
        // Cache it for next time
        state.set_account_config(account_email, config.clone());
        return Ok(config);
    }

    Err(format!("No credentials found for account: {}", account_email))
}

/// Sync refreshed tokens from the OAuth token state back to the AppState cache.
/// Called after API calls that may have triggered a token refresh so the next
/// command picks up the fresh tokens instead of re-refreshing.
async fn sync_tokens_to_cache(
    token_state: &Arc<RwLock<TokenState>>,
    account_email: &str,
    state: &State<'_, AppState>,
) {
    let ts = token_state.read().await;
    state.update_account_tokens(
        account_email,
        ts.access_token.clone(),
        ts.refresh_token.clone(),
        ts.expires_at,
    );
}

#[tauri::command]
pub async fn fetch_emails(
    email: String,
    page: u32,
    limit: u32,
    search_query: Option<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<PaginatedEmailResponse, String> {
    debug!("fetch_emails: Starting cache-first fetch for account: {} (page: {}, limit: {}, search: {:?})", email, page, limit, search_query);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let offset = ((page - 1) * limit) as usize;

    // When searching, always use cache - search is a cache-only operation
    let is_searching = search_query.as_ref().is_some_and(|q| !q.trim().is_empty());

    if is_searching {
        let db = state.database.lock().await;
        let (emails, total_count) = db
            .search_cached_emails_by_folder_paginated(&email, "INBOX", offset, limit as usize, search_query.as_deref())
            .map_err(|e| format!("Failed to search emails: {}", e))?;
        drop(db);

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;
        return Ok(PaginatedEmailResponse {
            emails,
            total_count: total_count as u32,
            current_page: page,
            total_pages,
        });
    }

    // Step 1: Try to load from cache first
    let db = state.database.lock().await;
    let (cached_emails, cached_count) = db
        .get_cached_emails_by_folder_paginated(&email, "INBOX", offset, limit as usize)
        .map_err(|e| format!("Failed to get cached emails: {}", e))?;

    // Use cache only if we have a full page of results (otherwise fetch from API to get accurate total)
    let cache_is_sufficient = !cached_emails.is_empty() && cached_emails.len() == limit as usize;

    if cache_is_sufficient {
        debug!("fetch_emails: Cache HIT - returning {} cached emails instantly (total: {})", cached_emails.len(), cached_count);

        let total_pages = ((cached_count as f64) / (limit as f64)).ceil() as u32;

        // Trigger background sync (non-blocking)
        let app_handle_clone = app_handle.clone();
        let email_clone = email.clone();
        tauri::async_runtime::spawn(async move {
            debug!("fetch_emails: Triggering background sync for {}", email_clone);
            if let Err(e) = crate::services::sync::SyncService::sync_now(app_handle_clone).await {
                warn!("fetch_emails: Background sync failed: {}", e);
            }
        });

        drop(db);

        return Ok(PaginatedEmailResponse {
            emails: cached_emails,
            total_count: cached_count as u32,
            current_page: page,
            total_pages,
        });
    }

    debug!("fetch_emails: Cache insufficient (have {} of {} requested) - fetching from server", cached_emails.len(), limit);
    drop(db);

    // Step 2: Cache miss - fetch from email provider
    let config = get_account_oauth_config(&state, &email, &app_data_dir)?;
    debug!("fetch_emails: OAuth config loaded for {}", email);

    let (provider, token_handle) = create_email_provider(config, app_data_dir.clone());

    debug!("fetch_emails: Fetching paginated emails from provider...");
    let (emails, total_count) = provider
        .fetch_emails_paginated("INBOX", offset as u32, limit)
        .await
        .map_err(|e| {
            warn!("fetch_emails: Email fetch failed: {}", e);
            format!("Failed to fetch emails: {}", e)
        })?;
    sync_tokens_to_cache(&token_handle, &email, &state).await;
    debug!("fetch_emails: Fetched {} emails (total: {})", emails.len(), total_count);

    // Step 3: Store in cache
    debug!("fetch_emails: Storing emails in cache...");
    let db = state.database.lock().await;

    let mut newest_date = None;
    for email_item in &emails {
        db.upsert_email_with_context(email_item, &email, "INBOX")
            .map_err(|e| format!("Failed to store email: {}", e))?;

        if newest_date.is_none() || email_item.date > newest_date.unwrap() {
            newest_date = Some(email_item.date);
        }
    }

    // Update sync metadata
    db.update_sync_metadata(&email, "INBOX", newest_date)
        .map_err(|e| format!("Failed to update sync metadata: {}", e))?;

    // Evict old headers
    let settings_service = state.settings_service.lock().await;
    let settings = settings_service.load()
        .map_err(|e| format!("Failed to load settings: {}", e))?;
    drop(settings_service);

    db.evict_old_headers(&email, "INBOX", settings.max_headers_per_folder)
        .map_err(|e| format!("Failed to evict old headers: {}", e))?;

    drop(db);

    debug!("fetch_emails: Emails stored in cache successfully");

    let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

    Ok(PaginatedEmailResponse {
        emails,
        total_count,
        current_page: page,
        total_pages,
    })
}

#[tauri::command]
pub async fn get_cached_emails(state: State<'_, AppState>, limit: usize) -> Result<Vec<EmailSummary>, String> {
    let db = state.database.lock().await;
    db.get_recent_emails(limit)
        .map_err(|e| format!("Failed to get emails: {}", e))
}

#[tauri::command]
pub async fn get_email_body(
    app_handle: AppHandle,
    message_id: String,
    account_email: String,
    state: State<'_, AppState>,
) -> Result<EmailBody, String> {
    debug!("get_email_body: Cache-first fetch for message ID: {} (account: {})", message_id, account_email);

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Step 1: Get email_id from message_id
    let db = state.database.lock().await;
    let email_id = db
        .get_email_id_by_message_id(&message_id)
        .map_err(|e| format!("Failed to lookup email: {}", e))?;

    if let Some(email_id) = email_id {
        // Step 2: Try to load from cache (with LRU timestamp update)
        let cached_body = db
            .get_email_body_lru(email_id)
            .map_err(|e| format!("Failed to get cached body: {}", e))?;

        if let Some(body) = cached_body {
            debug!("get_email_body: Cache HIT - returning cached body instantly");

            // Sanitize HTML on the return path (raw HTML is stored in cache)
            let sanitized_html = body.html_body.map(|html| {
                debug!("get_email_body: Sanitizing cached HTML body ({} bytes)", html.len());
                sanitize_email_html(&html)
            });

            drop(db);

            return Ok(EmailBody {
                email_id,
                html_body: sanitized_html,
                text_body: body.text_body,
            });
        }

        debug!("get_email_body: Cache MISS - fetching from server");
    } else {
        debug!("get_email_body: Email not found in database - fetching from server");
    }

    drop(db);

    // Step 3: Cache miss - fetch from email provider
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;

    let (provider, token_handle) = create_email_provider(config, app_data_dir);

    let (html_body, text_body) = provider
        .get_email_body(&message_id)
        .await
        .map_err(|e| format!("Failed to fetch email body: {}", e))?;
    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    // Step 4: Store raw HTML in cache (if we have email_id)
    // Sanitization happens on the return path so future sanitizer changes apply to cached bodies.
    if let Some(email_id) = email_id {
        let body_to_cache = EmailBody {
            email_id,
            html_body: html_body.clone(),
            text_body: text_body.clone(),
        };

        let db = state.database.lock().await;
        let settings_service = state.settings_service.lock().await;
        let max_bodies = settings_service.load().map(|s| s.max_bodies_total).unwrap_or(100);
        drop(settings_service);

        if let Err(e) = db.store_email_body_lru(email_id, &body_to_cache, max_bodies) {
            warn!("get_email_body: Failed to cache body: {}", e);
        } else {
            debug!("get_email_body: Body cached successfully");
        }
        drop(db);
    }

    // Sanitize HTML on the return path
    let sanitized_html = html_body.map(|html| {
        debug!("get_email_body: Sanitizing HTML body ({} bytes)", html.len());
        sanitize_email_html(&html)
    });

    debug!("get_email_body: Retrieved body - HTML: {}, Text: {}",
        sanitized_html.is_some(), text_body.is_some());

    Ok(EmailBody {
        email_id: email_id.unwrap_or(0),
        html_body: sanitized_html,
        text_body,
    })
}

#[tauri::command]
pub async fn get_folders(
    app_handle: AppHandle,
    account_email: String,
    state: State<'_, AppState>,
) -> Result<Vec<EmailFolder>, String> {
    debug!("get_folders: Fetching folder list for account: {}", account_email);

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Get account-specific OAuth config
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;

    let (provider, token_handle) = create_email_provider(config, app_data_dir);

    let folders = provider
        .list_folders()
        .await
        .map_err(|e| format!("Failed to fetch folders: {}", e))?;
    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    debug!("get_folders: Retrieved {} folders", folders.len());
    Ok(folders)
}

#[tauri::command]
pub async fn fetch_emails_by_label(
    label_id: String,
    page: u32,
    limit: u32,
    account_email: String,
    search_query: Option<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<PaginatedEmailResponse, String> {
    debug!("fetch_emails_by_label: Fetching from {} for account: {} (page: {}, limit: {}, search: {:?})", label_id, account_email, page, limit, search_query);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let offset = ((page - 1) * limit) as usize;

    // When searching, always use cache - search is a cache-only operation
    let is_searching = search_query.as_ref().is_some_and(|q| !q.trim().is_empty());

    if is_searching {
        let db = state.database.lock().await;
        let (emails, total_count) = db
            .search_cached_emails_by_folder_paginated(&account_email, &label_id, offset, limit as usize, search_query.as_deref())
            .map_err(|e| format!("Failed to search emails: {}", e))?;
        drop(db);

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;
        return Ok(PaginatedEmailResponse {
            emails,
            total_count: total_count as u32,
            current_page: page,
            total_pages,
        });
    }

    // Step 1: Try to load from cache first
    let db = state.database.lock().await;
    let (cached_emails, cached_count) = db
        .get_cached_emails_by_folder_paginated(&account_email, &label_id, offset, limit as usize)
        .map_err(|e| format!("Failed to get cached emails: {}", e))?;

    // Use cache only if we have a full page of results (otherwise fetch from API to get accurate total)
    let cache_is_sufficient = !cached_emails.is_empty() && cached_emails.len() == limit as usize;

    if cache_is_sufficient {
        debug!("fetch_emails_by_label: Cache HIT - returning {} cached emails instantly (total: {})", cached_emails.len(), cached_count);

        let total_pages = ((cached_count as f64) / (limit as f64)).ceil() as u32;

        // Trigger background sync (non-blocking)
        let app_handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::services::sync::SyncService::sync_now(app_handle_clone).await {
                warn!("fetch_emails_by_label: Background sync failed: {}", e);
            }
        });

        drop(db);

        return Ok(PaginatedEmailResponse {
            emails: cached_emails,
            total_count: cached_count as u32,
            current_page: page,
            total_pages,
        });
    }

    debug!("fetch_emails_by_label: Cache insufficient - fetching from server");
    drop(db);

    // Step 2: Cache miss - fetch from email provider
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;

    let (provider, token_handle) = create_email_provider(config, app_data_dir.clone());

    let (emails, total_count) = provider
        .fetch_emails_paginated(&label_id, offset as u32, limit)
        .await
        .map_err(|e| format!("Failed to fetch emails: {}", e))?;
    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    debug!("fetch_emails_by_label: Fetched {} emails (total: {})", emails.len(), total_count);

    // Step 3: Store in cache
    let db = state.database.lock().await;

    let mut newest_date = None;
    for email_item in &emails {
        db.upsert_email_with_context(email_item, &account_email, &label_id)
            .map_err(|e| format!("Failed to store email: {}", e))?;

        if newest_date.is_none() || email_item.date > newest_date.unwrap() {
            newest_date = Some(email_item.date);
        }
    }

    db.update_sync_metadata(&account_email, &label_id, newest_date)
        .map_err(|e| format!("Failed to update sync metadata: {}", e))?;

    let settings_service = state.settings_service.lock().await;
    let settings = settings_service.load()
        .map_err(|e| format!("Failed to load settings: {}", e))?;
    drop(settings_service);

    db.evict_old_headers(&account_email, &label_id, settings.max_headers_per_folder)
        .map_err(|e| format!("Failed to evict old headers: {}", e))?;

    drop(db);

    let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;

    Ok(PaginatedEmailResponse {
        emails,
        total_count,
        current_page: page,
        total_pages,
    })
}

#[tauri::command]
pub async fn fetch_all_emails(
    page: u32,
    limit: u32,
    search_query: Option<String>,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<PaginatedEmailResponse, String> {
    debug!("fetch_all_emails: Fetching from all accounts (page: {}, limit: {}, search: {:?})...", page, limit, search_query);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let offset = ((page - 1) * limit) as usize;

    // When searching, always use cache - search is a cache-only operation
    let is_searching = search_query.as_ref().is_some_and(|q| !q.trim().is_empty());

    if is_searching {
        let db = state.database.lock().await;
        let (emails, total_count) = db
            .search_all_cached_emails_paginated(offset, limit as usize, search_query.as_deref())
            .map_err(|e| format!("Failed to search emails: {}", e))?;
        drop(db);

        let total_pages = ((total_count as f64) / (limit as f64)).ceil() as u32;
        return Ok(PaginatedEmailResponse {
            emails,
            total_count: total_count as u32,
            current_page: page,
            total_pages,
        });
    }

    // Step 1: Try to load from cache first
    let db = state.database.lock().await;
    let (cached_emails, cached_count) = db
        .get_all_cached_emails_paginated(offset, limit as usize)
        .map_err(|e| format!("Failed to get cached emails: {}", e))?;

    // Use cache only if we have a full page of results (otherwise fetch from API to get accurate total)
    let cache_is_sufficient = !cached_emails.is_empty() && cached_emails.len() == limit as usize;

    if cache_is_sufficient {
        debug!("fetch_all_emails: Cache HIT - returning {} cached emails instantly (total: {})", cached_emails.len(), cached_count);

        let total_pages = ((cached_count as f64) / (limit as f64)).ceil() as u32;

        // Trigger background sync (non-blocking)
        let app_handle_clone = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::services::sync::SyncService::sync_now(app_handle_clone).await {
                warn!("fetch_all_emails: Background sync failed: {}", e);
            }
        });

        drop(db);

        return Ok(PaginatedEmailResponse {
            emails: cached_emails,
            total_count: cached_count as u32,
            current_page: page,
            total_pages,
        });
    }

    debug!("fetch_all_emails: Cache insufficient - fetching from server");
    drop(db);

    // Step 2: Cache miss - fetch from Gmail API
    let accounts = state.get_all_account_configs(&app_data_dir)?;

    if accounts.is_empty() {
        debug!("fetch_all_emails: No authenticated accounts");
        return Err("Not authenticated".to_string());
    }

    // Fetch from all accounts concurrently, proportionally fewer per account
    let num_accounts = accounts.len();
    let per_account_limit = (limit as usize / num_accounts) + 1;

    debug!(
        "fetch_all_emails: Fetching {} emails per account from {} accounts",
        per_account_limit, num_accounts
    );

    let fetch_futures: Vec<_> = accounts
        .into_iter()
        .map(|(account_email, config)| {
            let app_data_dir = app_data_dir.clone();
            async move {
                debug!("fetch_all_emails: Fetching from account: {}", account_email);
                let (provider, token_handle) = create_email_provider(config, app_data_dir);

                let (emails, count) = match provider
                    .fetch_emails_paginated("INBOX", 0, per_account_limit as u32)
                    .await
                {
                    Ok((mut emails, count)) => {
                        for email in &mut emails {
                            email.account_email = Some(account_email.clone());
                        }
                        debug!(
                            "fetch_all_emails: Got {} emails from {}",
                            emails.len(),
                            account_email
                        );
                        (emails, count)
                    }
                    Err(e) => {
                        warn!(
                            "fetch_all_emails: Error fetching from {}: {}",
                            account_email, e
                        );
                        (Vec::new(), 0)
                    }
                };
                (account_email, emails, count, token_handle)
            }
        })
        .collect();

    let results = futures::future::join_all(fetch_futures).await;

    let mut all_emails: Vec<EmailSummary> = Vec::new();
    let mut total_estimate = 0u32;

    for (account_email, emails, count, token_handle) in results {
        // Sync refreshed tokens back to cache for each account
        let ts = token_handle.read().await;
        state.update_account_tokens(
            &account_email,
            ts.access_token.clone(),
            ts.refresh_token.clone(),
            ts.expires_at,
        );
        drop(ts);

        all_emails.extend(emails);
        total_estimate += count;
    }

    // Sort by date descending
    all_emails.sort_by(|a, b| b.date.cmp(&a.date));

    // Apply pagination to combined results
    let start = offset;
    let end = std::cmp::min(start + limit as usize, all_emails.len());
    let page_emails = if start < all_emails.len() {
        all_emails[start..end].to_vec()
    } else {
        Vec::new()
    };

    // Store in cache
    let db = state.database.lock().await;
    for email_item in &page_emails {
        let account_email = email_item.account_email.as_deref().unwrap_or("unknown");
        db.upsert_email_with_context(email_item, account_email, "INBOX")
            .map_err(|e| format!("Failed to store email: {}", e))?;
    }
    drop(db);

    let total_pages = ((total_estimate as f64) / (limit as f64)).ceil() as u32;

    debug!("fetch_all_emails: Retrieved {} emails total (page {} of {})", page_emails.len(), page, total_pages);

    // Run tagging rules on newly synced emails (synchronously since we have the lock)
    let email_ids: Vec<i64> = page_emails.iter().filter_map(|e| e.id).collect();
    if !email_ids.is_empty() {
        // Reacquire the database lock to run tagging rules
        let db = state.database.lock().await;
        if let Err(e) = run_tagging_rules_sync(&db, Some(email_ids)) {
            warn!("fetch_all_emails: Failed to run tagging rules: {}", e);
        }
        drop(db);
    }

    Ok(PaginatedEmailResponse {
        emails: page_emails,
        total_count: total_estimate,
        current_page: page,
        total_pages,
    })
}

#[tauri::command]
pub async fn list_attachments(
    app_handle: AppHandle,
    message_id: String,
    account_email: String,
    state: State<'_, AppState>,
) -> Result<Vec<AttachmentInfo>, String> {
    debug!("list_attachments: Fetching for message {} (account: {})", message_id, account_email);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;
    let (provider, token_handle) = create_email_provider(config, app_data_dir);

    let attachments = provider
        .list_attachments(&message_id)
        .await
        .map_err(|e| format!("Failed to list attachments: {}", e))?;
    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    debug!("list_attachments: Found {} attachments", attachments.len());
    Ok(attachments)
}

#[tauri::command]
pub async fn download_attachment(
    app_handle: AppHandle,
    message_id: String,
    attachment_id: String,
    filename: String,
    account_email: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    debug!("download_attachment: {} for message {} (account: {})", filename, message_id, account_email);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;
    let (provider, token_handle) = create_email_provider(config, app_data_dir);

    // Fetch attachment bytes from provider
    let bytes = provider
        .download_attachment(&message_id, &attachment_id)
        .await
        .map_err(|e| format!("Failed to download attachment: {}", e))?;
    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    debug!("download_attachment: Downloaded {} bytes, showing save dialog", bytes.len());

    // Show native save dialog (async callback bridged with oneshot channel)
    let (tx, rx) = tokio::sync::oneshot::channel();
    app_handle
        .dialog()
        .file()
        .set_file_name(&filename)
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let file_path = rx
        .await
        .map_err(|_| "Save dialog channel error".to_string())?;

    match file_path {
        Some(path) => {
            let path_buf = path
                .as_path()
                .ok_or_else(|| "Invalid save path".to_string())?
                .to_path_buf();
            tokio::task::spawn_blocking(move || std::fs::write(&path_buf, &bytes))
                .await
                .map_err(|e| format!("Task join error: {}", e))?
                .map_err(|e| format!("Failed to write file: {}", e))?;
            debug!("download_attachment: File saved successfully");
            Ok(true)
        }
        None => {
            debug!("download_attachment: User cancelled save dialog");
            Ok(false)
        }
    }
}

#[tauri::command]
pub async fn send_email(
    app_handle: AppHandle,
    account_email: String,
    to: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    debug!("send_email: Sending from {} to {}", account_email, to);

    let app_data_dir = get_app_data_dir(&app_handle)?;
    let config = get_account_oauth_config(&state, &account_email, &app_data_dir)?;
    let (provider, token_handle) = create_email_provider(config, app_data_dir);

    provider
        .send_email(&to, &subject, &body, in_reply_to.as_deref())
        .await
        .map_err(|e| format!("Failed to send email: {}", e))?;

    sync_tokens_to_cache(&token_handle, &account_email, &state).await;

    log::info!("send_email: Email sent successfully from {}", account_email);
    Ok(())
}

/// Synchronous function to run tagging rules on emails
fn run_tagging_rules_sync(
    db: &DatabaseService,
    email_ids: Option<Vec<i64>>,
) -> Result<(), String> {
    // Get enabled rules
    let rules = db
        .get_enabled_tagging_rules()
        .map_err(|e| format!("Failed to get tagging rules: {}", e))?;

    if rules.is_empty() {
        return Ok(());
    }

    // Get email IDs to process
    let ids_to_process = match email_ids {
        Some(ids) => ids,
        None => db
            .get_all_email_ids()
            .map_err(|e| format!("Failed to get email IDs: {}", e))?,
    };

    let mut tags_applied = 0;

    // Process each email against each rule
    for email_id in &ids_to_process {
        if let Some(email) = db
            .get_email_by_id(*email_id)
            .map_err(|e| format!("Failed to get email: {}", e))?
        {
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

    if tags_applied > 0 {
        debug!(
            "run_tagging_rules_sync: Applied {} tags to {} emails",
            tags_applied,
            ids_to_process.len()
        );
    }

    Ok(())
}

/// Progress event payload emitted during email import.
#[derive(Clone, Serialize)]
struct ImportProgress {
    account_email: String,
    imported: u32,
    total_estimate: Option<u32>,
    phase: String, // "fetching" or "storing"
    done: bool,
}

/// Import emails for an account using a specified strategy.
///
/// Strategies:
/// - `"days_30"`, `"days_60"`, `"days_90"` — time-gated import
/// - `"count_100"`, `"count_1000"`, `"count_10000"` — quantity-based import
/// - `"all"` — import all emails
/// - `"skip"` — do nothing
#[tauri::command]
pub async fn import_emails(
    account_email: String,
    strategy: String,
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    info!("import_emails: Starting import for {} with strategy '{}'", account_email, strategy);

    if strategy == "skip" {
        return Ok(0);
    }

    let app_data_dir = app_handle.path().app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    // Resolve the provider for this account
    let provider = {
        let accounts = state.accounts.lock().await;
        accounts.get(&account_email).cloned()
            .ok_or_else(|| format!("Account not found: {}", account_email))?
    };

    // Determine fetch parameters based on strategy
    let (query, max_count): (Option<String>, u32) = match strategy.as_str() {
        "days_30" => {
            let after = chrono::Utc::now() - chrono::Duration::days(30);
            (Some(format!("after:{}", after.format("%Y/%m/%d"))), 10000)
        }
        "days_60" => {
            let after = chrono::Utc::now() - chrono::Duration::days(60);
            (Some(format!("after:{}", after.format("%Y/%m/%d"))), 10000)
        }
        "days_90" => {
            let after = chrono::Utc::now() - chrono::Duration::days(90);
            (Some(format!("after:{}", after.format("%Y/%m/%d"))), 10000)
        }
        "count_100" => (None, 100),
        "count_1000" => (None, 1000),
        "count_10000" => (None, 10000),
        "all" => (None, u32::MAX),
        _ => return Err(format!("Unknown import strategy: {}", strategy)),
    };

    let folder_id = "INBOX";
    let mut total_imported: u32 = 0;
    let mut newest_date: Option<i64> = None;

    // Emit initial progress
    let _ = app_handle.emit("import-progress", ImportProgress {
        account_email: account_email.clone(),
        imported: 0,
        total_estimate: if max_count == u32::MAX { None } else { Some(max_count) },
        phase: "fetching".to_string(),
        done: false,
    });

    // Build the Gmail query for time-gated imports
    let gmail_query = query.as_deref();

    // Try to use bulk paginated fetch for Gmail accounts
    let account_config = state.get_account_config(&account_email, &app_data_dir)
        .ok()
        .flatten();

    if let Some(config) = account_config {
        if config.account_type == AccountType::Gmail {
            let oauth_service = OAuthService::new(config);
            let gmail_service = GmailApiService::new(oauth_service, app_data_dir.clone());

            // Fetch all batches (paginated via nextPageToken)
            let batches = gmail_service.fetch_emails_bulk(
                folder_id,
                gmail_query,
                max_count,
            ).await.map_err(|e| format!("Failed to fetch emails: {}", e))?;

            // Store each batch to DB
            for batch in &batches {
                let db = state.database.lock().await;
                for email in batch {
                    db.upsert_email_with_context(email, &account_email, folder_id)
                        .map_err(|e| format!("Failed to store email: {}", e))?;
                    total_imported += 1;

                    if newest_date.is_none() || email.date > newest_date.unwrap() {
                        newest_date = Some(email.date);
                    }
                }
                if let Some(nd) = newest_date {
                    let _ = db.update_sync_metadata(&account_email, folder_id, Some(nd));
                }
                drop(db);

                let _ = app_handle.emit("import-progress", ImportProgress {
                    account_email: account_email.clone(),
                    imported: total_imported,
                    total_estimate: if max_count == u32::MAX { None } else { Some(max_count) },
                    phase: "storing".to_string(),
                    done: false,
                });
            }
        } else {
            // Outlook or other — single fetch
            let emails = provider
                .fetch_emails(folder_id, max_count)
                .await
                .map_err(|e| format!("Failed to fetch emails: {}", e))?;

            let db = state.database.lock().await;
            for email in &emails {
                db.upsert_email_with_context(email, &account_email, folder_id)
                    .map_err(|e| format!("Failed to store email: {}", e))?;
                total_imported += 1;
                if newest_date.is_none() || email.date > newest_date.unwrap() {
                    newest_date = Some(email.date);
                }
            }
            if let Some(nd) = newest_date {
                let _ = db.update_sync_metadata(&account_email, folder_id, Some(nd));
            }
        }
    } else {
        return Err(format!("Could not load config for account: {}", account_email));
    }

    // Emit completion
    let _ = app_handle.emit("import-progress", ImportProgress {
        account_email: account_email.clone(),
        imported: total_imported,
        total_estimate: Some(total_imported),
        phase: "done".to_string(),
        done: true,
    });

    info!("import_emails: Imported {} emails for {}", total_imported, account_email);
    Ok(total_imported)
}

// Rule evaluation logic is in crate::services::rule_engine
