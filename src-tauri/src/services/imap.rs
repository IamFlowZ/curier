use crate::models::email::EmailSummary;
use crate::services::oauth::OAuthService;
use anyhow::{Context, Result};
use log::{debug, info};
use async_imap::{Authenticator, Session};
use async_native_tls::{TlsConnector, TlsStream};
use futures::stream::StreamExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::TokioAsyncReadCompatExt;

const GMAIL_IMAP_HOST: &str = "imap.gmail.com";
const GMAIL_IMAP_PORT: u16 = 993;

pub type ImapSession = Session<TlsStream<tokio_util::compat::Compat<TcpStream>>>;

// XOAUTH2 Authenticator implementation
struct XOAuth2 {
    auth_string: Vec<u8>,
    sent: bool,
}

impl Authenticator for XOAuth2 {
    type Response = Vec<u8>;

    fn process(&mut self, _data: &[u8]) -> Self::Response {
        if !self.sent {
            self.sent = true;
            self.auth_string.clone()
        } else {
            // Return empty vector on subsequent calls (server challenges)
            Vec::new()
        }
    }
}

pub struct ImapService {
    oauth_service: OAuthService,
    session: Arc<Mutex<Option<ImapSession>>>,
    app_data_dir: PathBuf,
}

impl ImapService {
    pub fn new(oauth_service: OAuthService, app_data_dir: PathBuf) -> Self {
        Self {
            oauth_service,
            session: Arc::new(Mutex::new(None)),
            app_data_dir,
        }
    }

    /// Generate XOAUTH2 authentication string
    fn generate_xoauth2_string(email: &str, access_token: &str) -> String {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            email, access_token
        )
    }

    /// Connect to Gmail IMAP server with OAuth2 authentication
    pub async fn connect(&self, email: String) -> Result<()> {
        info!("IMAP: Connecting to Gmail IMAP server");
        // Get valid access token
        let access_token = self
            .oauth_service
            .get_valid_access_token(&self.app_data_dir)
            .await
            .context("Failed to get access token")?;
        debug!("IMAP connect: Access token retrieved");

        debug!("IMAP connect: Connecting to TCP {}:{}", GMAIL_IMAP_HOST, GMAIL_IMAP_PORT);
        // Connect to IMAP server with TLS
        let tcp_stream = TcpStream::connect((GMAIL_IMAP_HOST, GMAIL_IMAP_PORT))
            .await
            .context("Failed to connect to IMAP server")?;
        debug!("IMAP connect: TCP connection established");

        // Convert Tokio stream to futures-compatible stream
        let tcp_stream = tcp_stream.compat();

        debug!("IMAP connect: Establishing TLS connection");
        let tls = TlsConnector::new();
        let tls_stream = tls
            .connect(GMAIL_IMAP_HOST, tcp_stream)
            .await
            .context("Failed to establish TLS connection")?;
        debug!("IMAP connect: TLS connection established");

        let client = async_imap::Client::new(tls_stream);
        debug!("IMAP connect: IMAP client created");

        // Authenticate with XOAUTH2
        debug!("IMAP connect: Authenticating with XOAUTH2");
        let xoauth2_string = Self::generate_xoauth2_string(&email, &access_token);
        let authenticator = XOAuth2 {
            auth_string: xoauth2_string.into_bytes(),
            sent: false,
        };

        use tokio::time::{timeout, Duration};

        let auth_future = client.authenticate("XOAUTH2", authenticator);
        let session = timeout(Duration::from_secs(30), auth_future)
            .await
            .map_err(|_| anyhow::anyhow!("XOAUTH2 authentication timed out after 30 seconds"))?
            .map_err(|e| anyhow::anyhow!("XOAUTH2 authentication failed: {:?}", e))?;
        debug!("IMAP connect: XOAUTH2 authentication successful");

        // Store session
        let mut session_guard = self.session.lock().await;
        *session_guard = Some(session);
        debug!("IMAP connect: Session stored, connection complete");

        Ok(())
    }

    /// Check if connected
    pub async fn is_connected(&self) -> bool {
        let session_guard = self.session.lock().await;
        session_guard.is_some()
    }

    /// Select a mailbox (e.g., "INBOX")
    pub async fn select_mailbox(&self, mailbox: &str) -> Result<()> {
        let mut session_guard = self.session.lock().await;
        let session = session_guard
            .as_mut()
            .context("Not connected to IMAP server")?;

        session
            .select(mailbox)
            .await
            .context(format!("Failed to select mailbox: {}", mailbox))?;

        Ok(())
    }

    /// Fetch recent emails (returns up to `count` most recent emails)
    pub async fn fetch_recent_emails(&self, count: u32) -> Result<Vec<EmailSummary>> {
        let mut session_guard = self.session.lock().await;
        let session = session_guard
            .as_mut()
            .context("Not connected to IMAP server")?;

        // Search for all messages
        let uids = session
            .uid_search("ALL")
            .await
            .context("Failed to search emails")?;

        // Convert to sorted vector (UID order = chronological order in IMAP)
        let mut uid_vec: Vec<u32> = uids.iter().copied().collect();
        uid_vec.sort_unstable();

        // Get the last `count` UIDs (most recent)
        let recent_uids: Vec<u32> = uid_vec.iter().rev().take(count as usize).copied().collect();

        if recent_uids.is_empty() {
            return Ok(Vec::new());
        }

        // Create UID sequence
        let uid_set = recent_uids
            .iter()
            .map(|uid: &u32| uid.to_string())
            .collect::<Vec<_>>()
            .join(",");

        // Fetch envelope data (headers only, efficient)
        let mut fetch_stream = session
            .uid_fetch(uid_set, "ENVELOPE")
            .await
            .context("Failed to fetch email envelopes")?;

        let mut emails: Vec<EmailSummary> = Vec::new();

        // Collect messages from stream
        while let Some(fetch_result) = fetch_stream.next().await {
            if let Ok(msg) = fetch_result {
                if let Some(envelope) = msg.envelope() {
                // Parse subject
                let subject = envelope
                    .subject
                    .as_ref()
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .unwrap_or("(No Subject)")
                    .to_string();

                // Parse from address
                let (from_address, from_name) = if let Some(from_list) = &envelope.from {
                    if let Some(from) = from_list.first() {
                        let address = from
                            .mailbox
                            .as_ref()
                            .and_then(|m| std::str::from_utf8(m).ok())
                            .unwrap_or("unknown")
                            .to_string()
                            + "@"
                            + from
                                .host
                                .as_ref()
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .unwrap_or("unknown");

                        let name = from
                            .name
                            .as_ref()
                            .and_then(|n| std::str::from_utf8(n).ok())
                            .map(|s| s.to_string());

                        (address, name)
                    } else {
                        ("unknown@unknown".to_string(), None)
                    }
                } else {
                    ("unknown@unknown".to_string(), None)
                };

                // Parse date (convert to Unix timestamp)
                let date = envelope
                    .date
                    .as_ref()
                    .and_then(|d| std::str::from_utf8(d).ok())
                    .and_then(|d| chrono::DateTime::parse_from_rfc2822(d).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp());

                let uid = msg.uid.unwrap_or(0);

                // Create email summary (no snippet for now, will add when fetching full body)
                emails.push(EmailSummary {
                    id: None,
                    uid,
                    message_id: None, // IMAP doesn't use Gmail message IDs
                    subject,
                    from_address,
                    from_name,
                    date,
                    snippet: None,
                    has_attachments: false, // Will detect when fetching full message
                    is_read: false,
                    account_email: None,
                });
                }
            }
        }

        Ok(emails)
    }

    /// Logout and close connection
    pub async fn logout(&self) -> Result<()> {
        let mut session_guard = self.session.lock().await;
        if let Some(mut session) = session_guard.take() {
            session
                .logout()
                .await
                .context("Failed to logout from IMAP server")?;
        }
        Ok(())
    }
}
