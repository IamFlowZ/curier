use serde::{Deserialize, Serialize};

/// Custom client-side tag for organizing emails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub created_at: i64,
}

/// Input for creating a new tag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTagInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Email-tag association with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailTag {
    pub email_id: i64,
    pub tag_id: i64,
    pub applied_by: String, // 'manual' | 'rule:{rule_id}'
    pub applied_at: i64,
}

/// Filter criterion for matching emails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterCriterion {
    /// Field to match: 'from', 'to', 'subject', 'body', 'tag', 'is_read', 'has_attachments', 'date'
    pub field: String,
    /// Operator: 'equals', 'contains', 'starts_with', 'wildcard', 'before', 'after', 'between'
    pub operator: String,
    /// Value to match against (can be string, bool, number, or tuple for 'between')
    pub value: serde_json::Value,
    /// If true, negates the match (NOT operator)
    #[serde(default)]
    pub negate: bool,
}

/// Auto-tagging rule for automatically applying tags to emails
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaggingRule {
    pub id: i64,
    pub name: String,
    pub tag_id: i64,
    pub criteria: Vec<FilterCriterion>,
    /// 'all' = AND logic, 'any' = OR logic
    pub match_mode: String,
    pub priority: i32,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Input for creating a new tagging rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaggingRuleInput {
    pub name: String,
    pub tag_id: i64,
    pub criteria: Vec<FilterCriterion>,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_match_mode() -> String {
    "all".to_string()
}

fn default_enabled() -> bool {
    true
}

/// Feed column definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub id: i64,
    pub name: String,
    /// NULL = all accounts, specific email = account-scoped
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_scope: Option<String>,
    pub criteria: Vec<FilterCriterion>,
    /// 'all' = AND logic, 'any' = OR logic
    pub match_mode: String,
    pub column_order: i32,
    /// Column width on 1-12 scale (Bootstrap-style). Uses proportional flex sizing.
    pub column_width: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Input for creating a new feed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFeedInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_scope: Option<String>,
    pub criteria: Vec<FilterCriterion>,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    #[serde(default)]
    pub column_order: i32,
    #[serde(default = "default_column_width")]
    pub column_width: i32,
}

fn default_column_width() -> i32 {
    4
}

/// Input for updating an existing feed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFeedInput {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub criteria: Option<Vec<FilterCriterion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column_width: Option<i32>,
}

/// Result of running tagging rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecutionResult {
    pub rules_executed: i32,
    pub emails_processed: i32,
    pub tags_applied: i32,
    pub tags_removed: i32,
}

/// Rule execution tracking record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleExecution {
    pub id: i64,
    pub rule_id: i64,
    pub executed_at: i64,
    pub emails_matched: i32,
}

/// Paginated feed emails response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedFeedResponse {
    pub emails: Vec<crate::models::email::EmailSummary>,
    pub total_count: u32,
    pub page: u32,
    pub has_more: bool,
}

/// Email with tags attached
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailWithTags {
    #[serde(flatten)]
    pub email: crate::models::email::EmailSummary,
    pub tags: Vec<Tag>,
}
