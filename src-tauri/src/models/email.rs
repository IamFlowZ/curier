use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailSummary {
    pub id: Option<i64>,        // Database ID
    pub uid: u32,               // IMAP UID (or hashed message ID)
    pub message_id: Option<String>, // Gmail message ID for fetching body
    pub subject: String,
    pub from_address: String,
    pub from_name: Option<String>,
    pub date: i64,              // Unix timestamp
    pub snippet: Option<String>,
    pub has_attachments: bool,
    pub is_read: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_email: Option<String>, // Which account this email belongs to
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailBody {
    pub email_id: i64,
    pub html_body: Option<String>,
    pub text_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedEmailResponse {
    pub emails: Vec<EmailSummary>,
    pub total_count: u32,
    pub current_page: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
}
