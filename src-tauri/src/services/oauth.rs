use crate::models::auth::{AccountType, AuthTokens, GoogleCredentials, OAuthConfig, StoredCredentials};
use crate::services::credential_store::CredentialStore;
use anyhow::{Context, Result};
use log::{debug, warn};
use oauth2::basic::{BasicClient, BasicErrorResponse};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope,
    TokenResponse, TokenUrl,
};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

/// Loopback address the OAuth callback listener binds to. Port 0 lets the OS
/// pick a free ephemeral port, which is then embedded in the redirect URI.
const CALLBACK_LISTENER_BIND_ADDRESS: &str = "127.0.0.1:0";

/// How long the whole authorization flow may take before the listener gives up.
const CALLBACK_OVERALL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// How long a single accepted connection may take to deliver its request line.
/// Browsers open speculative "preconnect" sockets that never send anything;
/// this keeps such sockets from wedging the listener.
const CALLBACK_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

const CALLBACK_QUERY_PARAM_CODE: &str = "code";
const CALLBACK_QUERY_PARAM_STATE: &str = "state";
const CALLBACK_QUERY_PARAM_ERROR: &str = "error";

const CALLBACK_SUCCESS_RESPONSE: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
<!doctype html><html><head><meta charset=\"utf-8\"><title>Curier</title></head>\
<body style=\"font-family: -apple-system, sans-serif; text-align: center; padding: 50px;\">\
<h1 style=\"color: #10b981;\">Authentication successful</h1>\
<p>You can close this window and return to Curier.</p></body></html>";

const CALLBACK_BAD_REQUEST_RESPONSE: &str = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n\
Invalid OAuth callback.";

const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const GOOGLE_TOKEN_REVOCATION_URL: &str = "https://oauth2.googleapis.com/revoke";

/// OAuth 2.0 error code returned when a refresh token has been revoked or expired.
const OAUTH_ERROR_CODE_INVALID_GRANT: &str = "invalid_grant";

/// Marker embedded in error messages so callers can detect that the user must re-authenticate.
pub const REAUTH_REQUIRED_MARKER: &str = "REAUTH_REQUIRED";

/// Unix permission bits that grant group or world access.
const GROUP_AND_WORLD_PERMISSION_BITS: u32 = 0o077;

/// An `oauth2` client with the authorization and token endpoints configured.
type ConfiguredOAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// Whether a credentials file must contain a `client_secret`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientSecretRequirement {
    Required,
    Optional,
}

/// Mutable token fields shared via interior mutability.
/// Allows multiple concurrent callers to observe refreshed tokens
/// without re-reading from disk or holding stale snapshots.
#[derive(Debug, Clone)]
pub struct TokenState {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

/// Query parameters parsed from the browser's callback request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Outcome of handling one accepted connection on the callback listener.
#[derive(Debug, PartialEq, Eq)]
enum CallbackConnectionOutcome {
    /// A request carrying the expected `state` and an authorization code.
    Authorized(String),
    /// A request carrying the expected `state` but an `error` (e.g. the user denied consent).
    Denied(String),
    /// Anything else: stray connection, missing params, state mismatch. Keep listening.
    Ignored,
}

/// Loopback HTTP listener that receives the OAuth authorization-code redirect.
///
/// Bound to an ephemeral port on 127.0.0.1 before the authorization URL is
/// generated so the exact redirect URI can be embedded in it.
pub struct OAuthCallbackListener {
    listener: TcpListener,
    redirect_url: String,
}

impl OAuthCallbackListener {
    /// Bind a listener on 127.0.0.1 with an OS-chosen port.
    pub async fn bind() -> Result<Self> {
        let listener = TcpListener::bind(CALLBACK_LISTENER_BIND_ADDRESS)
            .await
            .context("Failed to bind OAuth callback listener on 127.0.0.1")?;
        let local_addr = listener
            .local_addr()
            .context("Failed to read OAuth callback listener address")?;
        let redirect_url = format!("http://127.0.0.1:{}", local_addr.port());
        debug!("OAuth callback listener bound to {}", redirect_url);
        Ok(Self { listener, redirect_url })
    }

    /// The redirect URI (including the chosen port) that must be used in the
    /// authorization URL and the token exchange.
    pub fn redirect_url(&self) -> &str {
        &self.redirect_url
    }

    /// Wait until the browser delivers a callback whose `state` matches
    /// `expected_state`, then return the authorization code.
    ///
    /// Requests without `code`/`state`, or with a mismatching `state`, receive a
    /// 400 response and the listener keeps waiting. Connections that never send
    /// a request line are dropped after a short timeout. The whole wait is
    /// bounded by [`CALLBACK_OVERALL_TIMEOUT`].
    pub async fn wait_for_authorization_code(self, expected_state: &str) -> Result<String> {
        let deadline = tokio::time::Instant::now() + CALLBACK_OVERALL_TIMEOUT;

        loop {
            let (stream, peer_addr) = tokio::time::timeout_at(deadline, self.listener.accept())
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "Timed out after {} seconds waiting for the OAuth callback",
                        CALLBACK_OVERALL_TIMEOUT.as_secs()
                    )
                })?
                .context("Failed to accept OAuth callback connection")?;

            debug!("OAuth callback listener accepted connection from {}", peer_addr);

            let outcome = tokio::time::timeout(
                CALLBACK_CONNECTION_TIMEOUT,
                Self::handle_connection(stream, expected_state),
            )
            .await;

            match outcome {
                Ok(Ok(CallbackConnectionOutcome::Authorized(code))) => return Ok(code),
                Ok(Ok(CallbackConnectionOutcome::Denied(error))) => {
                    anyhow::bail!("Authorization was not granted: {}", error);
                }
                Ok(Ok(CallbackConnectionOutcome::Ignored)) => {
                    debug!("OAuth callback listener ignored a request without a valid callback; still waiting");
                }
                Ok(Err(io_error)) => {
                    debug!("OAuth callback connection failed: {}; still waiting", io_error);
                }
                Err(_elapsed) => {
                    debug!("OAuth callback connection sent no request within {:?}; still waiting", CALLBACK_CONNECTION_TIMEOUT);
                }
            }
        }
    }

    async fn handle_connection(mut stream: TcpStream, expected_state: &str) -> std::io::Result<CallbackConnectionOutcome> {
        let mut request_line = String::new();
        {
            let mut reader = BufReader::new(&mut stream);
            reader.read_line(&mut request_line).await?;
        }

        let outcome = match OAuthService::parse_callback_query(&request_line) {
            Ok(query) => Self::classify_callback_query(query, expected_state),
            Err(parse_error) => {
                debug!("OAuth callback request line could not be parsed: {}", parse_error);
                CallbackConnectionOutcome::Ignored
            }
        };

        let response = match outcome {
            CallbackConnectionOutcome::Authorized(_) | CallbackConnectionOutcome::Denied(_) => CALLBACK_SUCCESS_RESPONSE,
            CallbackConnectionOutcome::Ignored => CALLBACK_BAD_REQUEST_RESPONSE,
        };
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;
        let _ = stream.shutdown().await;

        Ok(outcome)
    }

    fn classify_callback_query(query: CallbackQuery, expected_state: &str) -> CallbackConnectionOutcome {
        if query.state.as_deref() != Some(expected_state) {
            return CallbackConnectionOutcome::Ignored;
        }
        match (query.code, query.error) {
            (Some(code), _) => CallbackConnectionOutcome::Authorized(code),
            (None, Some(error)) => CallbackConnectionOutcome::Denied(error),
            (None, None) => CallbackConnectionOutcome::Ignored,
        }
    }
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
        err.chain().any(|cause| cause.to_string().contains(REAUTH_REQUIRED_MARKER))
    }

    /// Shared HTTP client for token-endpoint calls. Redirects are disabled as
    /// recommended by the `oauth2` crate to avoid SSRF via redirect.
    fn token_endpoint_http_client() -> &'static reqwest::Client {
        static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
        CLIENT.get_or_init(|| {
            reqwest::ClientBuilder::new()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client with default TLS configuration should build")
        })
    }

    /// Load Google OAuth credentials from ~/.curier/google_credentials.json
    pub fn load_google_credentials() -> Result<GoogleCredentials> {
        let home_dir = dirs::home_dir()
            .context("Could not determine home directory")?;
        let creds_path = home_dir.join(".curier").join("google_credentials.json");
        Self::load_google_credentials_from_path(&creds_path)
    }

    /// Load and validate Google OAuth credentials from an explicit file path.
    pub fn load_google_credentials_from_path(creds_path: &Path) -> Result<GoogleCredentials> {
        Self::load_client_credentials_from_path(creds_path, ClientSecretRequirement::Required)
    }

    /// Load and validate an OAuth client credentials JSON file
    /// (`{"client_id": "...", "client_secret": "..."}`).
    ///
    /// On unix, warns if the file is readable by group or others.
    pub fn load_client_credentials_from_path(
        creds_path: &Path,
        secret_requirement: ClientSecretRequirement,
    ) -> Result<GoogleCredentials> {
        let contents = std::fs::read_to_string(creds_path)
            .context(format!(
                "Could not read credentials file at {}. Please create this file with your OAuth client_id and client_secret.",
                creds_path.display()
            ))?;

        Self::warn_if_credentials_file_is_shared(creds_path);

        let credentials: GoogleCredentials = serde_json::from_str(&contents)
            .context("Invalid JSON in credentials file. Expected format: {\"client_id\": \"...\", \"client_secret\": \"...\"}")?;

        if credentials.client_id.is_empty() {
            anyhow::bail!("Credentials file must contain a non-empty client_id");
        }
        if secret_requirement == ClientSecretRequirement::Required && credentials.client_secret.is_empty() {
            anyhow::bail!("Credentials file must contain non-empty client_id and client_secret");
        }

        Ok(credentials)
    }

    /// Whether a unix file mode grants any group or world permission bits.
    pub fn is_mode_group_or_world_accessible(mode: u32) -> bool {
        mode & GROUP_AND_WORLD_PERMISSION_BITS != 0
    }

    #[cfg(unix)]
    fn warn_if_credentials_file_is_shared(creds_path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(creds_path) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode();
                if Self::is_mode_group_or_world_accessible(mode) {
                    warn!(
                        "Credentials file {} is readable by other users (mode {:o}); run `chmod 600 {}`",
                        creds_path.display(),
                        mode & 0o777,
                        creds_path.display()
                    );
                }
            }
            Err(e) => debug!("Could not stat credentials file {}: {}", creds_path.display(), e),
        }
    }

    #[cfg(not(unix))]
    fn warn_if_credentials_file_is_shared(_creds_path: &Path) {}

    /// Fetch user email from Google userinfo API
    pub async fn fetch_user_email(access_token: &str) -> Result<String> {
        let client = reqwest::Client::new();
        let response = client
            .get(GOOGLE_USERINFO_URL)
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

    /// Build an `oauth2` client from the config. The client secret is only
    /// attached when non-empty, so public clients (e.g. Microsoft "Mobile and
    /// desktop" registrations) work without one.
    fn build_oauth_client(&self) -> Result<ConfiguredOAuthClient> {
        let mut client = BasicClient::new(ClientId::new(self.config.client_id.clone()))
            .set_auth_uri(AuthUrl::new(self.config.auth_url.clone()).context("Invalid auth URL")?)
            .set_token_uri(TokenUrl::new(self.config.token_url.clone()).context("Invalid token URL")?);
        if !self.config.client_secret.is_empty() {
            client = client.set_client_secret(ClientSecret::new(self.config.client_secret.clone()));
        }
        Ok(client)
    }

    /// Generate PKCE challenge and verifier
    fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
        PkceCodeChallenge::new_random_sha256()
    }

    /// Generate the authorization URL for the given loopback redirect URI.
    /// Returns the URL plus the PKCE verifier and CSRF token needed to finish the flow.
    pub fn generate_auth_url(&self, redirect_url: &str) -> Result<(String, PkceCodeVerifier, CsrfToken)> {
        let client = self
            .build_oauth_client()?
            .set_redirect_uri(RedirectUrl::new(redirect_url.to_string()).context("Invalid redirect URL")?);

        let (pkce_challenge, pkce_verifier) = Self::generate_pkce();

        let mut auth_url_builder = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent");

        for scope in &self.config.scopes {
            auth_url_builder = auth_url_builder.add_scope(Scope::new(scope.clone()));
        }

        let (auth_url, csrf_token) = auth_url_builder.url();

        Ok((auth_url.to_string(), pkce_verifier, csrf_token))
    }

    /// Complete the OAuth flow: wait for the loopback callback, verify the CSRF
    /// state, exchange the code for tokens and persist them under `email`.
    pub async fn complete_oauth_flow(
        &self,
        listener: OAuthCallbackListener,
        pkce_verifier: PkceCodeVerifier,
        csrf_token: CsrfToken,
        email: String,
        app_data_dir: &PathBuf,
    ) -> Result<AuthTokens> {
        let redirect_url = listener.redirect_url().to_string();
        let code = listener.wait_for_authorization_code(csrf_token.secret()).await?;

        let client = self
            .build_oauth_client()?
            .set_redirect_uri(RedirectUrl::new(redirect_url).context("Invalid redirect URL")?);

        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(Self::token_endpoint_http_client())
            .await
            .map_err(|e| anyhow::anyhow!("Token exchange failed ({})", Self::describe_token_error(&e)))?;

        let tokens = Self::tokens_from_response(&token_response, None);

        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Map an `oauth2` token-endpoint error to a short, non-sensitive code.
    /// Never formats the error with `{:?}`: the `Parse` variant carries the raw
    /// response body, which could contain tokens.
    pub fn describe_token_error<RE>(error: &RequestTokenError<RE, BasicErrorResponse>) -> String
    where
        RE: std::error::Error + 'static,
    {
        match error {
            RequestTokenError::ServerResponse(response) => format!("server_error:{}", response.error()),
            RequestTokenError::Request(_) => "request_failed".to_string(),
            RequestTokenError::Parse(_, _) => "parse_error".to_string(),
            RequestTokenError::Other(_) => "other".to_string(),
        }
    }

    /// Whether the token endpoint rejected the grant (revoked/expired refresh token).
    fn is_invalid_grant_error<RE>(error: &RequestTokenError<RE, BasicErrorResponse>) -> bool
    where
        RE: std::error::Error + 'static,
    {
        matches!(
            error,
            RequestTokenError::ServerResponse(response) if response.error().as_ref() == OAUTH_ERROR_CODE_INVALID_GRANT
        )
    }

    fn tokens_from_response(
        token_response: &oauth2::basic::BasicTokenResponse,
        fallback_refresh_token: Option<String>,
    ) -> AuthTokens {
        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or(fallback_refresh_token),
            expires_at,
        }
    }

    /// Extract the `code` and `state` query parameters from the HTTP request line
    /// (e.g. `GET /?code=abc&state=xyz HTTP/1.1`) that the browser sends to the
    /// local OAuth callback listener. Production code uses `parse_callback_query`
    /// so it can also see `error`; this strict variant is kept for tests.
    #[cfg(test)]
    pub fn parse_callback_request_line(request_line: &str) -> Result<(String, String)> {
        let query = Self::parse_callback_query(request_line)?;
        let code = query.code.context("No code in callback")?;
        let state = query.state.context("No state in callback")?;
        Ok((code, state))
    }

    /// Parse the callback query parameters from an HTTP request line without
    /// requiring any particular parameter to be present.
    pub fn parse_callback_query(request_line: &str) -> Result<CallbackQuery> {
        let request_target = request_line
            .split_whitespace()
            .nth(1)
            .context("Invalid HTTP request")?;

        let url = url::Url::parse(&format!("http://127.0.0.1{}", request_target))?;

        let find_param = |name: &str| -> Option<String> {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.to_string())
        };

        Ok(CallbackQuery {
            code: find_param(CALLBACK_QUERY_PARAM_CODE),
            state: find_param(CALLBACK_QUERY_PARAM_STATE),
            error: find_param(CALLBACK_QUERY_PARAM_ERROR),
        })
    }

    /// Store tokens and OAuth credentials in the credential store
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

        let client = self.build_oauth_client()?;

        debug!("refresh_token_with_value: Sending refresh request to token endpoint");
        let token_response = match client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(Self::token_endpoint_http_client())
            .await {
                Ok(response) => {
                    debug!("refresh_token_with_value: Token refresh successful");
                    response
                }
                Err(e) => {
                    let error_code = Self::describe_token_error(&e);
                    warn!("refresh_token_with_value: Token refresh failed: {}", error_code);
                    if Self::is_invalid_grant_error(&e) {
                        return Err(anyhow::anyhow!(
                            "{}: Token has been revoked or expired. Please re-authenticate.",
                            REAUTH_REQUIRED_MARKER
                        ));
                    }
                    return Err(anyhow::anyhow!("Failed to refresh token ({})", error_code));
                }
            };

        let tokens = Self::tokens_from_response(&token_response, Some(refresh_token.to_string()));

        // Store updated tokens
        let email = self.config.email.clone().unwrap_or_default();
        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Best-effort server-side revocation of an account's refresh token.
    ///
    /// Google exposes a revocation endpoint; Microsoft's v2.0 endpoint has no
    /// equivalent for public clients, so Outlook accounts are skipped. Failures
    /// are returned so the caller can log them, but local logout should proceed
    /// regardless.
    pub async fn revoke_refresh_token(config: &OAuthConfig) -> Result<()> {
        let Some(refresh_token) = config.refresh_token.as_deref() else {
            debug!("revoke_refresh_token: No refresh token to revoke");
            return Ok(());
        };

        match config.account_type {
            AccountType::Gmail => {
                let response = reqwest::Client::new()
                    .post(GOOGLE_TOKEN_REVOCATION_URL)
                    .form(&[("token", refresh_token)])
                    .send()
                    .await
                    .context("Failed to call Google token revocation endpoint")?;
                if !response.status().is_success() {
                    anyhow::bail!("Google token revocation returned HTTP {}", response.status());
                }
                debug!("revoke_refresh_token: Google refresh token revoked");
                Ok(())
            }
            AccountType::Outlook | AccountType::Imap => {
                debug!("revoke_refresh_token: No revocation endpoint for {:?}; skipping", config.account_type);
                Ok(())
            }
        }
    }

    // ============ Multi-Account Methods ============

    /// Store account credentials in the credential store
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

    /// Load all accounts from the credential store
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

    /// Remove account from the credential store
    pub fn remove_account_credentials(email: &str, app_data_dir: &PathBuf) -> Result<()> {
        debug!("CREDENTIAL_STORE: Removing credentials for: {}", email);

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.remove_account(email)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::auth::OAuthConfig;
    use oauth2::basic::BasicErrorResponseType;

    fn config_with_tokens(access: Option<&str>, refresh: Option<&str>, expires_at: Option<i64>) -> OAuthConfig {
        let mut config = OAuthConfig::google_defaults();
        config.client_id = "cid".to_string();
        config.client_secret = "secret".to_string();
        config.access_token = access.map(String::from);
        config.refresh_token = refresh.map(String::from);
        config.expires_at = expires_at;
        config
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    // ----- reauth error detection -----

    #[test]
    fn oauth_is_reauth_required_detects_marker_in_root_error() {
        let err = anyhow::anyhow!("REAUTH_REQUIRED: Token has been revoked");
        assert!(OAuthService::is_reauth_required_error(&err));
    }

    #[test]
    fn oauth_is_reauth_required_walks_context_chain() {
        let err = anyhow::anyhow!("REAUTH_REQUIRED: revoked")
            .context("Failed to get access token")
            .context("Gmail API: fetch failed");
        assert!(OAuthService::is_reauth_required_error(&err));
    }

    #[test]
    fn oauth_is_reauth_required_is_false_for_unrelated_errors() {
        let err = anyhow::anyhow!("network unreachable").context("Failed to get access token");
        assert!(!OAuthService::is_reauth_required_error(&err));
    }

    // ----- token error mapping (L-4) -----

    #[derive(Debug)]
    struct FakeTransportError;
    impl std::fmt::Display for FakeTransportError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "secret-bearing transport detail")
        }
    }
    impl std::error::Error for FakeTransportError {}

    type FakeTokenError = RequestTokenError<FakeTransportError, BasicErrorResponse>;

    #[test]
    fn oauth_describe_token_error_maps_server_response_to_error_code_only() {
        let err: FakeTokenError = RequestTokenError::ServerResponse(BasicErrorResponse::new(
            BasicErrorResponseType::InvalidGrant,
            Some("very detailed description".to_string()),
            None,
        ));
        assert_eq!(OAuthService::describe_token_error(&err), "server_error:invalid_grant");
        assert!(OAuthService::is_invalid_grant_error(&err));
    }

    #[test]
    fn oauth_describe_token_error_hides_transport_and_other_details() {
        let request_err: FakeTokenError = RequestTokenError::Request(FakeTransportError);
        assert_eq!(OAuthService::describe_token_error(&request_err), "request_failed");
        assert!(!OAuthService::is_invalid_grant_error(&request_err));

        let other_err: FakeTokenError = RequestTokenError::Other("raw body with token".to_string());
        assert_eq!(OAuthService::describe_token_error(&other_err), "other");
    }

    #[test]
    fn oauth_describe_token_error_distinguishes_other_server_errors_from_invalid_grant() {
        let err: FakeTokenError = RequestTokenError::ServerResponse(BasicErrorResponse::new(
            BasicErrorResponseType::InvalidClient,
            None,
            None,
        ));
        assert_eq!(OAuthService::describe_token_error(&err), "server_error:invalid_client");
        assert!(!OAuthService::is_invalid_grant_error(&err));
    }

    // ----- token expiry -----

    #[tokio::test]
    async fn oauth_is_authenticated_true_when_expiry_in_future() {
        let service = OAuthService::new(config_with_tokens(Some("a"), None, Some(now() + 3600)));
        assert!(service.is_authenticated().await.unwrap());
    }

    #[tokio::test]
    async fn oauth_is_authenticated_false_when_expired_or_missing() {
        let expired = OAuthService::new(config_with_tokens(Some("a"), None, Some(now() - 1)));
        assert!(!expired.is_authenticated().await.unwrap());

        let missing = OAuthService::new(config_with_tokens(Some("a"), None, None));
        assert!(!missing.is_authenticated().await.unwrap());
    }

    #[tokio::test]
    async fn oauth_is_authenticated_reflects_shared_token_state_updates() {
        let service = OAuthService::new(config_with_tokens(Some("a"), None, Some(now() - 1)));
        assert!(!service.is_authenticated().await.unwrap());
        {
            let handle = service.token_state_handle();
            let mut state = handle.write().await;
            state.expires_at = Some(now() + 600);
        }
        assert!(service.is_authenticated().await.unwrap());
    }

    #[tokio::test]
    async fn oauth_get_valid_access_token_returns_cached_token_without_refresh() {
        let service = OAuthService::new(config_with_tokens(Some("cached"), Some("r"), Some(now() + 3600)));
        let token = service.get_valid_access_token(&PathBuf::from("/nonexistent")).await.unwrap();
        assert_eq!(token, "cached");
    }

    #[tokio::test]
    async fn oauth_get_valid_access_token_errors_when_expired_and_no_refresh_token() {
        let service = OAuthService::new(config_with_tokens(Some("stale"), None, Some(now() - 10)));
        let err = service.get_valid_access_token(&PathBuf::from("/nonexistent")).await.unwrap_err();
        assert!(err.to_string().contains("No refresh token"), "{}", err);
    }

    #[tokio::test]
    async fn oauth_get_valid_access_token_treats_missing_expiry_as_expired() {
        let service = OAuthService::new(config_with_tokens(Some("stale"), None, None));
        assert!(service.get_valid_access_token(&PathBuf::from("/nonexistent")).await.is_err());
    }

    #[test]
    fn oauth_new_seeds_token_state_from_config() {
        let service = OAuthService::new(config_with_tokens(Some("a"), Some("r"), Some(42)));
        let state = service.token_state_handle();
        let state = state.blocking_read();
        assert_eq!(state.access_token.as_deref(), Some("a"));
        assert_eq!(state.refresh_token.as_deref(), Some("r"));
        assert_eq!(state.expires_at, Some(42));
    }

    // ----- auth URL generation -----

    #[test]
    fn oauth_generate_auth_url_embeds_dynamic_redirect_uri_scopes_and_pkce() {
        let service = OAuthService::new(config_with_tokens(None, None, None));
        let (url, _verifier, csrf) = service.generate_auth_url("http://127.0.0.1:54321").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let params: std::collections::HashMap<String, String> =
            parsed.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(params["redirect_uri"], "http://127.0.0.1:54321");
        assert_eq!(params["state"], *csrf.secret());
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["access_type"], "offline");
        assert_eq!(params["prompt"], "consent");
        assert!(params["scope"].contains("gmail.send"));
        assert!(params["scope"].contains("gmail.readonly"));
        assert!(!params["scope"].contains("mail.google.com"));
    }

    #[test]
    fn oauth_build_client_without_secret_succeeds_for_public_clients() {
        let mut config = OAuthConfig::microsoft_defaults();
        config.client_id = "public-client".to_string();
        config.client_secret = String::new();
        let service = OAuthService::new(config);
        assert!(service.generate_auth_url("http://127.0.0.1:1").is_ok());
    }

    // ----- credentials file -----

    #[test]
    fn oauth_load_google_credentials_from_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google_credentials.json");
        std::fs::write(&path, r#"{"client_id":"id-1","client_secret":"sec-1"}"#).unwrap();
        let creds = OAuthService::load_google_credentials_from_path(&path).unwrap();
        assert_eq!(creds.client_id, "id-1");
        assert_eq!(creds.client_secret, "sec-1");
    }

    #[test]
    fn oauth_load_google_credentials_missing_file_errors_with_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.json");
        let err = OAuthService::load_google_credentials_from_path(&path).unwrap_err();
        assert!(err.to_string().contains("missing.json"), "{}", err);
    }

    #[test]
    fn oauth_load_google_credentials_rejects_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google_credentials.json");
        std::fs::write(&path, "{ nope").unwrap();
        let err = OAuthService::load_google_credentials_from_path(&path).unwrap_err();
        assert!(err.to_string().contains("Invalid JSON"), "{}", err);
    }

    #[test]
    fn oauth_load_google_credentials_rejects_empty_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google_credentials.json");
        std::fs::write(&path, r#"{"client_id":"","client_secret":"x"}"#).unwrap();
        let err = OAuthService::load_google_credentials_from_path(&path).unwrap_err();
        assert!(err.to_string().contains("non-empty"), "{}", err);

        std::fs::write(&path, r#"{"client_id":"x","client_secret":""}"#).unwrap();
        assert!(OAuthService::load_google_credentials_from_path(&path).is_err());
    }

    #[test]
    fn oauth_load_client_credentials_allows_empty_secret_when_optional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("microsoft_credentials.json");
        std::fs::write(&path, r#"{"client_id":"x","client_secret":""}"#).unwrap();
        let creds = OAuthService::load_client_credentials_from_path(&path, ClientSecretRequirement::Optional).unwrap();
        assert_eq!(creds.client_id, "x");
        assert!(creds.client_secret.is_empty());

        std::fs::write(&path, r#"{"client_id":"","client_secret":""}"#).unwrap();
        assert!(OAuthService::load_client_credentials_from_path(&path, ClientSecretRequirement::Optional).is_err());
    }

    #[test]
    fn oauth_group_or_world_accessible_mode_detection() {
        assert!(!OAuthService::is_mode_group_or_world_accessible(0o600));
        assert!(!OAuthService::is_mode_group_or_world_accessible(0o100600));
        assert!(OAuthService::is_mode_group_or_world_accessible(0o644));
        assert!(OAuthService::is_mode_group_or_world_accessible(0o660));
        assert!(OAuthService::is_mode_group_or_world_accessible(0o604));
    }

    #[cfg(unix)]
    #[test]
    fn oauth_load_credentials_still_succeeds_when_file_is_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("google_credentials.json");
        std::fs::write(&path, r#"{"client_id":"id-1","client_secret":"sec-1"}"#).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        // Only warns; loading must not fail.
        assert!(OAuthService::load_google_credentials_from_path(&path).is_ok());
    }

    // ----- callback parsing -----

    #[test]
    fn oauth_parse_callback_extracts_code_and_state() {
        let (code, state) = OAuthService::parse_callback_request_line("GET /?code=4%2Fabc&state=xyz&scope=email HTTP/1.1\r\n").unwrap();
        assert_eq!(code, "4/abc");
        assert_eq!(state, "xyz");
    }

    #[test]
    fn oauth_parse_callback_errors_without_code_or_state() {
        assert!(OAuthService::parse_callback_request_line("GET /?state=xyz HTTP/1.1").is_err());
        assert!(OAuthService::parse_callback_request_line("GET /?code=abc HTTP/1.1").is_err());
        assert!(OAuthService::parse_callback_request_line("GET /?error=access_denied HTTP/1.1").is_err());
    }

    #[test]
    fn oauth_parse_callback_errors_on_malformed_request_line() {
        assert!(OAuthService::parse_callback_request_line("").is_err());
        assert!(OAuthService::parse_callback_request_line("GET").is_err());
    }

    #[test]
    fn oauth_parse_callback_query_captures_error_param() {
        let query = OAuthService::parse_callback_query("GET /?error=access_denied&state=s HTTP/1.1").unwrap();
        assert_eq!(query, CallbackQuery { code: None, state: Some("s".into()), error: Some("access_denied".into()) });
    }

    #[test]
    fn oauth_classify_callback_requires_matching_state() {
        let authorized = CallbackQuery { code: Some("c".into()), state: Some("good".into()), error: None };
        assert_eq!(
            OAuthCallbackListener::classify_callback_query(authorized, "good"),
            CallbackConnectionOutcome::Authorized("c".into())
        );

        let wrong_state = CallbackQuery { code: Some("c".into()), state: Some("bad".into()), error: None };
        assert_eq!(OAuthCallbackListener::classify_callback_query(wrong_state, "good"), CallbackConnectionOutcome::Ignored);

        let no_state = CallbackQuery { code: Some("c".into()), state: None, error: None };
        assert_eq!(OAuthCallbackListener::classify_callback_query(no_state, "good"), CallbackConnectionOutcome::Ignored);

        let denied = CallbackQuery { code: None, state: Some("good".into()), error: Some("access_denied".into()) };
        assert_eq!(
            OAuthCallbackListener::classify_callback_query(denied, "good"),
            CallbackConnectionOutcome::Denied("access_denied".into())
        );

        let empty = CallbackQuery { code: None, state: Some("good".into()), error: None };
        assert_eq!(OAuthCallbackListener::classify_callback_query(empty, "good"), CallbackConnectionOutcome::Ignored);
    }

    // ----- loopback listener (M-5) -----

    async fn send_raw_request(redirect_url: &str, request: &str) -> String {
        let addr = url::Url::parse(redirect_url).unwrap();
        let mut stream = TcpStream::connect((addr.host_str().unwrap(), addr.port().unwrap())).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        let mut reader = BufReader::new(&mut stream);
        reader.read_line(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn oauth_listener_binds_ephemeral_loopback_port() {
        let listener = OAuthCallbackListener::bind().await.unwrap();
        let parsed = url::Url::parse(listener.redirect_url()).unwrap();
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert!(parsed.port().unwrap() > 0);

        let second = OAuthCallbackListener::bind().await.unwrap();
        assert_ne!(listener.redirect_url(), second.redirect_url(), "each flow gets its own port");
    }

    #[tokio::test]
    async fn oauth_listener_ignores_bad_requests_and_returns_code_on_matching_state() {
        let listener = OAuthCallbackListener::bind().await.unwrap();
        let redirect_url = listener.redirect_url().to_string();

        let waiter = tokio::spawn(async move { listener.wait_for_authorization_code("expected-state").await });

        // 1. Connection that never sends anything (browser preconnect).
        let addr = url::Url::parse(&redirect_url).unwrap();
        let idle = TcpStream::connect((addr.host_str().unwrap(), addr.port().unwrap())).await.unwrap();

        // 2. Request without code/state -> 400, keep listening.
        let status = send_raw_request(&redirect_url, "GET /?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await;
        assert!(status.starts_with("HTTP/1.1 400"), "{}", status);

        // 3. Forged state -> 400, keep listening.
        let status = send_raw_request(&redirect_url, "GET /?code=evil&state=forged HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await;
        assert!(status.starts_with("HTTP/1.1 400"), "{}", status);

        drop(idle);

        // 4. Real callback -> 200 and the code is returned.
        let status = send_raw_request(&redirect_url, "GET /?code=real-code&state=expected-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").await;
        assert!(status.starts_with("HTTP/1.1 200"), "{}", status);

        let code = tokio::time::timeout(Duration::from_secs(10), waiter).await.unwrap().unwrap().unwrap();
        assert_eq!(code, "real-code");
    }

    #[tokio::test]
    async fn oauth_listener_reports_user_denial_with_matching_state() {
        let listener = OAuthCallbackListener::bind().await.unwrap();
        let redirect_url = listener.redirect_url().to_string();
        let waiter = tokio::spawn(async move { listener.wait_for_authorization_code("s").await });

        send_raw_request(&redirect_url, "GET /?error=access_denied&state=s HTTP/1.1\r\n\r\n").await;

        let err = tokio::time::timeout(Duration::from_secs(10), waiter).await.unwrap().unwrap().unwrap_err();
        assert!(err.to_string().contains("access_denied"), "{}", err);
    }

    // ----- config defaults -----

    #[test]
    fn oauth_config_defaults_for_account_type() {
        use crate::models::auth::AccountType;
        assert_eq!(OAuthConfig::defaults_for(&AccountType::Gmail).account_type, AccountType::Gmail);
        assert_eq!(OAuthConfig::defaults_for(&AccountType::Outlook).account_type, AccountType::Outlook);
        assert_eq!(OAuthConfig::defaults_for(&AccountType::Imap).account_type, AccountType::Gmail);
        assert_eq!(OAuthConfig::default().account_type, AccountType::Gmail);
    }

    #[test]
    fn oauth_config_microsoft_defaults_use_v2_0_endpoints() {
        let config = OAuthConfig::microsoft_defaults();
        assert_eq!(config.auth_url, "https://login.microsoftonline.com/common/oauth2/v2.0/authorize");
        assert_eq!(config.token_url, "https://login.microsoftonline.com/common/oauth2/v2.0/token");
    }

    #[test]
    fn oauth_config_google_scopes_are_read_and_send_only() {
        let scopes = OAuthConfig::google_defaults().scopes;
        assert_eq!(
            scopes,
            vec![
                "https://www.googleapis.com/auth/gmail.readonly".to_string(),
                "https://www.googleapis.com/auth/gmail.send".to_string(),
                "email".to_string(),
            ]
        );
    }
}
