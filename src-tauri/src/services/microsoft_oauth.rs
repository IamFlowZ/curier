use crate::models::auth::{AuthTokens, StoredCredentials, AccountType};
use crate::services::credential_store::CredentialStore;
use anyhow::{Context, Result};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse, TokenUrl,
};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;

// Microsoft OAuth endpoints
const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

pub struct MicrosoftOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
    pub email: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl Default for MicrosoftOAuthConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            redirect_url: "http://127.0.0.1:8081".to_string(), // Different port from Gmail
            scopes: vec![
                "https://graph.microsoft.com/Mail.Read".to_string(),
                "https://graph.microsoft.com/User.Read".to_string(),
                "offline_access".to_string(),
                "openid".to_string(),
                "email".to_string(),
            ],
            email: None,
            access_token: None,
            refresh_token: None,
            expires_at: None,
        }
    }
}

pub struct MicrosoftOAuthService {
    config: MicrosoftOAuthConfig,
}

impl MicrosoftOAuthService {
    pub fn new(config: MicrosoftOAuthConfig) -> Self {
        Self { config }
    }

    /// Generate PKCE challenge and verifier
    fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
        PkceCodeChallenge::new_random_sha256()
    }

    /// Generate OAuth URL for Microsoft
    pub fn generate_auth_url(&self) -> Result<(String, PkceCodeVerifier, CsrfToken)> {
        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(MICROSOFT_AUTH_URL.to_string())?,
            Some(TokenUrl::new(MICROSOFT_TOKEN_URL.to_string())?),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);

        let (pkce_challenge, pkce_verifier) = Self::generate_pkce();

        let mut auth_url_builder = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        for scope in &self.config.scopes {
            auth_url_builder = auth_url_builder.add_scope(Scope::new(scope.clone()));
        }

        let (auth_url, csrf_token) = auth_url_builder.url();

        Ok((auth_url.to_string(), pkce_verifier, csrf_token))
    }

    /// Complete OAuth flow - waits for callback on port 8081
    pub async fn complete_oauth_flow(
        &self,
        pkce_verifier: PkceCodeVerifier,
        csrf_token: CsrfToken,
        email: String,
        app_data_dir: &PathBuf,
    ) -> Result<AuthTokens> {
        // Wait for callback on port 8081 (different from Gmail's 8080)
        let (code, state) = Self::wait_for_callback()?;

        if state != *csrf_token.secret() {
            anyhow::bail!("CSRF token mismatch");
        }

        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(MICROSOFT_AUTH_URL.to_string())?,
            Some(TokenUrl::new(MICROSOFT_TOKEN_URL.to_string())?),
        )
        .set_redirect_uri(RedirectUrl::new(self.config.redirect_url.clone())?);

        let token_response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(async_http_client)
            .await
            .map_err(|e| anyhow::anyhow!("Microsoft token exchange failed: {:?}", e))?;

        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let tokens = AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            expires_at,
        };

        self.store_tokens(&tokens, &email, app_data_dir)?;

        Ok(tokens)
    }

    /// Wait for OAuth callback on port 8081
    fn wait_for_callback() -> Result<(String, String)> {
        let listener = TcpListener::bind("127.0.0.1:8081")?;

        let (mut stream, _) = listener.accept()?;
        let mut reader = BufReader::new(&stream);

        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;

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

        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body style=\"font-family: -apple-system, sans-serif; text-align: center; padding: 50px;\"><h1 style=\"color: #10b981;\">✓ Microsoft Authentication successful!</h1><p>You can close this window and return to Curier.</p></body></html>";
        stream.write_all(response.as_bytes())?;
        stream.flush()?;

        Ok((code, state))
    }

    /// Store Microsoft tokens in encrypted file storage
    fn store_tokens(&self, tokens: &AuthTokens, email: &str, app_data_dir: &PathBuf) -> Result<()> {
        let credentials = StoredCredentials {
            client_id: self.config.client_id.clone(),
            client_secret: self.config.client_secret.clone(),
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            email: Some(email.to_string()),
            account_type: AccountType::Outlook,
        };

        let store = CredentialStore::new(app_data_dir.clone())?;
        store.store_account(email, &credentials)?;

        Ok(())
    }

    /// Refresh access token
    pub async fn refresh_token(&self, app_data_dir: &PathBuf) -> Result<AuthTokens> {
        let refresh_token = self.config.refresh_token
            .clone()
            .context("No refresh token available")?;

        let client = BasicClient::new(
            ClientId::new(self.config.client_id.clone()),
            Some(ClientSecret::new(self.config.client_secret.clone())),
            AuthUrl::new(MICROSOFT_AUTH_URL.to_string())?,
            Some(TokenUrl::new(MICROSOFT_TOKEN_URL.to_string())?),
        );

        let token_response = client
            .exchange_refresh_token(&oauth2::RefreshToken::new(refresh_token.clone()))
            .request_async(async_http_client)
            .await
            .context("Failed to refresh Microsoft token")?;

        let expires_in = token_response.expires_in().unwrap_or_default().as_secs() as i64;
        let expires_at = chrono::Utc::now().timestamp() + expires_in;

        let tokens = AuthTokens {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().clone())
                .or(Some(refresh_token)),
            expires_at,
        };

        if let Some(email) = &self.config.email {
            self.store_tokens(&tokens, email, app_data_dir)?;
        }

        Ok(tokens)
    }

    /// Get valid access token
    pub async fn get_valid_access_token(&self, app_data_dir: &PathBuf) -> Result<String> {
        let now = chrono::Utc::now().timestamp();
        let is_expired = self.config.expires_at.map(|exp| exp <= now).unwrap_or(true);

        if is_expired {
            let tokens = self.refresh_token(app_data_dir).await?;
            return Ok(tokens.access_token);
        }

        self.config.access_token.clone()
            .ok_or_else(|| anyhow::anyhow!("No access token available"))
    }
}
