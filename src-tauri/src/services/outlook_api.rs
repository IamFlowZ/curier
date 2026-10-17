use crate::models::email::{AttachmentInfo, EmailSummary};
use crate::services::email_provider::{EmailFolder, EmailProvider};
use crate::services::oauth::{OAuthService, TokenState};
use anyhow::{Context, Result};
use async_trait::async_trait;
use log::{debug, info};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Selected fields for the `$select` query parameter.
const MESSAGE_SELECT: &str = "id,subject,from,receivedDateTime,bodyPreview,hasAttachments,isRead";

// ── Graph API response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GraphMessagesResponse {
    value: Vec<GraphMessage>,
    #[serde(rename = "@odata.count")]
    odata_count: Option<u32>,
    #[serde(rename = "@odata.nextLink")]
    odata_next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessage {
    id: String,
    subject: Option<String>,
    from: Option<GraphEmailAddressWrapper>,
    received_date_time: Option<String>,
    body_preview: Option<String>,
    has_attachments: Option<bool>,
    is_read: Option<bool>,
    body: Option<GraphBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphEmailAddressWrapper {
    email_address: GraphEmailAddress,
}

#[derive(Debug, Deserialize)]
struct GraphEmailAddress {
    name: Option<String>,
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphBody {
    content_type: Option<String>,
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphAttachmentsResponse {
    value: Vec<GraphAttachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphAttachment {
    id: String,
    name: Option<String>,
    content_type: Option<String>,
    size: Option<u64>,
    content_bytes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphFoldersResponse {
    value: Vec<GraphFolder>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphFolder {
    id: String,
    display_name: String,
    total_item_count: Option<u32>,
    unread_item_count: Option<u32>,
}

// ── Service implementation ──────────────────────────────────────────────────

pub struct OutlookApiService {
    oauth_service: OAuthService,
    app_data_dir: PathBuf,
    http_client: reqwest::Client,
}

impl OutlookApiService {
    pub fn new(oauth_service: OAuthService, app_data_dir: PathBuf) -> Self {
        Self {
            oauth_service,
            app_data_dir,
            http_client: reqwest::Client::new(),
        }
    }

    /// Get a valid access token (convenience wrapper).
    async fn access_token(&self) -> Result<String> {
        self.oauth_service
            .get_valid_access_token(&self.app_data_dir)
            .await
            .context("Failed to get access token for Outlook")
    }

    /// Normalize folder IDs.
    /// The sync service uses Gmail-style folder names like "INBOX".
    /// Graph API uses well-known folder names like "inbox", "sentitems", etc.
    fn normalize_folder_id(folder_id: &str) -> &str {
        match folder_id {
            "INBOX" => "inbox",
            "SENT" => "sentitems",
            "DRAFT" | "DRAFTS" => "drafts",
            "TRASH" => "deleteditems",
            "SPAM" => "junkemail",
            other => other,
        }
    }
}

#[async_trait]
impl EmailProvider for OutlookApiService {
    async fn fetch_emails(&self, folder_id: &str, count: u32) -> Result<Vec<EmailSummary>> {
        let folder = Self::normalize_folder_id(folder_id);
        debug!("Outlook API: Fetching {} emails from folder {}", count, folder);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/me/mailFolders/{}/messages?$top={}&$select={}&$orderby=receivedDateTime desc",
            GRAPH_API_BASE, folder, count, MESSAGE_SELECT
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch messages from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API list messages failed: {} - {}",
                status,
                error_text
            ));
        }

        let data: GraphMessagesResponse = response
            .json()
            .await
            .context("Failed to parse Graph messages response")?;

        let emails: Vec<EmailSummary> = data
            .value
            .into_iter()
            .map(|m| graph_message_to_email_summary(m))
            .collect();

        info!("Outlook API: Fetched {} emails from {}", emails.len(), folder);
        Ok(emails)
    }

    async fn fetch_emails_paginated(
        &self,
        folder_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<EmailSummary>, u32)> {
        let folder = Self::normalize_folder_id(folder_id);
        debug!(
            "Outlook API: Fetching paginated emails from {} (offset: {}, limit: {})",
            folder, offset, limit
        );

        let access_token = self.access_token().await?;

        // Use $skip and $top for pagination, $count for total
        let url = format!(
            "{}/me/mailFolders/{}/messages?$top={}&$skip={}&$select={}&$orderby=receivedDateTime desc&$count=true",
            GRAPH_API_BASE, folder, limit, offset, MESSAGE_SELECT
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .header("ConsistencyLevel", "eventual") // required for $count
            .send()
            .await
            .context("Failed to fetch paginated messages from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API paginated messages failed: {} - {}",
                status,
                error_text
            ));
        }

        let data: GraphMessagesResponse = response
            .json()
            .await
            .context("Failed to parse Graph paginated response")?;

        let total_count = data.odata_count.unwrap_or(0);
        let emails: Vec<EmailSummary> = data
            .value
            .into_iter()
            .map(|m| graph_message_to_email_summary(m))
            .collect();

        debug!(
            "Outlook API: Paginated fetch returned {} emails (total: {})",
            emails.len(),
            total_count
        );
        Ok((emails, total_count))
    }

    async fn fetch_emails_after_date(
        &self,
        folder_id: &str,
        after_timestamp: i64,
        count: u32,
    ) -> Result<Vec<EmailSummary>> {
        let folder = Self::normalize_folder_id(folder_id);
        debug!(
            "Outlook API: Fetching {} emails from {} after timestamp {}",
            count, folder, after_timestamp
        );

        let access_token = self.access_token().await?;

        // Convert timestamp to ISO 8601 for Graph $filter
        let datetime = chrono::DateTime::<chrono::Utc>::from_timestamp(after_timestamp, 0)
            .ok_or_else(|| anyhow::anyhow!("Invalid timestamp"))?;
        let iso_date = datetime.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let url = format!(
            "{}/me/mailFolders/{}/messages?$top={}&$select={}&$orderby=receivedDateTime desc&$filter=receivedDateTime ge {}",
            GRAPH_API_BASE, folder, count, MESSAGE_SELECT, iso_date
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch messages after date from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API filter messages failed: {} - {}",
                status,
                error_text
            ));
        }

        let data: GraphMessagesResponse = response
            .json()
            .await
            .context("Failed to parse Graph filtered response")?;

        let emails: Vec<EmailSummary> = data
            .value
            .into_iter()
            .map(|m| graph_message_to_email_summary(m))
            .collect();

        info!(
            "Outlook API: Fetched {} emails from {} after {}",
            emails.len(),
            folder,
            iso_date
        );
        Ok(emails)
    }

    async fn get_email_body(&self, message_id: &str) -> Result<(Option<String>, Option<String>)> {
        debug!("Outlook API: Fetching body for message ID: {}", message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/me/messages/{}?$select=body",
            GRAPH_API_BASE, message_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch message body from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API get message body failed: {} - {}",
                status,
                error_text
            ));
        }

        let message: GraphMessage = response
            .json()
            .await
            .context("Failed to parse Graph message body response")?;

        let (html_body, text_body) = match message.body {
            Some(body) => {
                let content = body.content;
                match body.content_type.as_deref() {
                    Some("html") => (content, None),
                    Some("text") => (None, content),
                    _ => (content, None), // default to html
                }
            }
            None => (None, None),
        };

        debug!(
            "Outlook API: Email body fetched - HTML: {}, Text: {}",
            html_body.is_some(),
            text_body.is_some()
        );

        Ok((html_body, text_body))
    }

    async fn list_folders(&self) -> Result<Vec<EmailFolder>> {
        debug!("Outlook API: Fetching folder list");

        let access_token = self.access_token().await?;

        let url = format!("{}/me/mailFolders?$top=100", GRAPH_API_BASE);

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to fetch folders from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API list folders failed: {} - {}",
                status,
                error_text
            ));
        }

        let data: GraphFoldersResponse = response
            .json()
            .await
            .context("Failed to parse Graph folders response")?;

        // Well-known folder display names (Graph returns localized names)
        let well_known_names = [
            "Inbox", "Sent Items", "Drafts", "Deleted Items", "Junk Email",
            "Archive", "Outbox",
        ];

        let folders: Vec<EmailFolder> = data
            .value
            .into_iter()
            .map(|f| {
                let folder_type = if well_known_names.iter().any(|n| *n == f.display_name) {
                    "system".to_string()
                } else {
                    "user".to_string()
                };

                EmailFolder {
                    id: f.id,
                    name: f.display_name,
                    folder_type,
                    message_count: f.total_item_count,
                    unread_count: f.unread_item_count,
                }
            })
            .collect();

        debug!("Outlook API: Found {} folders", folders.len());
        Ok(folders)
    }

    async fn list_attachments(&self, message_id: &str) -> Result<Vec<AttachmentInfo>> {
        debug!("Outlook API: Listing attachments for message ID: {}", message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/me/messages/{}/attachments?$select=id,name,contentType,size",
            GRAPH_API_BASE, message_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to list attachments from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API list attachments failed: {} - {}",
                status,
                error_text
            ));
        }

        let data: GraphAttachmentsResponse = response
            .json()
            .await
            .context("Failed to parse Graph attachments response")?;

        let attachments: Vec<AttachmentInfo> = data
            .value
            .into_iter()
            .map(|a| AttachmentInfo {
                id: a.id,
                filename: a.name.unwrap_or_else(|| "attachment".to_string()),
                mime_type: a.content_type.unwrap_or_else(|| "application/octet-stream".to_string()),
                size: a.size.unwrap_or(0),
            })
            .collect();

        debug!("Outlook API: Found {} attachments", attachments.len());
        Ok(attachments)
    }

    async fn download_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        debug!("Outlook API: Downloading attachment {} for message {}", attachment_id, message_id);

        let access_token = self.access_token().await?;

        let url = format!(
            "{}/me/messages/{}/attachments/{}",
            GRAPH_API_BASE, message_id, attachment_id
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&access_token)
            .send()
            .await
            .context("Failed to download attachment from Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Graph API download attachment failed: {} - {}",
                status,
                error_text
            ));
        }

        let att: GraphAttachment = response
            .json()
            .await
            .context("Failed to parse Graph attachment response")?;

        let content_b64 = att
            .content_bytes
            .ok_or_else(|| anyhow::anyhow!("No content bytes in attachment response"))?;

        use base64::{Engine as _, engine::general_purpose};
        general_purpose::STANDARD
            .decode(&content_b64)
            .context("Failed to decode attachment content bytes")
    }

    async fn send_email(&self, to: &str, subject: &str, body: &str, in_reply_to: Option<&str>) -> Result<()> {
        debug!("Outlook API: Sending email to {}", to);
        let access_token = self.access_token().await?;

        let url = format!("{}/me/sendMail", GRAPH_API_BASE);

        let mut message = serde_json::json!({
            "message": {
                "subject": subject,
                "body": {
                    "contentType": "text",
                    "content": body
                },
                "toRecipients": [
                    {
                        "emailAddress": {
                            "address": to
                        }
                    }
                ]
            }
        });

        if let Some(reply_id) = in_reply_to {
            message["message"]["internetMessageHeaders"] = serde_json::json!([
                { "name": "In-Reply-To", "value": reply_id },
                { "name": "References", "value": reply_id }
            ]);
        }

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&access_token)
            .json(&message)
            .send()
            .await
            .context("Failed to send email via Outlook")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Outlook send failed: {} - {}. You may need to re-authenticate your Outlook account to enable sending.", status, error_text));
        }

        info!("Outlook API: Email sent successfully to {}", to);
        Ok(())
    }

    fn oauth_token_state_handle(&self) -> Arc<RwLock<TokenState>> {
        self.oauth_service.token_state_handle()
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Convert a Graph API message to an EmailSummary.
fn graph_message_to_email_summary(message: GraphMessage) -> EmailSummary {
    let from_address = message
        .from
        .as_ref()
        .and_then(|f| f.email_address.address.clone())
        .unwrap_or_default();

    let from_name = message
        .from
        .as_ref()
        .and_then(|f| f.email_address.name.clone());

    // Parse ISO 8601 received date to unix timestamp
    let date = message
        .received_date_time
        .as_deref()
        .and_then(parse_iso8601_to_timestamp)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let uid = hash_message_id(&message.id);

    EmailSummary {
        id: None,
        uid,
        message_id: Some(message.id),
        subject: message.subject.unwrap_or_default(),
        from_address,
        from_name,
        date,
        snippet: message.body_preview,
        has_attachments: message.has_attachments.unwrap_or(false),
        is_read: message.is_read.unwrap_or(false),
        account_email: None,
    }
}

/// Parse ISO 8601 date string to Unix timestamp.
fn parse_iso8601_to_timestamp(date_str: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(date_str)
        .ok()
        .map(|dt| dt.timestamp())
        .or_else(|| {
            // Graph sometimes returns without timezone offset
            chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|ndt| ndt.and_utc().timestamp())
        })
}

/// Hash message ID string to u32 for UID compatibility.
fn hash_message_id(message_id: &str) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    message_id.hash(&mut hasher);
    hasher.finish() as u32
}
