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

    /// Fetch recent emails from INBOX
    pub async fn fetch_recent_emails(&self, count: u32) -> Result<Vec<EmailSummary>> {
        self.fetch_emails_by_label(LABEL_INBOX, count).await
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

    /// Fetch emails (alias for fetch_emails_by_label)
    pub async fn fetch_emails(&self, label_id: &str, count: u32) -> Result<Vec<EmailSummary>> {
        self.fetch_emails_by_label(label_id, count).await
    }

    /// Fetch recent emails with pagination using Gmail's nextPageToken.
    /// Returns (emails, total_count_estimate).
    pub async fn fetch_recent_emails_paginated(&self, offset: u32, limit: u32) -> Result<(Vec<EmailSummary>, u32)> {
        self.fetch_emails_by_label_paginated(LABEL_INBOX, offset, limit).await
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

/// Build a minimal RFC 2822 message for sending via the Gmail API.
fn build_rfc2822_message(from: &str, to: &str, subject: &str, body: &str, in_reply_to: Option<&str>) -> String {
    let mut message = format!(
        "From: {}\r\nTo: {}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n",
        from, to, subject
    );
    if let Some(reply_id) = in_reply_to {
        message.push_str(&format!("In-Reply-To: {}\r\nReferences: {}\r\n", reply_id, reply_id));
    }
    message.push_str(&format!("\r\n{}", body));
    message
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

        debug!("Gmail API: Sending email to {}", to);
        let access_token = self.access_token().await?;

        // Gmail replaces the From header with the authenticated user's address
        let raw_message = build_rfc2822_message("me", to, subject, body, in_reply_to);
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

        info!("Gmail API: Email sent successfully to {}", to);
        Ok(())
    }

    fn oauth_token_state_handle(&self) -> Arc<RwLock<TokenState>> {
        GmailApiService::oauth_token_state_handle(self)
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
                let from = &header.value;
                if let Some(email_start) = from.rfind('<') {
                    if let Some(email_end) = from.rfind('>') {
                        from_address = from[email_start + 1..email_end].to_string();
                        let name_part = from[..email_start].trim();
                        if !name_part.is_empty() {
                            from_name = Some(name_part.to_string());
                        }
                    } else {
                        from_address = from.clone();
                    }
                } else {
                    from_address = from.clone();
                }
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

/// Decode base64url-encoded body data to a string
fn decode_base64_body(data: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};

    let normalized = data.replace('-', "+").replace('_', "/");

    match general_purpose::STANDARD.decode(&normalized) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
        Err(_) => data.to_string(),
    }
}

/// Decode base64url-encoded data to raw bytes
fn decode_base64_bytes(data: &str) -> Result<Vec<u8>> {
    use base64::{Engine as _, engine::general_purpose};

    let normalized = data.replace('-', "+").replace('_', "/");
    general_purpose::STANDARD
        .decode(&normalized)
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
