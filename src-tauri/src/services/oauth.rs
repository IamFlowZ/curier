use crate::models::auth::{AuthTokens, GoogleCredentials, OAuthConfig, StoredCredentials, AccountType};
use crate::services::credential_store::CredentialStore;
use anyhow::{Context, Result};
use log::{debug, warn};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse, TokenUrl,
};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Mutable token fields shared via interior mutability.
/// Allows multiple concurrent callers to observe refreshed tokens
/// without re-reading from disk or holding stale snapshots.
#[derive(Debug, Clone)]
pub struct TokenState {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

pub struct OAuthService {
    config: OAuthConfig,
    token_state: Arc<RwLock<TokenState>>,
}

impl OAuthService {
    pub fn new(config: OAuthConfig) -> Self {
        let token_state = TokenState {
            access_token: config.access_token.clone(),
            refresh_token: config.refresh_token.clone(),
            expires_at: config.expires_at,
        };
        Self {
            config,
            token_state: Arc::new(RwLock::new(token_state)),
        }
    }

    /// Get a clone of the shared token state handle.
    /// Callers can read this after API calls to sync refreshed tokens
    /// back to the AppState cache.
    pub fn token_state_handle(&self) -> Arc<RwLock<TokenState>> {
        Arc::clone(&self.token_state)
    }

    /// Check if an error indicates the user needs to re-authenticate
    /// (e.g., refresh token revoked or expired).
    /// Checks the full anyhow error chain since the original error may be wrapped with `.context()`.
    pub fn is_reauth_required_error(err: &anyhow::Error) -> bool {
        err.chain().any(|cause| cause.to_string().contains("REAUTH_REQUIRED"))
    }

    /// Load Google OAuth credentials from ~/.curier/google_credentials.json
    pub fn load_google_credentials() -> Result<GoogleCredentials> {
        let home_dir = dirs::home_dir()
            .context("Could not determine home directory")?;
        let creds_path = home_dir.join(".curier").join("google_credentials.json");

        let contents = std::fs::read_to_string(&creds_path)
            .context(format!(
                "Could not read credentials file at {}. Please create this file with your Google OAuth client_id and client_secret.",
                creds_path.display()
            ))?;

        let credentials: GoogleCredentials = serde_json::from_str(&contents)
            .context("Invalid JSON in credentials file. Expected format: {\"client_id\": \"...\", \"client_secret\": \"...\"}")?;

        if credentials.client_id.is_empty() || credentials.client_secret.is_empty() {
            anyhow::bail!("Credentials file must contain non-empty client_id and client_secret");
        }

        Ok(credentials)
    }

    /// Fetch user email from Google userinfo API
    pub async fn fetch_user_email(access_token: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let response = client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to fetch user info from Google")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Google userinfo API returned error {}: {}", status, body);
        }

        #[derive(serde::Deserialize)]
        struct UserInfo {
            email: String,
        }

        let user_info: UserInfo = response.json().await
            .context("Failed to parse user info response")?;

        Ok(user_info.email)
    }

    /// Create service from stored credentials (includes config + tokens)
    pub fn from_stored(app_data_dir: &PathBuf) -> Result<Option<Self>> {
        let config = Self::load_stored_credentials(app_data_dir)?;
        Ok(config.map(|c| Self::new(c)))
    }

    /// Generate PKCE challenge and verifier
    fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
        PkceCodeChallenge::new_random_sha256()
    }

    /// Generate OAuth URL - returns URL immediately for frontend to open
    pub fn generate_auth_url(&self) -> Result<(String, PkceCodeVerifier, CsrfToken)> {
        // Build OAuth client
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(self.config.auth_url.clone())?,
            Some(TokenUrl::new(self.config.token_url.clone())?),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);

        // Generate PKCE challenge
        let (pkce_challenge, pkce_verifier) = Self::generate_pkce();

        // Generate authorization URL
        let mut auth_url_builder = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent");

        // Add scopes
        for scope in &self.config.scopes {
            auth_url_builder = auth_url_builder.add_scope(Scope::new(scope.clone()));
        }

        let (auth_url, csrf_token) = auth_url_builder.url();
        let auth_url_string = auth_url.to_string();

        Ok((auth_url_string, pkce_verifier, csrf_token))
    }

    /// Complete OAuth flow - waits for callback and exchanges for tokens
    pub async fn complete_oauth_flow(
        &self,
        pkce_verifier: PkceCodeVerifier,
        csrf_token: CsrfToken,
        email: String,
        app_data_dir: &PathBuf,
    ) -> Result<AuthTokens> {
        // Wait for callback (this blocks until user completes auth in browser)
        let (code, state) = Self::wait_for_callback(&self.config.redirect_url)?;

        // Verify CSRF token
        if state != *csrf_token.secret() {
            anyhow::bail!("CSRF token mismatch");
        }

        // Build OAuth client for token exchange
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(self.config.auth_url.clone())?,
            Some(TokenUrl::new(self.config.token_url.clone())?),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);

        // Exchange code for tokens
        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await
            .map_err(|e| anyhow::anyhow!("Token exchange failed: {:?}", e))?;

        // Calculate expiration time
        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let tokens = AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone()),
            expires_at,
        };

        // Store tokens in encrypted file storage
        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Wait for OAuth callback on local server.
    /// Parses the port from the redirect URL (defaults to 8080).
    pub fn wait_for_callback(redirect_url: &str) -> Result<(String, String)> {
        let port = url::Url::parse(redirect_url)
            .ok()
            .and_then(|u| u.port())
            .unwrap_or(8080);
        let bind_addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&bind_addr)
            .context(format!("Failed to bind callback listener on {}", bind_addr))?;

        // Accept one connection
        let (mut stream, _) = listener.accept()?;
        let mut reader = BufReader::new(&stream);

        // Read HTTP request line
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

        // Parse query parameters from URL
        let redirect_url = request_line
            .split_whitespace()
            .nth(1)
            .context("Invalid HTTP request")?;

        let url = url::Url::parse(&format!("http://127.0.0.1{}", redirect_url))?;

        let code = url
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string())
            .context("No code in callback")?;

        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.to_string())
            .context("No state in callback")?;

        // Send success response to browser
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body style=\"font-family: -apple-system, sans-serif; text-align: center; padding: 50px;\"><h1 style=\"color: #10b981;\">✓ Authentication successful!</h1><p>You can close this window and return to Curier.</p></body></html>";
        stream.write_all(response.as_bytes())?;
        stream.flush()?;

        Ok((code, state))
    }

    /// Store tokens and OAuth credentials in encrypted file storage
    fn store_tokens(&self, tokens: &AuthTokens, email: &str, app_data_dir: &PathBuf) -> Result<()> {
        let credentials = StoredCredentials {
            client_id: self.config.client_id.clone(),
            client_secret: self.config.client_secret.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            email: Some(email.to_string()),
            account_type: self.config.account_type.clone(),
        };

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.store_account(email, &credentials)?;
        store.flush()?;

        Ok(())
    }

    /// Retrieve tokens from encrypted file storage
    pub fn get_tokens(&self, app_data_dir: &PathBuf) -> Result<Option<AuthTokens>> {
        debug!("get_tokens: Reading from credential store");

        let email = match &self.config.email {
            Some(email) => email.clone(),
            None => {
                debug!("get_tokens: No email in config, cannot retrieve tokens");
                return Ok(None);
            }
        };

        let store = CredentialStore::new(app_data_dir.clone())?;
        match store.load_account(&email)? {
            Some(credentials) => {
                debug!("get_tokens: Credentials loaded successfully");
                Ok(Some(AuthTokens {
                    access_token: credentials.access_token,
                    refresh_token: credentials.refresh_token,
                    expires_at: credentials.expires_at,
                }))
            }
            None => {
                debug!("get_tokens: No credentials found");
                Ok(None)
            }
        }
    }

    /// Check if current tokens are valid (not expired).
    /// Reads from the shared token state (not the immutable config snapshot).
    pub async fn is_authenticated(&self) -> Result<bool> {
        let state = self.token_state.read().await;
        if let Some(expires_at) = state.expires_at {
            let now = chrono::Utc::now().timestamp();
            Ok(expires_at > now)
        } else {
            Ok(false)
        }
    }

    /// Refresh access token using refresh token
    pub async fn refresh_token(&self, app_data_dir: &PathBuf) -> Result<AuthTokens> {
        let refresh_token = self.config.refresh_token
            .clone()
            .context("No refresh token available")?;

        let old_refresh_token = self.config.refresh_token.clone();

        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(self.config.auth_url.clone())?,
            Some(TokenUrl::new(self.config.token_url.clone())?),
        );

        let token_response = client
            .exchange_refresh_token(&oauth2::RefreshToken::new(refresh_token))
            .request_async(async_http_client)
            .await
            .context("Failed to refresh token")?;

        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let tokens = AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or(old_refresh_token),
            expires_at,
        };

        // Get email from config to preserve it
        let email = self.config.email.clone().unwrap_or_default();
        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Get valid access token using double-check locking.
    ///
    /// Phase 1 (read lock): if the token is still valid, return it immediately.
    /// Phase 2 (write lock): re-check expiry (another task may have refreshed
    /// while we waited for the lock), then refresh if still expired.
    /// The write lock serializes refreshes per-account, eliminating races.
    pub async fn get_valid_access_token(&self, app_data_dir: &PathBuf) -> Result<String> {
        // Phase 1: Read lock — fast path for valid tokens
        {
            let state = self.token_state.read().await;
            let now = chrono::Utc::now().timestamp();
            let is_expired = state.expires_at.map(|exp| exp <= now).unwrap_or(true);

            if !is_expired {
                if let Some(ref token) = state.access_token {
                    debug!("get_valid_access_token: Using cached access token (read lock)");
                    return Ok(token.clone());
                }
            }
        }

        // Phase 2: Write lock — double-check then refresh
        let mut state = self.token_state.write().await;
        let now = chrono::Utc::now().timestamp();
        let is_expired = state.expires_at.map(|exp| exp <= now).unwrap_or(true);

        if !is_expired {
            if let Some(ref token) = state.access_token {
                debug!("get_valid_access_token: Another task already refreshed (write lock)");
                return Ok(token.clone());
            }
        }

        debug!("get_valid_access_token: Token expired, refreshing under write lock");
        let refresh_token = state.refresh_token.clone()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;

        let tokens = self.refresh_token_with_value(&refresh_token, app_data_dir).await?;

        // Update shared token state so all callers see the new tokens
        state.access_token = Some(tokens.access_token.clone());
        state.refresh_token = tokens.refresh_token;
        state.expires_at = Some(tokens.expires_at);

        Ok(tokens.access_token)
    }

    /// Refresh token using provided refresh token value
    async fn refresh_token_with_value(&self, refresh_token: &str, app_data_dir: &PathBuf) -> Result<AuthTokens> {
        debug!("refresh_token_with_value: Starting refresh");

        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(self.config.auth_url.clone())?,
            Some(TokenUrl::new(self.config.token_url.clone())?),
        );

        debug!("refresh_token_with_value: Sending refresh request to Google");
        let token_response = match client
            .exchange_refresh_token(&oauth2::RefreshToken::new(refresh_token.to_string()))
            .request_async(async_http_client)
            .await {
                Ok(response) => {
                    debug!("refresh_token_with_value: Token refresh successful");
                    response
                }
                Err(e) => {
                    let error_string = format!("{:?}", e);
                    warn!("refresh_token_with_value: Token refresh FAILED: {}", error_string);
                    if error_string.contains("invalid_grant") {
                        return Err(anyhow::anyhow!("REAUTH_REQUIRED: Token has been revoked or expired. Please re-authenticate."));
                    }
                    return Err(anyhow::anyhow!("Failed to refresh token: {:?}", e));
                }
            };

        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let tokens = AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or_else(|| Some(refresh_token.to_string())),
            expires_at,
        };

        // Store updated tokens
        let email = self.config.email.clone().unwrap_or_default();
        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Load stored OAuth credentials from encrypted file storage
    pub fn load_stored_credentials(app_data_dir: &PathBuf) -> Result<Option<OAuthConfig>> {
        debug!("CREDENTIAL_STORE: Reading from encrypted storage");

        let store = CredentialStore::new(app_data_dir.clone())?;
        let all_accounts = store.load_all()?;

        if all_accounts.is_empty() {
            debug!("CREDENTIAL_STORE: No credentials found");
            return Ok(None);
        }

        // Return the first account (for legacy single-account compatibility)
        let (email, credentials) = all_accounts.into_iter().next().unwrap();

        let mut config = OAuthConfig::default();
        config.client_id = credentials.client_id;
        config.client_secret = credentials.client_secret;
        config.email = Some(email.clone());
        config.access_token = Some(credentials.access_token);
        config.refresh_token = credentials.refresh_token;
        config.expires_at = Some(credentials.expires_at);

        debug!("CREDENTIAL_STORE: Loaded config with email: {}", email);
        Ok(Some(config))
    }

    /// Get stored email from config
    pub fn get_stored_email(&self) -> Result<Option<String>> {
        Ok(self.config.email.clone())
    }

    /// Clear stored tokens and credentials (logout)
    pub fn clear_tokens(&self, app_data_dir: &PathBuf) -> Result<()> {
        let store = CredentialStore::new(app_data_dir.clone())?;
        store.clear_all()?;
        Ok(())
    }

    // ============ Multi-Account Methods ============

    /// Store account credentials in encrypted file storage
    pub fn store_account_credentials(email: &str, config: &OAuthConfig, tokens: &AuthTokens, account_type: AccountType, app_data_dir: &PathBuf) -> Result<()> {
        debug!("CREDENTIAL_STORE: Storing credentials for account: {}", email);

        let credentials = StoredCredentials {
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            email: Some(email.to_string()),
            account_type,
        };

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.store_account(email, &credentials)?;

        debug!("CREDENTIAL_STORE: Successfully stored credentials for: {}", email);
        Ok(())
    }

    /// Load all accounts from encrypted file storage
    pub fn load_all_accounts(app_data_dir: &PathBuf) -> Result<Vec<(String, OAuthConfig)>> {
        debug!("CREDENTIAL_STORE: Loading all accounts");

        let store = CredentialStore::new(app_data_dir.clone())?;
        let all_accounts = store.load_all()?;

        let result: Vec<(String, OAuthConfig)> = all_accounts
            .into_iter()
            .map(|(email, credentials)| {
                let mut config = OAuthConfig::defaults_for(&credentials.account_type);
                config.client_id = credentials.client_id;
                config.client_secret = credentials.client_secret;
                config.email = Some(email.clone());
                config.account_type = credentials.account_type;
                config.access_token = Some(credentials.access_token);
                config.refresh_token = credentials.refresh_token;
                config.expires_at = Some(credentials.expires_at);
                (email, config)
            })
            .collect();

        debug!("CREDENTIAL_STORE: Successfully loaded {} accounts", result.len());
        Ok(result)
    }

    /// Load credentials for a specific account
    pub fn load_account_credentials(email: &str, app_data_dir: &PathBuf) -> Result<Option<OAuthConfig>> {
        debug!("CREDENTIAL_STORE: Loading credentials for: {}", email);

        let store = CredentialStore::new(app_data_dir.clone())?;
        match store.load_account(email)? {
            Some(credentials) => {
                let mut config = OAuthConfig::defaults_for(&credentials.account_type);
                config.client_id = credentials.client_id;
                config.client_secret = credentials.client_secret;
                config.email = credentials.email;
                config.account_type = credentials.account_type;
                config.access_token = Some(credentials.access_token);
                config.refresh_token = credentials.refresh_token;
                config.expires_at = Some(credentials.expires_at);
                Ok(Some(config))
            }
            None => Ok(None),
        }
    }

    /// Remove account from encrypted file storage
    pub fn remove_account_credentials(email: &str, app_data_dir: &PathBuf) -> Result<()> {
        debug!("CREDENTIAL_STORE: Removing credentials for: {}", email);

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.remove_account(email)?;

        Ok(())
    }

    /// Update tokens for a specific account
    pub fn update_account_tokens(email: &str, tokens: &AuthTokens, app_data_dir: &PathBuf) -> Result<()> {
        debug!("CREDENTIAL_STORE: Updating tokens for: {}", email);

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.update_account_tokens(
            email,
            tokens.access_token.clone(),
            tokens.refresh_token.clone(),
            tokens.expires_at,
        )?;
        store.flush()?;

        debug!("CREDENTIAL_STORE: Successfully updated tokens for: {}", email);
        Ok(())
    }
}
