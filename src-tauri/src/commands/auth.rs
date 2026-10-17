use crate::models::auth::{AuthState, OAuthConfig, Account, AccountType, GoogleCredentials};
use crate::services::credential_store::CredentialStore;
use crate::services::email_provider::EmailProvider;
use crate::services::gmail_api::GmailApiService;
use crate::services::oauth::{ClientSecretRequirement, OAuthCallbackListener, OAuthService};
use crate::services::outlook_api::OutlookApiService;
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use log::{debug, info, warn};

const GOOGLE_CREDENTIALS_FILENAME: &str = "google_credentials.json";
const MICROSOFT_CREDENTIALS_FILENAME: &str = "microsoft_credentials.json";
const PROVIDER_OUTLOOK: &str = "outlook";
/// Temporary account key used while the real address is still unknown.
const GOOGLE_PLACEHOLDER_ACCOUNT: &str = "pending@oauth.temp";
const MICROSOFT_PLACEHOLDER_ACCOUNT: &str = "pending@microsoft.oauth.temp";
const MICROSOFT_GRAPH_ME_URL: &str = "https://graph.microsoft.com/v1.0/me";

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

    let (filename, secret_requirement) = match provider.as_str() {
        PROVIDER_OUTLOOK => (MICROSOFT_CREDENTIALS_FILENAME, ClientSecretRequirement::Optional),
        _ => (GOOGLE_CREDENTIALS_FILENAME, ClientSecretRequirement::Required),
    };

    let creds_path = home_dir.join(".curier").join(filename);

    match OAuthService::load_client_credentials_from_path(&creds_path, secret_requirement) {
        Ok(creds) => Ok(ProviderCredentialStatus {
            exists: true,
            client_id: Some(creds.client_id),
        }),
        Err(_) => Ok(ProviderCredentialStatus {
            exists: false,
            client_id: None,
        }),
    }
}

/// Run the loopback authorization-code flow for `config`: bind the callback
/// listener, emit the auth URL to the frontend, wait for the browser callback
/// and exchange the code for tokens stored under `placeholder_email`.
async fn run_loopback_oauth_flow(
    service: &OAuthService,
    window: &tauri::Window,
    placeholder_email: &str,
    app_data_dir: &PathBuf,
    log_prefix: &str,
) -> Result<crate::models::auth::AuthTokens, String> {
    let listener = OAuthCallbackListener::bind()
        .await
        .map_err(|e| {
            warn!("{}: Failed to bind callback listener: {}", log_prefix, e);
            format!("Failed to start OAuth callback listener: {}", e)
        })?;

    debug!("{}: Generating auth URL for redirect {}", log_prefix, listener.redirect_url());
    let (url, pkce_verifier, csrf_token) = service
        .generate_auth_url(listener.redirect_url())
        .map_err(|e| {
            warn!("{}: Failed to generate auth URL: {}", log_prefix, e);
            format!("Failed to generate auth URL: {}", e)
        })?;

    // Emit URL to frontend so browser can open
    debug!("{}: Auth URL generated, emitting to frontend...", log_prefix);
    let _ = window.emit("oauth-url", url);

    debug!("{}: Waiting for OAuth callback...", log_prefix);
    service
        .complete_oauth_flow(listener, pkce_verifier, csrf_token, placeholder_email.to_string(), app_data_dir)
        .await
        .map_err(|e| {
            warn!("{}: OAuth flow failed: {}", log_prefix, e);
            format!("OAuth flow failed: {}", e)
        })
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
    let placeholder_email = GOOGLE_PLACEHOLDER_ACCOUNT.to_string();

    match run_loopback_oauth_flow(&service, &window, &placeholder_email, &app_data_dir, "start_google_oauth").await {
        Ok(tokens) => {
            debug!("start_google_oauth: Token exchange successful, fetching user email...");

            // Fetch the actual user email from Google
            let email = OAuthService::fetch_user_email(&tokens.access_token)
                .await
                .map_err(|e| format!("Failed to fetch user email: {}", e))?;

            debug!("start_google_oauth: Got user email");

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
        Err(e) => Err(e),
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

    // Resolve credentials: explicit params > keychain (existing account) > JSON file.
    // Microsoft "Mobile and desktop" registrations are public clients, so the
    // secret is optional; only the client_id is required.
    let (resolved_client_id, resolved_client_secret) = match client_id {
        Some(id) if !id.is_empty() => {
            debug!("start_microsoft_oauth: Using credentials provided by frontend");
            (id, client_secret.unwrap_or_default())
        }
        _ => {
            let existing_outlook_account = OAuthService::load_all_accounts(&app_data_dir)
                .ok()
                .and_then(|accounts| {
                    accounts
                        .into_iter()
                        .find(|(_, c)| c.account_type == AccountType::Outlook && !c.client_id.is_empty())
                });
            match existing_outlook_account {
                Some((_, existing)) => {
                    debug!("start_microsoft_oauth: Reusing credentials from existing Outlook account in keychain");
                    (existing.client_id, existing.client_secret)
                }
                None => {
                    let credentials = load_microsoft_credentials()
                        .map_err(|e| format!("{}", e))?;
                    debug!("start_microsoft_oauth: Loaded Microsoft credentials from config file");
                    (credentials.client_id, credentials.client_secret)
                }
            }
        }
    };

    let mut config = OAuthConfig::microsoft_defaults();
    config.client_id = resolved_client_id;
    config.client_secret = resolved_client_secret;

    let service = OAuthService::new(config.clone());
    let placeholder_email = MICROSOFT_PLACEHOLDER_ACCOUNT.to_string();

    match run_loopback_oauth_flow(&service, &window, &placeholder_email, &app_data_dir, "start_microsoft_oauth").await {
        Ok(tokens) => {
            debug!("start_microsoft_oauth: Token exchange successful, fetching user email...");

            // Fetch the actual user email from Microsoft Graph
            let email = fetch_microsoft_user_email(&tokens.access_token)
                .await
                .map_err(|e| format!("Failed to fetch user email: {}", e))?;

            debug!("start_microsoft_oauth: Got user email");

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
        Err(e) => Err(e),
    }
}

/// Load Microsoft OAuth credentials from ~/.curier/microsoft_credentials.json.
/// The client secret may be empty for public-client app registrations.
fn load_microsoft_credentials() -> anyhow::Result<GoogleCredentials> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let creds_path = home_dir.join(".curier").join(MICROSOFT_CREDENTIALS_FILENAME);
    OAuthService::load_client_credentials_from_path(&creds_path, ClientSecretRequirement::Optional)
}

/// Fetch user email from Microsoft Graph API
async fn fetch_microsoft_user_email(access_token: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .get(MICROSOFT_GRAPH_ME_URL)
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
            debug!("logout: Logging out account: {}", account_email);

            // Best-effort server-side revocation before the local copy is deleted
            if let Ok(Some(config)) = state.get_account_config(&account_email, &app_data_dir) {
                revoke_tokens_best_effort(&config).await;
            }

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

            debug!("logout: Successfully logged out account");
        }
        None => {
            // Log out all accounts
            info!("logout: Logging out all accounts");

            let all_configs = state.get_all_account_configs(&app_data_dir)?;

            for (_, config) in &all_configs {
                revoke_tokens_best_effort(config).await;
            }

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

/// Revoke the account's refresh token at the provider. Failures are logged and
/// never block local logout.
async fn revoke_tokens_best_effort(config: &OAuthConfig) {
    if let Err(e) = OAuthService::revoke_refresh_token(config).await {
        warn!("logout: Token revocation failed (continuing with local logout): {}", e);
    }
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
