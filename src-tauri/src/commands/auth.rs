use crate::models::auth::{AuthState, OAuthConfig, Account, AccountType, GoogleCredentials};
use crate::services::credential_store::CredentialStore;
use crate::services::email_provider::EmailProvider;
use crate::services::gmail_api::GmailApiService;
use crate::services::oauth::OAuthService;
use crate::services::outlook_api::OutlookApiService;
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use log::{debug, info, warn};

fn get_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))
}

#[derive(Serialize)]
pub struct ProviderCredentialStatus {
    pub exists: bool,
    pub client_id: Option<String>,
}

/// Check whether OAuth credentials are available for a given provider.
/// Checks the keychain (credential store) first, then falls back to the JSON config files.
#[tauri::command]
pub fn check_provider_credentials(
    app_handle: AppHandle,
    provider: String,
) -> Result<ProviderCredentialStatus, String> {
    // First, check the credential store (keychain) for any existing account of this provider type
    let app_data_dir = get_app_data_dir(&app_handle)?;
    let target_type = match provider.as_str() {
        "outlook" => AccountType::Outlook,
        _ => AccountType::Gmail,
    };

    if let Ok(accounts) = OAuthService::load_all_accounts(&app_data_dir) {
        for (_, config) in &accounts {
            if config.account_type == target_type && !config.client_id.is_empty() {
                return Ok(ProviderCredentialStatus {
                    exists: true,
                    client_id: Some(config.client_id.clone()),
                });
            }
        }
    }

    // Fall back to checking the JSON config files
    let home_dir = dirs::home_dir()
        .ok_or_else(|| "Could not determine home directory".to_string())?;

    let filename = match provider.as_str() {
        "outlook" => "microsoft_credentials.json",
        _ => "google_credentials.json",
    };

    let creds_path = home_dir.join(".curier").join(filename);

    match std::fs::read_to_string(&creds_path) {
        Ok(contents) => {
            if let Ok(creds) = serde_json::from_str::<GoogleCredentials>(&contents) {
                if !creds.client_id.is_empty() && !creds.client_secret.is_empty() {
                    return Ok(ProviderCredentialStatus {
                        exists: true,
                        client_id: Some(creds.client_id),
                    });
                }
            }
            Ok(ProviderCredentialStatus {
                exists: false,
                client_id: None,
            })
        }
        Err(_) => Ok(ProviderCredentialStatus {
            exists: false,
            client_id: None,
        }),
    }
}

#[tauri::command]
pub async fn start_google_oauth(
    app_handle: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<AuthState, String> {
    info!("start_google_oauth: Starting Google OAuth flow");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Resolve credentials: explicit params > keychain (existing account) > JSON file
    let (resolved_client_id, resolved_client_secret) = match (client_id, client_secret) {
        (Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => {
            debug!("start_google_oauth: Using credentials provided by frontend");
            (id, secret)
        }
        _ => {
            // Try to reuse credentials from an existing Gmail account in the keychain
            if let Ok(accounts) = OAuthService::load_all_accounts(&app_data_dir) {
                if let Some((_, existing)) = accounts.iter().find(|(_, c)| c.account_type == AccountType::Gmail && !c.client_id.is_empty()) {
                    debug!("start_google_oauth: Reusing credentials from existing Gmail account in keychain");
                    (existing.client_id.clone(), existing.client_secret.clone())
                } else {
                    let credentials = OAuthService::load_google_credentials()
                        .map_err(|e| format!("{}", e))?;
                    debug!("start_google_oauth: Loaded credentials from config file");
                    (credentials.client_id, credentials.client_secret)
                }
            } else {
                let credentials = OAuthService::load_google_credentials()
                    .map_err(|e| format!("{}", e))?;
                debug!("start_google_oauth: Loaded credentials from config file");
                (credentials.client_id, credentials.client_secret)
            }
        }
    };

    let mut config = OAuthConfig::default();
    config.client_id = resolved_client_id;
    config.client_secret = resolved_client_secret;

    let service = OAuthService::new(config.clone());

    // Generate the auth URL
    debug!("start_google_oauth: Generating auth URL...");
    let (url, pkce_verifier, csrf_token) = service
        .generate_auth_url()
        .map_err(|e| {
            warn!("start_google_oauth: Failed to generate auth URL: {}", e);
            format!("Failed to generate auth URL: {}", e)
        })?;

    // Emit URL to frontend so browser can open
    debug!("start_google_oauth: Auth URL generated, emitting to frontend...");
    let _ = window.emit("oauth-url", url.clone());

    // Wait for callback and exchange for tokens (this blocks)
    debug!("start_google_oauth: Waiting for OAuth callback...");

    // We use a placeholder email for now, will fetch real email after token exchange
    let placeholder_email = "pending@oauth.temp".to_string();

    match service.complete_oauth_flow(pkce_verifier, csrf_token, placeholder_email.clone(), &app_data_dir).await {
        Ok(tokens) => {
            debug!("start_google_oauth: Token exchange successful, fetching user email...");

            // Fetch the actual user email from Google
            let email = OAuthService::fetch_user_email(&tokens.access_token)
                .await
                .map_err(|e| format!("Failed to fetch user email: {}", e))?;

            debug!("start_google_oauth: Got user email: {}", email);

            // Update config with email
            config.email = Some(email.clone());

            // Store in multi-account credential storage
            if let Err(e) = OAuthService::store_account_credentials(&email, &config, &tokens, AccountType::Gmail, &app_data_dir) {
                warn!("start_google_oauth: Failed to store credentials: {}", e);
            }

            // Remove the placeholder account that was stored during OAuth flow
            if let Err(e) = OAuthService::remove_account_credentials(&placeholder_email, &app_data_dir) {
                debug!("start_google_oauth: Could not remove placeholder account (may not exist): {}", e);
            }

            // Flush all credential changes to keychain in a single write
            if let Err(e) = CredentialStore::new(app_data_dir.clone()).and_then(|s| s.flush()) {
                warn!("start_google_oauth: Failed to flush credentials to keychain: {}", e);
            }

            // Update config with tokens
            config.access_token = Some(tokens.access_token);
            config.refresh_token = tokens.refresh_token;
            config.expires_at = Some(tokens.expires_at);

            // Cache config in app state
            state.set_account_config(&email, config.clone());

            // Clear any re-auth required flag since we just authenticated
            state.clear_reauth_required(&email);

            // Register account for syncing
            debug!("start_google_oauth: Registering account for syncing...");
            let oauth_service = OAuthService::new(config);
            let gmail_service: Arc<dyn EmailProvider> = Arc::new(GmailApiService::new(oauth_service, app_data_dir));
            state.register_email_provider(email.clone(), gmail_service).await;
            debug!("start_google_oauth: Account registered for syncing");

            let auth_state = AuthState {
                is_authenticated: true,
                email: Some(email),
            };
            info!("start_google_oauth: OAuth flow completed successfully");
            Ok(auth_state)
        }
        Err(e) => {
            warn!("start_google_oauth: OAuth flow failed: {}", e);
            Err(format!("OAuth flow failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn start_microsoft_oauth(
    app_handle: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<AuthState, String> {
    info!("start_microsoft_oauth: Starting Microsoft OAuth flow");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Resolve credentials: explicit params > keychain (existing account) > JSON file
    let (resolved_client_id, resolved_client_secret) = match (client_id, client_secret) {
        (Some(id), Some(secret)) if !id.is_empty() && !secret.is_empty() => {
            debug!("start_microsoft_oauth: Using credentials provided by frontend");
            (id, secret)
        }
        _ => {
            if let Ok(accounts) = OAuthService::load_all_accounts(&app_data_dir) {
                if let Some((_, existing)) = accounts.iter().find(|(_, c)| c.account_type == AccountType::Outlook && !c.client_id.is_empty()) {
                    debug!("start_microsoft_oauth: Reusing credentials from existing Outlook account in keychain");
                    (existing.client_id.clone(), existing.client_secret.clone())
                } else {
                    let credentials = load_microsoft_credentials()
                        .map_err(|e| format!("{}", e))?;
                    debug!("start_microsoft_oauth: Loaded Microsoft credentials from config file");
                    (credentials.client_id, credentials.client_secret)
                }
            } else {
                let credentials = load_microsoft_credentials()
                    .map_err(|e| format!("{}", e))?;
                debug!("start_microsoft_oauth: Loaded Microsoft credentials from config file");
                (credentials.client_id, credentials.client_secret)
            }
        }
    };

    let mut config = OAuthConfig::microsoft_defaults();
    config.client_id = resolved_client_id;
    config.client_secret = resolved_client_secret;

    let service = OAuthService::new(config.clone());

    // Generate the auth URL
    debug!("start_microsoft_oauth: Generating auth URL...");
    let (url, pkce_verifier, csrf_token) = service
        .generate_auth_url()
        .map_err(|e| {
            warn!("start_microsoft_oauth: Failed to generate auth URL: {}", e);
            format!("Failed to generate auth URL: {}", e)
        })?;

    // Emit URL to frontend so browser can open
    debug!("start_microsoft_oauth: Auth URL generated, emitting to frontend...");
    let _ = window.emit("oauth-url", url.clone());

    // Wait for callback on port 8081 and exchange for tokens
    debug!("start_microsoft_oauth: Waiting for OAuth callback...");

    let placeholder_email = "pending@microsoft.oauth.temp".to_string();

    match service.complete_oauth_flow(pkce_verifier, csrf_token, placeholder_email.clone(), &app_data_dir).await {
        Ok(tokens) => {
            debug!("start_microsoft_oauth: Token exchange successful, fetching user email...");

            // Fetch the actual user email from Microsoft Graph
            let email = fetch_microsoft_user_email(&tokens.access_token)
                .await
                .map_err(|e| format!("Failed to fetch user email: {}", e))?;

            debug!("start_microsoft_oauth: Got user email: {}", email);

            // Update config with email
            config.email = Some(email.clone());

            // Store in multi-account credential storage
            if let Err(e) = OAuthService::store_account_credentials(&email, &config, &tokens, AccountType::Outlook, &app_data_dir) {
                warn!("start_microsoft_oauth: Failed to store credentials: {}", e);
            }

            // Remove the placeholder account
            if let Err(e) = OAuthService::remove_account_credentials(&placeholder_email, &app_data_dir) {
                debug!("start_microsoft_oauth: Could not remove placeholder account (may not exist): {}", e);
            }

            // Flush all credential changes to keychain in a single write
            if let Err(e) = CredentialStore::new(app_data_dir.clone()).and_then(|s| s.flush()) {
                warn!("start_microsoft_oauth: Failed to flush credentials to keychain: {}", e);
            }

            // Update config with tokens
            config.access_token = Some(tokens.access_token);
            config.refresh_token = tokens.refresh_token;
            config.expires_at = Some(tokens.expires_at);

            // Cache config in app state
            state.set_account_config(&email, config.clone());

            // Clear any re-auth required flag
            state.clear_reauth_required(&email);

            // Register Outlook account for syncing
            debug!("start_microsoft_oauth: Registering Outlook account for syncing...");
            let oauth_service = OAuthService::new(config);
            let outlook_service: Arc<dyn EmailProvider> = Arc::new(OutlookApiService::new(oauth_service, app_data_dir));
            state.register_email_provider(email.clone(), outlook_service).await;
            debug!("start_microsoft_oauth: Account registered for syncing");

            let auth_state = AuthState {
                is_authenticated: true,
                email: Some(email),
            };
            info!("start_microsoft_oauth: OAuth flow completed successfully");
            Ok(auth_state)
        }
        Err(e) => {
            warn!("start_microsoft_oauth: OAuth flow failed: {}", e);
            Err(format!("OAuth flow failed: {}", e))
        }
    }
}

/// Load Microsoft OAuth credentials from ~/.curier/microsoft_credentials.json
fn load_microsoft_credentials() -> anyhow::Result<GoogleCredentials> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let creds_path = home_dir.join(".curier").join("microsoft_credentials.json");

    let contents = std::fs::read_to_string(&creds_path)
        .map_err(|_| anyhow::anyhow!(
            "Could not read credentials file at {}. Please create this file with your Microsoft OAuth client_id and client_secret.",
            creds_path.display()
        ))?;

    let credentials: GoogleCredentials = serde_json::from_str(&contents)
        .map_err(|_| anyhow::anyhow!("Invalid JSON in Microsoft credentials file. Expected format: {{\"client_id\": \"...\", \"client_secret\": \"...\"}}"))?;

    if credentials.client_id.is_empty() || credentials.client_secret.is_empty() {
        anyhow::bail!("Microsoft credentials file must contain non-empty client_id and client_secret");
    }

    Ok(credentials)
}

/// Fetch user email from Microsoft Graph API
async fn fetch_microsoft_user_email(access_token: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://graph.microsoft.com/v1.0/me")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch user info from Microsoft: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Microsoft Graph /me API returned error {}: {}", status, body);
    }

    #[derive(serde::Deserialize)]
    struct MicrosoftUser {
        mail: Option<String>,
        #[serde(rename = "userPrincipalName")]
        user_principal_name: Option<String>,
    }

    let user: MicrosoftUser = response.json().await
        .map_err(|e| anyhow::anyhow!("Failed to parse Microsoft user info: {}", e))?;

    // Prefer mail field, fall back to userPrincipalName
    user.mail
        .or(user.user_principal_name)
        .ok_or_else(|| anyhow::anyhow!("Microsoft Graph /me returned no email address"))
}

#[tauri::command]
pub async fn start_oauth_flow(
    app_handle: AppHandle,
    client_id: String,
    client_secret: String,
    email: String,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<AuthState, String> {
    info!("start_oauth_flow: Starting with email: {}", email);

    let app_data_dir = get_app_data_dir(&app_handle)?;

    let mut config = OAuthConfig::default();
    config.client_id = client_id.clone();
    config.client_secret = client_secret.clone();
    config.email = Some(email.clone());

    debug!("start_oauth_flow: Creating OAuth service...");
    let service = OAuthService::new(config.clone());

    // Step 1: Generate the auth URL (non-blocking)
    debug!("start_oauth_flow: Generating auth URL...");
    let (url, pkce_verifier, csrf_token) = service
        .generate_auth_url()
        .map_err(|e| {
            warn!("start_oauth_flow: Failed to generate auth URL: {}", e);
            format!("Failed to generate auth URL: {}", e)
        })?;

    debug!("start_oauth_flow: Auth URL generated, emitting to frontend...");
    // Step 2: Emit URL to frontend immediately so browser can open
    let _ = window.emit("oauth-url", url.clone());

    debug!("start_oauth_flow: URL emitted, starting OAuth callback wait...");

    // Step 3: Wait for callback and exchange for tokens (this blocks)
    match service.complete_oauth_flow(pkce_verifier, csrf_token, email.clone(), &app_data_dir).await {
        Ok(tokens) => {
            info!("start_oauth_flow: OAuth flow completed successfully");

            // Store in multi-account credential storage
            if let Err(e) = OAuthService::store_account_credentials(&email, &config, &tokens, AccountType::Gmail, &app_data_dir) {
                warn!("start_oauth_flow: Failed to store multi-account credentials: {}", e);
            }

            // Flush credential changes to keychain in a single write
            if let Err(e) = CredentialStore::new(app_data_dir.clone()).and_then(|s| s.flush()) {
                warn!("start_oauth_flow: Failed to flush credentials to keychain: {}", e);
            }

            // Update config with the tokens before caching
            config.access_token = Some(tokens.access_token);
            config.refresh_token = tokens.refresh_token;
            config.expires_at = Some(tokens.expires_at);

            // Cache config in app state
            state.set_account_config(&email, config.clone());

            // Clear any re-auth required flag since we just authenticated
            state.clear_reauth_required(&email);

            // Register account for syncing
            debug!("start_oauth_flow: Registering account for syncing...");
            let oauth_service = OAuthService::new(config);
            let gmail_service: Arc<dyn EmailProvider> = Arc::new(GmailApiService::new(oauth_service, app_data_dir));
            state.register_email_provider(email.clone(), gmail_service).await;
            debug!("start_oauth_flow: Account registered for syncing");

            let auth_state = AuthState {
                is_authenticated: true,
                email: Some(email),
            };
            debug!("start_oauth_flow: Returning auth state: {:?}", auth_state);
            Ok(auth_state)
        }
        Err(e) => {
            warn!("start_oauth_flow: OAuth flow failed: {}", e);
            Err(format!("OAuth flow failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn is_authenticated(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<AuthState, String> {
    debug!("is_authenticated: Command called");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Check multi-account cache for any account
    let all_configs = state.get_all_account_configs(&app_data_dir)?;

    if all_configs.is_empty() {
        debug!("is_authenticated: No stored credentials");
        return Ok(AuthState {
            is_authenticated: false,
            email: None,
        });
    }

    // Check the first account's validity
    let (email, config) = all_configs.into_iter().next().unwrap();
    let service = OAuthService::new(config.clone());

    // Check if token is expired (reads from shared token state)
    let authenticated = service
        .is_authenticated()
        .await
        .map_err(|e| format!("Failed to check authentication: {}", e))?;

    if authenticated {
        debug!("is_authenticated: Token still valid, returning authenticated=true");
        return Ok(AuthState {
            is_authenticated: true,
            email: Some(email),
        });
    }

    // Token is expired - try to refresh
    debug!("is_authenticated: Token expired, attempting refresh...");
    if config.refresh_token.is_some() {
        match service.get_valid_access_token(&app_data_dir).await {
            Ok(_token) => {
                debug!("is_authenticated: Token refresh successful!");
                // Sync refreshed tokens from shared state back to AppState cache
                let token_handle = service.token_state_handle();
                let ts = token_handle.read().await;
                state.update_account_tokens(
                    &email,
                    ts.access_token.clone(),
                    ts.refresh_token.clone(),
                    ts.expires_at,
                );
                drop(ts);
                return Ok(AuthState {
                    is_authenticated: true,
                    email: Some(email),
                });
            }
            Err(e) => {
                debug!("is_authenticated: Token refresh failed: {}", e);
            }
        }
    } else {
        debug!("is_authenticated: No refresh token available");
    }

    debug!("is_authenticated: Returning authenticated=false, email={}", email);

    Ok(AuthState {
        is_authenticated: false,
        email: Some(email),
    })
}

#[tauri::command]
pub async fn logout(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    email: Option<String>,
) -> Result<(), String> {
    info!("logout: Command called");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    match email {
        Some(account_email) => {
            // Log out specific account
            info!("logout: Logging out account: {}", account_email);

            // Delete all account data from the database (emails, sync metadata, etc.)
            {
                let db = state.database.lock().await;
                if let Err(e) = db.delete_account_data(&account_email) {
                    warn!("logout: Failed to delete account data for {}: {}", account_email, e);
                }
            }

            OAuthService::remove_account_credentials(&account_email, &app_data_dir)
                .map_err(|e| format!("Failed to logout account {}: {}", account_email, e))?;

            // Flush to keychain
            if let Err(e) = CredentialStore::new(app_data_dir).and_then(|s| s.flush()) {
                warn!("logout: Failed to flush credentials to keychain: {}", e);
            }

            // Remove from in-memory cache
            state.remove_account(&account_email);

            info!("logout: Successfully logged out account: {}", account_email);
        }
        None => {
            // Log out all accounts
            info!("logout: Logging out all accounts");

            let all_configs = state.get_all_account_configs(&app_data_dir)?;

            // Delete all account data from the database
            {
                let db = state.database.lock().await;
                for (account_email, _) in &all_configs {
                    if let Err(e) = db.delete_account_data(account_email) {
                        warn!("logout: Failed to delete account data for {}: {}", account_email, e);
                    }
                }
            }

            for (account_email, _) in &all_configs {
                if let Err(e) = OAuthService::remove_account_credentials(account_email, &app_data_dir) {
                    warn!("logout: Failed to remove credentials for {}: {}", account_email, e);
                }
            }

            // Flush all removals to keychain in one write
            if let Err(e) = CredentialStore::new(app_data_dir).and_then(|s| s.flush()) {
                warn!("logout: Failed to flush credentials to keychain: {}", e);
            }

            state.clear_all_accounts();

            info!("logout: Successfully logged out {} accounts", all_configs.len());
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn clear_credentials(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("clear_credentials: Clearing all stored credentials");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    let all_configs = state.get_all_account_configs(&app_data_dir)?;
    for (account_email, _) in &all_configs {
        if let Err(e) = OAuthService::remove_account_credentials(account_email, &app_data_dir) {
            warn!("clear_credentials: Failed to remove credentials for {}: {}", account_email, e);
        }
    }

    state.clear_all_accounts();

    info!("clear_credentials: All credentials cleared");
    Ok(())
}

#[tauri::command]
pub async fn get_access_token(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Use the first available account
    let all_configs = state.get_all_account_configs(&app_data_dir)?;
    let (_, config) = all_configs
        .into_iter()
        .next()
        .ok_or_else(|| "Not authenticated - no stored credentials".to_string())?;

    let service = OAuthService::new(config);

    service
        .get_valid_access_token(&app_data_dir)
        .await
        .map_err(|e| format!("Failed to get access token: {}", e))
}

#[tauri::command]
pub async fn get_accounts(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<Account>, String> {
    debug!("get_accounts: Fetching all accounts");

    let app_data_dir = get_app_data_dir(&app_handle)?;

    // Get all accounts from multi-account storage
    let all_configs = state.get_all_account_configs(&app_data_dir)?;
    let reauth_accounts = state.get_reauth_required_accounts();

    if !all_configs.is_empty() {
        let accounts: Vec<Account> = all_configs
            .into_iter()
            .filter_map(|(email, config)| {
                let account_id = email.replace("@", "-at-").replace(".", "-");
                let auth_status = if reauth_accounts.contains(&email) {
                    Some("reauth_required".to_string())
                } else {
                    Some("ok".to_string())
                };
                Some(Account {
                    id: account_id,
                    account_type: config.account_type.clone(),
                    email: email.clone(),
                    display_name: email,
                    auth_status,
                })
            })
            .collect();
        debug!("get_accounts: Found {} accounts", accounts.len());
        Ok(accounts)
    } else {
        debug!("get_accounts: No accounts found");
        Ok(vec![])
    }
}
