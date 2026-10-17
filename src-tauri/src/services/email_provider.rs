use crate::models::email::{AttachmentInfo, EmailSummary};
use crate::services::oauth::TokenState;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Provider-agnostic email folder representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailFolder {
    pub id: String,
    pub name: String,
    /// "system" or "user"
    pub folder_type: String,
    pub message_count: Option<u32>,
    pub unread_count: Option<u32>,
}

/// Abstraction over email backends (Gmail, Outlook, etc.).
///
/// Every method mirrors the surface area used by the command and sync layers
/// so that swapping providers requires no changes in those layers.
#[async_trait]
pub trait EmailProvider: Send + Sync {
    /// Fetch emails from a folder/label, up to `count`.
    async fn fetch_emails(&self, folder_id: &str, count: u32) -> Result<Vec<EmailSummary>>;

    /// Fetch emails from a folder with offset-based pagination.
    /// Returns `(emails, total_count_estimate)`.
    async fn fetch_emails_paginated(
        &self,
        folder_id: &str,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<EmailSummary>, u32)>;

    /// Fetch emails received after `after_timestamp` (unix seconds) for delta sync.
    async fn fetch_emails_after_date(
        &self,
        folder_id: &str,
        after_timestamp: i64,
        count: u32,
    ) -> Result<Vec<EmailSummary>>;

    /// Fetch the full body of a message.
    /// Returns `(html_body, text_body)`.
    async fn get_email_body(&self, message_id: &str) -> Result<(Option<String>, Option<String>)>;

    /// List available folders/labels.
    async fn list_folders(&self) -> Result<Vec<EmailFolder>>;

    /// List attachment metadata for a message.
    async fn list_attachments(&self, message_id: &str) -> Result<Vec<AttachmentInfo>>;

    /// Download attachment bytes by attachment ID.
    async fn download_attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>>;

    /// Send an email. `in_reply_to` is an optional Message-ID for threading replies.
    async fn send_email(&self, to: &str, subject: &str, body: &str, in_reply_to: Option<&str>) -> Result<()>;

    /// Get a handle to the shared OAuth token state for cache write-back.
    fn oauth_token_state_handle(&self) -> Arc<RwLock<TokenState>>;
}
