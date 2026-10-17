use crate::models::email::{AttachmentInfo, EmailSummary};
use crate::services::email_provider::{EmailFolder, EmailProvider};
use crate::services::oauth::{OAuthService, TokenState};
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";

// Standard Gmail label IDs
pub const LABEL_INBOX: &str = "INBOX";
pub const LABEL_SENT: &str = "SENT";
pub const LABEL_DRAFTS: &str = "DRAFT";
pub const LABEL_TRASH: &str = "TRASH";
pub const LABEL_SPAM: &str = "SPAM";
pub const LABEL_STARRED: &str = "STARRED";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailFolder {
    pub id: String,
    pub name: String,
    pub folder_type: String, // "system" or "user"
    pub message_count: Option<u32>,
    pub unread_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListLabelsResponse {
    labels: Option<Vec<LabelInfo>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LabelInfo {
    id: String,
    name: String,
    #[serde(rename = "type")]
    label_type: Option<String>,
    messages_total: Option<u32>,
    messages_unread: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListMessagesResponse {
    messages: Option<Vec<MessageId>>,
    next_page_token: Option<String>,
    result_size_estimate: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageId {
    id: String,
    #[allow(dead_code)]
    thread_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    id: String,
    #[allow(dead_code)]
    thread_id: String,
    #[allow(dead_code)]
    label_ids: Option<Vec<String>>,
    snippet: Option<String>,
    payload: MessagePayload,
    #[allow(dead_code)]
    internal_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessagePayload {
    headers: Vec<MessageHeader>,
    body: Option<MessageBody>,
    parts: Option<Vec<MessagePart>>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessagePart {
    mime_type: String,
    filename: Option<String>,
    body: Option<MessageBody>,
    parts: Option<Vec<MessagePart>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageHeader {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageBody {
    #[allow(dead_code)]
    size: u32,
    data: Option<String>,
    attachment_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentResponse {
    #[allow(dead_code)]
    size: u32,
    data: String,
}

pub struct GmailApiService {
    oauth_service: OAuthService,
    app_data_dir: PathBuf,
    http_client: reqwest::Client,
}

impl GmailApiService {
    pub fn new(oauth_service: OAuthService, app_data_dir: PathBuf) -> Self {
        Self {
            oauth_service,
            app_data_dir,
            http_client: reqwest::Client::new(),
        }
    }

    /// Get a handle to the OAuth token state for cache write-back.
    /// After API calls that may trigger a token refresh, callers can
    /// read this to sync the updated tokens back to the AppState cache.
    pub fn oauth_token_state_handle(&self) -> Arc<RwLock<TokenState>> {
        self.oauth_service.token_state_handle()
    }

    /// Get a valid access token (convenience wrapper)
    async fn access_token(&self) -> Result<String> {
        self.oauth_service
            .get_valid_access_token(&self.app_data_dir)
            .await
            .context("Failed to get access token")
    }

    /// List message IDs from Gmail with optional label, query, and page token.
    /// Returns (message_ids, next_page_token, result_size_estimate).
    async fn list_message_ids(
        &self,
        access_token: &str,
        label_id: Option<&str>,
        query: Option<&str>,
        max_results: u32,
        page_token: Option<&str>,
    ) -> Result<(Vec<String>, Option<String>, Option<u32>)> {
        let mut url = format!(
            "{}/users/me/messages?maxResults={}",
            GMAIL_API_BASE, max_results
        );

        if let Some(label) = label_id {
            url.push_str(&format!("&labelIds={}", label));
        }
        if let Some(q) = query {
            url.push_str(&format!("&q={}", urlencoding::encode(q)));
        }
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", token));
        }

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to list messages")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gmail API list messages failed: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response.text().await?;
        let list_data: ListMessagesResponse = serde_json::from_str(&response_text)
            .context(format!("Failed to parse list response: {}", response_text))?;

        let ids = list_data
            .messages
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.id)
            .collect();

        Ok((ids, list_data.next_page_token, list_data.result_size_estimate))
    }

    /// Fetch metadata for a single message and convert to EmailSummary.
    async fn fetch_message_summary(
        &self,
        access_token: &str,
        message_id: &str,
    ) -> Result<Option<EmailSummary>> {
        let url = format!(
            "{}/users/me/messages/{}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date",
            GMAIL_API_BASE, message_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .context("Failed to get message")?;

        if !response.status().is_success() {
            warn!("Gmail API: Failed to fetch message {}", message_id);
            return Ok(None);
        }

        let message: Message = response
            .json()
            .await
            .context("Failed to parse message")?;

        Ok(Some(message_to_email_summary(&message)))
    }

    /// Fetch message summaries for a list of message IDs.
    async fn fetch_messages_as_summaries(
        &self,
        access_token: &str,
        message_ids: &[String],
    ) -> Result<Vec<EmailSummary>> {
        let mut emails = Vec::with_capacity(message_ids.len());

        for (i, msg_id) in message_ids.iter().enumerate() {
            debug!("Gmail API: Fetching message {}/{}", i + 1, message_ids.len());

            if let Some(summary) = self.fetch_message_summary(access_token, msg_id).await? {
                emails.push(summary);
            }
        }

        Ok(emails)
    }

    /// Fetch full email body by message ID
    pub async fn get_email_body(&self, message_id: &str) -> Result<(Option<String>, Option<String>)> {
        debug!("Gmail API: Fetching email body for message ID: {}", message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/users/me/messages/{}?format=full",
            GMAIL_API_BASE, message_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch email body")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gmail API get message failed: {} - {}",
                status,
                error_text
            ));
        }

        let message: Message = response
            .json()
            .await
            .context("Failed to parse message response")?;

        let (html_body, text_body) = extract_body_from_payload(&message.payload);

        debug!("Gmail API: Email body fetched - HTML: {}, Text: {}",
            html_body.is_some(), text_body.is_some());

        Ok((html_body, text_body))
    }

    /// Fetch folder/label list from Gmail API
    pub async fn list_folders(&self) -> Result<Vec<GmailFolder>> {
        debug!("Gmail API: Fetching folder list");

        let access_token = self.access_token().await?;
        let labels_url = format!("{}/users/me/labels", GMAIL_API_BASE);

        let response = self
            .http_client
            .get(&labels_url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch labels")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gmail API list labels failed: {} - {}",
                status,
                error_text
            ));
        }

        let labels_data: ListLabelsResponse = response
            .json()
            .await
            .context("Failed to parse labels response")?;

        let labels = labels_data.labels.unwrap_or_default();

        let system_labels = vec![
            LABEL_INBOX, LABEL_SENT, LABEL_DRAFTS, LABEL_TRASH, LABEL_SPAM, LABEL_STARRED,
        ];

        let mut folders: Vec<GmailFolder> = Vec::new();

        // Add system folders in a specific order
        for label_id in &system_labels {
            if let Some(label) = labels.iter().find(|l| l.id == *label_id) {
                folders.push(GmailFolder {
                    id: label.id.clone(),
                    name: get_display_name(&label.id),
                    folder_type: "system".to_string(),
                    message_count: label.messages_total,
                    unread_count: label.messages_unread,
                });
            }
        }

        // Add user-created labels
        for label in labels.iter() {
            if label.label_type.as_deref() == Some("user") && !label.name.starts_with("CATEGORY_") {
                folders.push(GmailFolder {
                    id: label.id.clone(),
                    name: label.name.clone(),
                    folder_type: "user".to_string(),
                    message_count: label.messages_total,
                    unread_count: label.messages_unread,
                });
            }
        }

        debug!("Gmail API: Found {} folders", folders.len());
        Ok(folders)
    }

    /// Fetch emails by label ID
    pub async fn fetch_emails_by_label(&self, label_id: &str, count: u32) -> Result<Vec<EmailSummary>> {
        debug!("Gmail API: Fetching {} emails from label {}", count, label_id);

        let access_token = self.access_token().await?;

        let (message_ids, _, _) = self
            .list_message_ids(&access_token, Some(label_id), None, count, None)
            .await?;

        debug!("Gmail API: Found {} messages in {}", message_ids.len(), label_id);

        let emails = self
            .fetch_messages_as_summaries(&access_token, &message_ids)
            .await?;

        info!("Gmail API: Fetched {} emails from {}", emails.len(), label_id);
        Ok(emails)
    }

    /// Fetch emails by label with pagination using Gmail's nextPageToken.
    /// Skips `offset` messages by walking page tokens, then fetches `limit` messages.
    pub async fn fetch_emails_by_label_paginated(
        &self,
        label_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<EmailSummary>, u32)> {
        debug!("Gmail API: Fetching paginated emails from {} (offset: {}, limit: {})", label_id, offset, limit);

        let access_token = self.access_token().await?;

        let mut total_estimate = 0u32;
        let mut page_token: Option<String> = None;
        let mut skipped = 0u32;

        // Skip pages to reach the offset
        while skipped < offset {
            let batch_size = std::cmp::min(offset - skipped, 100); // Gmail max is 100 per page
            let (ids, next_token, estimate) = self
                .list_message_ids(&access_token, Some(label_id), None, batch_size, page_token.as_deref())
                .await?;

            if let Some(est) = estimate {
                total_estimate = est;
            }

            skipped += ids.len() as u32;
            page_token = next_token;

            // No more pages
            if page_token.is_none() {
                break;
            }
        }

        // Fetch the actual page
        let (message_ids, _, estimate) = self
            .list_message_ids(&access_token, Some(label_id), None, limit, page_token.as_deref())
            .await?;

        if let Some(est) = estimate {
            total_estimate = est;
        }

        let emails = self
            .fetch_messages_as_summaries(&access_token, &message_ids)
            .await?;

        debug!("Gmail API: Paginated fetch returned {} emails (total estimate: {})", emails.len(), total_estimate);
        Ok((emails, total_estimate))
    }

    /// Fetch up to `max_count` emails by walking through all pages via nextPageToken.
    /// Returns all emails collected across pages.
    pub async fn fetch_emails_bulk(
        &self,
        label_id: &str,
        query: Option<&str>,
        max_count: u32,
    ) -> Result<Vec<Vec<EmailSummary>>> {
        let access_token = self.access_token().await?;
        let mut page_token: Option<String> = None;
        let mut total_fetched: u32 = 0;
        let page_size: u32 = 100; // Gmail API max per request
        let mut batches: Vec<Vec<EmailSummary>> = Vec::new();

        loop {
            let remaining = max_count.saturating_sub(total_fetched);
            if remaining == 0 {
                break;
            }

            let batch_size = std::cmp::min(remaining, page_size);
            let (message_ids, next_token, _) = self
                .list_message_ids(&access_token, Some(label_id), query, batch_size, page_token.as_deref())
                .await?;

            if message_ids.is_empty() {
                break;
            }

            let emails = self
                .fetch_messages_as_summaries(&access_token, &message_ids)
                .await?;

            total_fetched += emails.len() as u32;
            batches.push(emails);

            page_token = next_token;
            if page_token.is_none() {
                break;
            }
        }

        Ok(batches)
    }

    /// List attachment metadata for a message.
    pub async fn list_attachments(&self, message_id: &str) -> Result<Vec<AttachmentInfo>> {
        debug!("Gmail API: Listing attachments for message ID: {}", message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/users/me/messages/{}?format=full",
            GMAIL_API_BASE, message_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch message for attachments")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gmail API get message failed: {} - {}",
                status,
                error_text
            ));
        }

        let message: Message = response
            .json()
            .await
            .context("Failed to parse message response")?;

        let attachments = extract_attachments_from_payload(&message.payload);
        debug!("Gmail API: Found {} attachments", attachments.len());
        Ok(attachments)
    }

    /// Download attachment bytes by attachment ID.
    pub async fn download_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        debug!("Gmail API: Downloading attachment {} for message {}", attachment_id, message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/users/me/messages/{}/attachments/{}",
            GMAIL_API_BASE, message_id, attachment_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to download attachment")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gmail API download attachment failed: {} - {}",
                status,
                error_text
            ));
        }

        let att: AttachmentResponse = response
            .json()
            .await
            .context("Failed to parse attachment response")?;

        decode_base64_bytes(&att.data)
    }

    /// Fetch emails after a specific date (for delta sync)
    pub async fn fetch_emails_after_date(
        &self,
        label_id: &str,
        after_timestamp: i64,
        count: u32,
    ) -> Result<Vec<EmailSummary>> {
        debug!(
            "Gmail API: Fetching {} emails from {} after timestamp {}",
            count, label_id, after_timestamp
        );

        let access_token = self.access_token().await?;

        // Convert timestamp to YYYY/MM/DD format for Gmail API query
        use chrono::{DateTime, Utc};
        let datetime = DateTime::<Utc>::from_timestamp(after_timestamp, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?;
        let query = format!("after:{}", datetime.format("%Y/%m/%d"));

        let (message_ids, _, _) = self
            .list_message_ids(&access_token, Some(label_id), Some(&query), count, None)
            .await?;

        debug!(
            "Gmail API: Found {} messages in {} after {}",
            message_ids.len(),
            label_id,
            datetime.format("%Y/%m/%d")
        );

        if message_ids.is_empty() {
            return Ok(Vec::new());
        }

        let emails = self
            .fetch_messages_as_summaries(&access_token, &message_ids)
            .await?;

        info!(
            "Gmail API: Fetched {} emails from {} after {}",
            emails.len(),
            label_id,
            datetime.format("%Y/%m/%d")
        );
        Ok(emails)
    }
}

/// Maximum number of base64 characters per RFC 2047 encoded-word chunk so the
/// whole `=?UTF-8?B?...?=` token stays under the 75-character limit.
const RFC2047_MAX_ENCODED_CHARS_PER_WORD: usize = 60;
const RFC2047_ENCODED_WORD_PREFIX: &str = "=?UTF-8?B?";
const RFC2047_ENCODED_WORD_SUFFIX: &str = "?=";

/// Error returned when a caller-supplied header value could inject extra headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderInjectionError {
    pub header_name: String,
}

impl std::fmt::Display for HeaderInjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} header must not contain line breaks", self.header_name)
    }
}

impl std::error::Error for HeaderInjectionError {}

/// Reject header values containing CR or LF, which would terminate the header
/// and let the value smuggle in additional headers (e.g. `Bcc:`).
pub fn validate_header_value(header_name: &str, value: &str) -> std::result::Result<(), HeaderInjectionError> {
    if value.contains('\r') || value.contains('\n') {
        return Err(HeaderInjectionError { header_name: header_name.to_string() });
    }
    Ok(())
}

/// Encode a header value as RFC 2047 encoded-words when it contains
/// non-ASCII characters; pure-ASCII input is returned unchanged.
pub fn encode_header_value_rfc2047(value: &str) -> String {
    use base64::{engine::general_purpose, Engine as _};

    if value.is_ascii() {
        return value.to_string();
    }

    // Split on UTF-8 character boundaries so each chunk decodes independently.
    // 45 bytes of input -> 60 base64 chars.
    const MAX_INPUT_BYTES_PER_WORD: usize = RFC2047_MAX_ENCODED_CHARS_PER_WORD / 4 * 3;
    let mut encoded_words: Vec<String> = Vec::new();
    let mut chunk = String::new();
    for character in value.chars() {
        if chunk.len() + character.len_utf8() > MAX_INPUT_BYTES_PER_WORD {
            encoded_words.push(general_purpose::STANDARD.encode(chunk.as_bytes()));
            chunk.clear();
        }
        chunk.push(character);
    }
    if !chunk.is_empty() {
        encoded_words.push(general_purpose::STANDARD.encode(chunk.as_bytes()));
    }

    encoded_words
        .into_iter()
        .map(|word| format!("{}{}{}", RFC2047_ENCODED_WORD_PREFIX, word, RFC2047_ENCODED_WORD_SUFFIX))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a minimal RFC 2822 message for sending via the Gmail API.
///
/// Callers must have validated `to`, `subject` and `in_reply_to` with
/// [`validate_header_value`]; this function is defensive and returns an error
/// if a line break slipped through. Non-ASCII subjects are RFC 2047 encoded.
fn build_rfc2822_message(
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> std::result::Result<String, HeaderInjectionError> {
    validate_header_value("From", from)?;
    validate_header_value("To", to)?;
    validate_header_value("Subject", subject)?;
    if let Some(reply_id) = in_reply_to {
        validate_header_value("In-Reply-To", reply_id)?;
    }

    let mut message = format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n",
        from,
        to,
        encode_header_value_rfc2047(subject)
    );
    if let Some(reply_id) = in_reply_to {
        message.push_str(&format!("In-Reply-To: {}\r\nReferences: {}\r\n", reply_id, reply_id));
    }
    message.push_str(&format!("\r\n{}", body));
    Ok(message)
}

#[async_trait]
impl EmailProvider for GmailApiService {
    async fn fetch_emails(&self, folder_id: &str, count: u32) -> Result<Vec<EmailSummary>> {
        self.fetch_emails_by_label(folder_id, count).await
    }

    async fn fetch_emails_paginated(
        &self,
        folder_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<EmailSummary>, u32)> {
        self.fetch_emails_by_label_paginated(folder_id, offset, limit).await
    }

    async fn fetch_emails_after_date(
        &self,
        folder_id: &str,
        after_timestamp: i64,
        count: u32,
    ) -> Result<Vec<EmailSummary>> {
        GmailApiService::fetch_emails_after_date(self, folder_id, after_timestamp, count).await
    }

    async fn get_email_body(&self, message_id: &str) -> Result<(Option<String>, Option<String>)> {
        GmailApiService::get_email_body(self, message_id).await
    }

    async fn list_folders(&self) -> Result<Vec<EmailFolder>> {
        let gmail_folders = GmailApiService::list_folders(self).await?;
        Ok(gmail_folders
            .into_iter()
            .map(|f| EmailFolder {
                id: f.id,
                name: f.name,
                folder_type: f.folder_type,
                message_count: f.message_count,
                unread_count: f.unread_count,
            })
            .collect())
    }

    async fn list_attachments(&self, message_id: &str) -> Result<Vec<AttachmentInfo>> {
        GmailApiService::list_attachments(self, message_id).await
    }

    async fn download_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        GmailApiService::download_attachment(self, message_id, attachment_id).await
    }

    async fn send_email(&self, to: &str, subject: &str, body: &str, in_reply_to: Option<&str>) -> Result<()> {
        use base64::{Engine as _, engine::general_purpose};

        debug!("Gmail API: Sending email");
        let access_token = self.access_token().await?;

        // Gmail replaces the From header with the authenticated user's address
        let raw_message = build_rfc2822_message("me", to, subject, body, in_reply_to)
            .map_err(|e| anyhow::anyhow!("Refusing to send email: {}", e))?;
        let encoded = general_purpose::URL_SAFE_NO_PAD.encode(raw_message.as_bytes());

        let url = format!("{}/users/me/messages/send", GMAIL_API_BASE);
        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&access_token)
            .json(&serde_json::json!({ "raw": encoded }))
            .send()
            .await
            .context("Failed to send email via Gmail")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Gmail send failed: {} - {}", status, error_text));
        }

        debug!("Gmail API: Email sent successfully");
        Ok(())
    }

    fn oauth_token_state_handle(&self) -> Arc<RwLock<TokenState>> {
        GmailApiService::oauth_token_state_handle(self)
    }
}

/// Split a `From:` header into (address, display name) using a real RFC 5322
/// parser. Malformed headers never panic: the raw value is returned as the
/// address with no display name.
fn parse_from_header(from: &str) -> (String, Option<String>) {
    use mailparse::MailAddr;

    let first_mailbox = mailparse::addrparse(from).ok().and_then(|addresses| {
        addresses.iter().find_map(|address| match address {
            MailAddr::Single(info) => Some(info.clone()),
            MailAddr::Group(group) => group.addrs.first().cloned(),
        })
    });

    match first_mailbox {
        Some(info) if !info.addr.is_empty() => {
            let display_name = info.display_name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
            (info.addr, display_name)
        }
        _ => (from.trim().to_string(), None),
    }
}

/// Convert a Gmail API Message to an EmailSummary by parsing headers.
fn message_to_email_summary(message: &Message) -> EmailSummary {
    let mut subject = String::new();
    let mut from_address = String::new();
    let mut from_name = None;
    let mut date_str = String::new();

    for header in &message.payload.headers {
        match header.name.as_str() {
            "Subject" => subject = header.value.clone(),
            "From" => {
                let (address, name) = parse_from_header(&header.value);
                from_address = address;
                from_name = name;
            }
            "Date" => date_str = header.value.clone(),
            _ => {}
        }
    }

    let date = parse_email_date(&date_str).unwrap_or_else(|| chrono::Utc::now().timestamp());
    let uid = hash_message_id(&message.id);

    EmailSummary {
        id: None,
        uid,
        message_id: Some(message.id.clone()),
        subject,
        from_address,
        from_name,
        date,
        snippet: message.snippet.clone(),
        has_attachments: false,
        is_read: false,
        account_email: None,
    }
}

/// Get display name for system labels
fn get_display_name(label_id: &str) -> String {
    match label_id {
        "INBOX" => "Inbox".to_string(),
        "SENT" => "Sent".to_string(),
        "DRAFT" => "Drafts".to_string(),
        "TRASH" => "Trash".to_string(),
        "SPAM" => "Spam".to_string(),
        "STARRED" => "Starred".to_string(),
        _ => label_id.to_string(),
    }
}

/// Parse email date string to Unix timestamp
fn parse_email_date(date_str: &str) -> Option<i64> {
    use chrono::DateTime;

    // Try RFC 2822 format (most common in email headers)
    if let Ok(dt) = DateTime::parse_from_rfc2822(date_str) {
        return Some(dt.timestamp());
    }

    // Try RFC 3339 format
    if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.timestamp());
    }

    None
}

/// Hash Gmail message ID (string) to u32 for UID compatibility
fn hash_message_id(message_id: &str) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    message_id.hash(&mut hasher);
    hasher.finish() as u32
}

/// Extract HTML and text body from Gmail message payload
fn extract_body_from_payload(payload: &MessagePayload) -> (Option<String>, Option<String>) {
    let mut html_body = None;
    let mut text_body = None;

    if let Some(body) = &payload.body {
        if let Some(data) = &body.data {
            if let Some(mime_type) = &payload.mime_type {
                let decoded = decode_base64_body(data);
                match mime_type.as_str() {
                    "text/html" => html_body = Some(decoded),
                    "text/plain" => text_body = Some(decoded),
                    _ => {}
                }
            }
        }
    }

    if let Some(parts) = &payload.parts {
        for part in parts {
            let (html, text) = extract_body_from_part(part);
            if html.is_some() {
                html_body = html;
            }
            if text.is_some() {
                text_body = text;
            }
        }
    }

    (html_body, text_body)
}

/// Recursively extract body from message part
fn extract_body_from_part(part: &MessagePart) -> (Option<String>, Option<String>) {
    let mut html_body = None;
    let mut text_body = None;

    if let Some(body) = &part.body {
        if let Some(data) = &body.data {
            let decoded = decode_base64_body(data);
            match part.mime_type.as_str() {
                "text/html" => html_body = Some(decoded),
                "text/plain" => text_body = Some(decoded),
                _ => {}
            }
        }
    }

    if let Some(parts) = &part.parts {
        for nested_part in parts {
            let (html, text) = extract_body_from_part(nested_part);
            if html.is_some() {
                html_body = html;
            }
            if text.is_some() {
                text_body = text;
            }
        }
    }

    (html_body, text_body)
}

/// base64url engine that accepts both padded and unpadded input, as Gmail
/// emits unpadded base64url (RFC 4648 section 5) for body and attachment data.
const BASE64_URL_SAFE_INDIFFERENT_PADDING: base64::engine::GeneralPurpose = base64::engine::GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    base64::engine::GeneralPurposeConfig::new()
        .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
);

/// Decode base64url-encoded body data to a string.
/// Invalid data yields an empty string rather than the still-encoded input.
fn decode_base64_body(data: &str) -> String {
    use base64::Engine as _;

    match BASE64_URL_SAFE_INDIFFERENT_PADDING.decode(data) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(e) => {
            warn!("Gmail API: Failed to decode base64url body data: {}", e);
            String::new()
        }
    }
}

/// Decode base64url-encoded data to raw bytes
fn decode_base64_bytes(data: &str) -> Result<Vec<u8>> {
    use base64::Engine as _;

    BASE64_URL_SAFE_INDIFFERENT_PADDING
        .decode(data)
        .context("Failed to decode base64 attachment data")
}

/// Extract attachment metadata from a Gmail message payload
fn extract_attachments_from_payload(payload: &MessagePayload) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();
    if let Some(parts) = &payload.parts {
        collect_attachments(parts, &mut attachments);
    }
    attachments
}

/// Recursively collect attachment metadata from MIME parts
fn collect_attachments(parts: &[MessagePart], out: &mut Vec<AttachmentInfo>) {
    for part in parts {
        if let Some(filename) = &part.filename {
            if !filename.is_empty() {
                if let Some(body) = &part.body {
                    if let Some(attachment_id) = &body.attachment_id {
                        out.push(AttachmentInfo {
                            id: attachment_id.clone(),
                            filename: filename.clone(),
                            mime_type: part.mime_type.clone(),
                            size: body.size as u64,
                        });
                    }
                }
            }
        }
        if let Some(nested) = &part.parts {
            collect_attachments(nested, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, value: &str) -> MessageHeader {
        MessageHeader { name: name.to_string(), value: value.to_string() }
    }

    fn body(data: Option<&str>, attachment_id: Option<&str>, size: u32) -> MessageBody {
        MessageBody { size, data: data.map(String::from), attachment_id: attachment_id.map(String::from) }
    }

    fn part(mime: &str, data: Option<&str>) -> MessagePart {
        MessagePart { mime_type: mime.to_string(), filename: None, body: data.map(|d| body(Some(d), None, 0)), parts: None }
    }

    fn message(headers: Vec<MessageHeader>) -> Message {
        Message {
            id: "abc123".to_string(),
            thread_id: "t".to_string(),
            label_ids: None,
            snippet: Some("snip".to_string()),
            payload: MessagePayload { headers, body: None, parts: None, mime_type: None },
            internal_date: None,
        }
    }

    fn b64url(s: &str) -> String {
        use base64::{engine::general_purpose, Engine as _};
        general_purpose::URL_SAFE.encode(s.as_bytes())
    }

    // ----- base64 -----

    #[test]
    fn gmail_decode_base64_body_decodes_padded_base64url() {
        // Includes bytes that encode to '-' and '_' in the url-safe alphabet.
        let original = "hello?>world~~";
        assert_eq!(decode_base64_body(&b64url(original)), original);
    }

    #[test]
    fn gmail_decode_base64_body_decodes_unpadded_base64url() {
        // Gmail emits unpadded base64url (RFC 4648 s5); the decoder must accept it.
        use base64::{engine::general_purpose, Engine as _};
        let unpadded = general_purpose::URL_SAFE_NO_PAD.encode(b"hello");
        assert_eq!(unpadded, "aGVsbG8");
        assert_eq!(decode_base64_body(&unpadded), "hello");
    }

    #[test]
    fn gmail_decode_base64_body_returns_empty_string_on_invalid_data() {
        // The encoded input must never be shown as if it were the body.
        assert_eq!(decode_base64_body("not base64!!"), "");
    }

    #[test]
    fn gmail_decode_base64_bytes_accepts_unpadded_input() {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = vec![0xffu8, 0xfe, 0x01];
        let unpadded = general_purpose::URL_SAFE_NO_PAD.encode(&bytes);
        assert_eq!(decode_base64_bytes(&unpadded).unwrap(), bytes);
    }

    #[test]
    fn gmail_decode_base64_body_is_lossy_for_invalid_utf8() {
        use base64::{engine::general_purpose, Engine as _};
        let encoded = general_purpose::URL_SAFE.encode([0xffu8, 0x41]);
        assert_eq!(decode_base64_body(&encoded), "\u{FFFD}A");
    }

    #[test]
    fn gmail_decode_base64_bytes_round_trips_binary() {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = vec![0u8, 1, 2, 250, 251, 252, 253, 254, 255];
        let encoded = general_purpose::URL_SAFE.encode(&bytes);
        assert_eq!(decode_base64_bytes(&encoded).unwrap(), bytes);
        assert!(decode_base64_bytes("***").is_err());
    }

    // ----- dates -----

    #[test]
    fn gmail_parse_email_date_accepts_rfc2822() {
        assert_eq!(parse_email_date("Tue, 1 Jul 2003 10:52:37 +0200"), Some(1_057_049_557));
    }

    #[test]
    fn gmail_parse_email_date_accepts_rfc3339() {
        assert_eq!(parse_email_date("2003-07-01T08:52:37Z"), Some(1_057_049_557));
    }

    #[test]
    fn gmail_parse_email_date_rejects_garbage() {
        assert_eq!(parse_email_date(""), None);
        assert_eq!(parse_email_date("yesterday"), None);
    }

    // ----- hashing -----

    #[test]
    fn gmail_hash_message_id_is_deterministic_and_distinguishes_ids() {
        assert_eq!(hash_message_id("abc"), hash_message_id("abc"));
        assert_ne!(hash_message_id("abc"), hash_message_id("abd"));
    }

    // ----- header extraction / address parsing -----

    #[test]
    fn gmail_message_summary_parses_name_and_angle_bracket_address() {
        let m = message(vec![
            header("Subject", "Hello"),
            header("From", "Jane Doe <jane@example.com>"),
            header("Date", "2003-07-01T08:52:37Z"),
        ]);
        let s = message_to_email_summary(&m);
        assert_eq!(s.subject, "Hello");
        assert_eq!(s.from_address, "jane@example.com");
        assert_eq!(s.from_name.as_deref(), Some("Jane Doe"));
        assert_eq!(s.date, 1_057_049_557);
        assert_eq!(s.message_id.as_deref(), Some("abc123"));
        assert_eq!(s.uid, hash_message_id("abc123"));
        assert_eq!(s.snippet.as_deref(), Some("snip"));
        assert!(s.id.is_none());
    }

    #[test]
    fn gmail_message_summary_bare_address_has_no_name() {
        let s = message_to_email_summary(&message(vec![header("From", "bob@example.com")]));
        assert_eq!(s.from_address, "bob@example.com");
        assert_eq!(s.from_name, None);
    }

    #[test]
    fn gmail_message_summary_angle_brackets_without_name() {
        let s = message_to_email_summary(&message(vec![header("From", "<bob@example.com>")]));
        assert_eq!(s.from_address, "bob@example.com");
        assert_eq!(s.from_name, None);
    }

    #[test]
    fn gmail_message_summary_quoted_name_is_unquoted() {
        let s = message_to_email_summary(&message(vec![header("From", "\"Doe, Jane\" <jane@example.com>")]));
        assert_eq!(s.from_address, "jane@example.com");
        assert_eq!(s.from_name.as_deref(), Some("Doe, Jane"));
    }

    #[test]
    fn gmail_message_summary_unclosed_angle_bracket_does_not_panic() {
        let s = message_to_email_summary(&message(vec![header("From", "Jane <jane@example.com")]));
        assert!(!s.from_address.is_empty());
        assert!(s.from_address.contains("jane@example.com"), "{}", s.from_address);
    }

    #[test]
    fn gmail_message_summary_malformed_from_with_reversed_brackets_does_not_panic() {
        // `>` before the last `<` used to produce a reversed slice range and panic (L-1).
        for raw in ["x> <attacker@example.com", "\"a>\" <b", "<", ">", "<>", "a>b<"] {
            let s = message_to_email_summary(&message(vec![header("From", raw)]));
            assert!(!s.from_address.is_empty() || raw.trim().is_empty(), "raw={:?}", raw);
        }
    }

    #[test]
    fn gmail_parse_from_header_handles_groups_and_multiple_mailboxes() {
        assert_eq!(parse_from_header("a@x.com, b@y.com"), ("a@x.com".to_string(), None));
        let (addr, name) = parse_from_header("Team: Alice <alice@x.com>, Bob <bob@x.com>;");
        assert_eq!(addr, "alice@x.com");
        assert_eq!(name.as_deref(), Some("Alice"));
    }

    #[test]
    fn gmail_message_summary_missing_headers_yield_empty_strings_and_now_date() {
        let before = chrono::Utc::now().timestamp();
        let s = message_to_email_summary(&message(vec![]));
        assert_eq!(s.subject, "");
        assert_eq!(s.from_address, "");
        assert!(s.date >= before);
    }

    #[test]
    fn gmail_message_summary_header_names_are_case_sensitive() {
        // Documents current behaviour: lowercase header names are ignored.
        let s = message_to_email_summary(&message(vec![header("subject", "lower")]));
        assert_eq!(s.subject, "");
    }

    // ----- MIME part selection -----

    #[test]
    fn gmail_extract_body_single_part_html() {
        let payload = MessagePayload {
            headers: vec![],
            body: Some(body(Some(&b64url("<p>x</p>")), None, 0)),
            parts: None,
            mime_type: Some("text/html".to_string()),
        };
        assert_eq!(extract_body_from_payload(&payload), (Some("<p>x</p>".to_string()), None));
    }

    #[test]
    fn gmail_extract_body_single_part_plain() {
        let payload = MessagePayload {
            headers: vec![],
            body: Some(body(Some(&b64url("plain")), None, 0)),
            parts: None,
            mime_type: Some("text/plain".to_string()),
        };
        assert_eq!(extract_body_from_payload(&payload), (None, Some("plain".to_string())));
    }

    #[test]
    fn gmail_extract_body_multipart_alternative_returns_both() {
        let payload = MessagePayload {
            headers: vec![],
            body: None,
            parts: Some(vec![part("text/plain", Some(&b64url("plain"))), part("text/html", Some(&b64url("<b>h</b>")))]),
            mime_type: Some("multipart/alternative".to_string()),
        };
        assert_eq!(extract_body_from_payload(&payload), (Some("<b>h</b>".to_string()), Some("plain".to_string())));
    }

    #[test]
    fn gmail_extract_body_recurses_into_nested_multipart() {
        let inner = MessagePart {
            mime_type: "multipart/alternative".to_string(),
            filename: None,
            body: None,
            parts: Some(vec![part("text/plain", Some(&b64url("p"))), part("text/html", Some(&b64url("h")))]),
        };
        let payload = MessagePayload {
            headers: vec![],
            body: None,
            parts: Some(vec![inner, part("application/pdf", Some(&b64url("ignored")))]),
            mime_type: Some("multipart/mixed".to_string()),
        };
        assert_eq!(extract_body_from_payload(&payload), (Some("h".to_string()), Some("p".to_string())));
    }

    #[test]
    fn gmail_extract_body_last_matching_part_wins() {
        let payload = MessagePayload {
            headers: vec![],
            body: None,
            parts: Some(vec![part("text/html", Some(&b64url("first"))), part("text/html", Some(&b64url("second")))]),
            mime_type: None,
        };
        assert_eq!(extract_body_from_payload(&payload).0.as_deref(), Some("second"));
    }

    #[test]
    fn gmail_extract_body_ignores_body_without_mime_type() {
        let payload = MessagePayload {
            headers: vec![],
            body: Some(body(Some(&b64url("x")), None, 0)),
            parts: None,
            mime_type: None,
        };
        assert_eq!(extract_body_from_payload(&payload), (None, None));
    }

    // ----- attachments -----

    #[test]
    fn gmail_collect_attachments_requires_filename_and_attachment_id() {
        let named = MessagePart {
            mime_type: "application/pdf".to_string(),
            filename: Some("doc.pdf".to_string()),
            body: Some(body(None, Some("att-1"), 1234)),
            parts: None,
        };
        let empty_name = MessagePart {
            mime_type: "image/png".to_string(),
            filename: Some(String::new()),
            body: Some(body(None, Some("att-2"), 10)),
            parts: None,
        };
        let no_attachment_id = MessagePart {
            mime_type: "image/png".to_string(),
            filename: Some("inline.png".to_string()),
            body: Some(body(Some("AAAA"), None, 10)),
            parts: None,
        };
        let nested = MessagePart {
            mime_type: "multipart/mixed".to_string(),
            filename: None,
            body: None,
            parts: Some(vec![MessagePart {
                mime_type: "text/csv".to_string(),
                filename: Some("data.csv".to_string()),
                body: Some(body(None, Some("att-3"), 5)),
                parts: None,
            }]),
        };
        let payload = MessagePayload {
            headers: vec![],
            body: None,
            parts: Some(vec![named, empty_name, no_attachment_id, nested]),
            mime_type: None,
        };

        let attachments = extract_attachments_from_payload(&payload);
        let ids: Vec<&str> = attachments.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["att-1", "att-3"]);
        assert_eq!(attachments[0].filename, "doc.pdf");
        assert_eq!(attachments[0].mime_type, "application/pdf");
        assert_eq!(attachments[0].size, 1234);
    }

    #[test]
    fn gmail_extract_attachments_with_no_parts_is_empty() {
        let payload = MessagePayload { headers: vec![], body: None, parts: None, mime_type: None };
        assert!(extract_attachments_from_payload(&payload).is_empty());
    }

    // ----- outgoing message -----

    #[test]
    fn gmail_build_rfc2822_message_has_crlf_headers_and_blank_line_before_body() {
        let msg = build_rfc2822_message("me", "to@example.com", "Subj", "Body line", None).unwrap();
        assert_eq!(
            msg,
            "From: me\r\nTo: to@example.com\r\nSubject: Subj\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody line"
        );
    }

    #[test]
    fn gmail_build_rfc2822_message_adds_reply_headers() {
        let msg = build_rfc2822_message("me", "to@example.com", "Re: x", "b", Some("<orig@id>")).unwrap();
        assert!(msg.contains("In-Reply-To: <orig@id>\r\n"));
        assert!(msg.contains("References: <orig@id>\r\n"));
        assert!(msg.ends_with("\r\n\r\nb"));
    }

    #[test]
    fn gmail_build_rfc2822_message_rejects_header_injection() {
        let err = build_rfc2822_message("me", "a@b.c\r\nBcc: x@y.z", "s", "b", None).unwrap_err();
        assert_eq!(err.header_name, "To");
        assert!(build_rfc2822_message("me", "a@b.c", "s\nBcc: x", "b", None).is_err());
        assert!(build_rfc2822_message("me", "a@b.c", "s", "b", Some("<id>\r\nBcc: x")).is_err());
        // Line breaks in the body are fine.
        assert!(build_rfc2822_message("me", "a@b.c", "s", "line1\r\nline2", None).is_ok());
    }

    #[test]
    fn gmail_build_rfc2822_message_encodes_non_ascii_subject() {
        use base64::{engine::general_purpose, Engine as _};
        let msg = build_rfc2822_message("me", "a@b.c", "Grüße ✓", "b", None).unwrap();
        assert!(msg.is_ascii(), "headers must be 7-bit: {}", msg);
        let expected = format!("Subject: =?UTF-8?B?{}?=\r\n", general_purpose::STANDARD.encode("Grüße ✓".as_bytes()));
        assert!(msg.contains(&expected), "{}", msg);
    }

    #[test]
    fn gmail_validate_header_value_rejects_cr_and_lf_only() {
        assert!(validate_header_value("To", "ok <a@b.c>").is_ok());
        assert!(validate_header_value("To", "a\rb").is_err());
        assert!(validate_header_value("To", "a\nb").is_err());
        assert_eq!(
            validate_header_value("Subject", "x\r\n").unwrap_err().to_string(),
            "Subject header must not contain line breaks"
        );
    }

    #[test]
    fn gmail_encode_header_value_rfc2047_leaves_ascii_untouched() {
        assert_eq!(encode_header_value_rfc2047("Plain subject"), "Plain subject");
        assert_eq!(encode_header_value_rfc2047(""), "");
    }

    #[test]
    fn gmail_encode_header_value_rfc2047_encodes_and_round_trips() {
        use base64::{engine::general_purpose, Engine as _};
        let encoded = encode_header_value_rfc2047("Grüße");
        assert_eq!(encoded, format!("=?UTF-8?B?{}?=", general_purpose::STANDARD.encode("Grüße".as_bytes())));

        // Long subjects are split into several encoded-words of at most 75 chars,
        // each decoding to a valid UTF-8 fragment.
        let long = "日本語のテキスト".repeat(10);
        let encoded = encode_header_value_rfc2047(&long);
        let mut decoded = String::new();
        for word in encoded.split(' ') {
            assert!(word.len() <= 75, "encoded-word too long: {}", word);
            let inner = word.strip_prefix("=?UTF-8?B?").unwrap().strip_suffix("?=").unwrap();
            decoded.push_str(std::str::from_utf8(&general_purpose::STANDARD.decode(inner).unwrap()).unwrap());
        }
        assert_eq!(decoded, long);
    }

    #[test]
    fn gmail_get_display_name_maps_system_labels_and_passes_through_others() {
        assert_eq!(get_display_name("INBOX"), "Inbox");
        assert_eq!(get_display_name("DRAFT"), "Drafts");
        assert_eq!(get_display_name("TRASH"), "Trash");
        assert_eq!(get_display_name("Label_42"), "Label_42");
    }
}
