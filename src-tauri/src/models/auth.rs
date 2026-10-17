use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleCredentials {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountType {
    Gmail,
    Outlook,
    Imap,
}

impl Default for AccountType {
    fn default() -> Self {
        AccountType::Gmail
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub account_type: AccountType,
    pub email: String,
    pub display_name: String,
    /// "ok" or "reauth_required" — communicates token validity to the frontend
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64, // Unix timestamp
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub account_type: AccountType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub is_authenticated: bool,
    pub email: Option<String>,
}

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Read-only mailbox access (list/get messages and attachments).
const GOOGLE_SCOPE_GMAIL_READONLY: &str = "https://www.googleapis.com/auth/gmail.readonly";
/// Send-only access; together with readonly this covers every Gmail call the app makes.
const GOOGLE_SCOPE_GMAIL_SEND: &str = "https://www.googleapis.com/auth/gmail.send";
/// Needed to read the signed-in address from the userinfo endpoint.
const GOOGLE_SCOPE_EMAIL: &str = "email";

/// Microsoft identity platform v2.0 endpoints (the `/v2/` paths do not exist).
const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";

/// OAuth client configuration plus cached tokens for one account.
///
/// The redirect URI is not part of the config: it is chosen per login flow by
/// `OAuthCallbackListener`, which binds an ephemeral loopback port.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    pub email: Option<String>,
    pub account_type: AccountType,
    // Store tokens to avoid repeated Keychain access
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl OAuthConfig {
    /// Create a config pre-populated with Google OAuth endpoints and scopes.
    pub fn google_defaults() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            auth_url: GOOGLE_AUTH_URL.to_string(),
            token_url: GOOGLE_TOKEN_URL.to_string(),
            scopes: vec![
                GOOGLE_SCOPE_GMAIL_READONLY.to_string(),
                GOOGLE_SCOPE_GMAIL_SEND.to_string(),
                GOOGLE_SCOPE_EMAIL.to_string(),
            ],
            email: None,
            account_type: AccountType::Gmail,
            access_token: None,
            refresh_token: None,
            expires_at: None,
        }
    }

    /// Create a config pre-populated with Microsoft OAuth endpoints and Graph API scopes.
    pub fn microsoft_defaults() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            auth_url: MICROSOFT_AUTH_URL.to_string(),
            token_url: MICROSOFT_TOKEN_URL.to_string(),
            scopes: vec![
                "https://graph.microsoft.com/Mail.Read".to_string(),
                "https://graph.microsoft.com/Mail.Send".to_string(),
                "https://graph.microsoft.com/User.Read".to_string(),
                "offline_access".to_string(),
                "openid".to_string(),
                "email".to_string(),
            ],
            email: None,
            account_type: AccountType::Outlook,
            access_token: None,
            refresh_token: None,
            expires_at: None,
        }
    }

    /// Return the appropriate defaults for the given account type.
    pub fn defaults_for(account_type: &AccountType) -> Self {
        match account_type {
            AccountType::Outlook => Self::microsoft_defaults(),
            _ => Self::google_defaults(),
        }
    }
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self::google_defaults()
    }
}
