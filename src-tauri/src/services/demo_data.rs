//! Demo data mode.
//!
//! When the `CURIER_DEMO_DATA` environment variable is set to `1`, the app runs
//! against a separate data directory (`<app_data_dir>/demo/`) pre-populated with
//! two fake accounts and a deterministic set of synthetic emails, tags, tagging
//! rules and feeds. No OAuth flow runs and nothing touches the network, which
//! makes the mode suitable for screenshots and for contributors without a
//! Gmail or Outlook account.
//!
//! Everything here is deterministic: the only time-dependent input is "now",
//! which is used so that the seeded emails always look recent.

use crate::models::auth::OAuthConfig;
use crate::models::email::{EmailBody, EmailSummary};
use crate::models::feeds::{CreateFeedInput, CreateTaggingRuleInput, FilterCriterion, TaggingRule};
use crate::services::database::DatabaseService;
use crate::services::rule_engine::evaluate_email_against_rule;
use anyhow::{Context, Result};
use log::{debug, info};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// Environment variable that activates demo mode when set to [`DEMO_DATA_ENV_ENABLED_VALUE`].
pub const DEMO_DATA_ENV_VAR: &str = "CURIER_DEMO_DATA";
/// The value of [`DEMO_DATA_ENV_VAR`] that activates demo mode.
pub const DEMO_DATA_ENV_ENABLED_VALUE: &str = "1";
/// Subdirectory of the app data directory that holds the demo database and settings.
pub const DEMO_DATA_DIR_NAME: &str = "demo";

/// First demo account (a personal mailbox).
pub const DEMO_PRIMARY_ACCOUNT_EMAIL: &str = "dakota@example.com";
/// Second demo account (a work mailbox).
pub const DEMO_SECONDARY_ACCOUNT_EMAIL: &str = "work@example.org";

/// Tab color bar for the primary account (matches the frontend palette's blue).
const DEMO_PRIMARY_ACCOUNT_COLOR: &str = "#3b82f6";
/// Tab color bar for the secondary account (matches the frontend palette's green).
const DEMO_SECONDARY_ACCOUNT_COLOR: &str = "#10b981";

/// Folder every demo email is filed under; the inbox commands only read `INBOX`.
const DEMO_FOLDER_ID: &str = "INBOX";
/// Body cache capacity used while seeding. Well above the number of seeded
/// emails so that `store_email_body_lru` never evicts a demo body.
const DEMO_BODY_CACHE_CAPACITY: usize = 1_000;
/// Prefix for synthetic provider message ids (the inbox UI fetches bodies by message id).
const DEMO_MESSAGE_ID_PREFIX: &str = "demo";
/// `applied_by` prefix the tagging-rule runner uses; matching it means the
/// startup rule run treats seeded tags as its own and keeps them consistent.
const RULE_APPLIED_BY_PREFIX: &str = "rule:";

const MATCH_MODE_ALL: &str = "all";
const MATCH_MODE_ANY: &str = "any";

const FIELD_FROM: &str = "from";
const FIELD_SUBJECT: &str = "subject";
const FIELD_TAG: &str = "tag";
const OPERATOR_EQUALS: &str = "equals";
const OPERATOR_CONTAINS: &str = "contains";
const OPERATOR_STARTS_WITH: &str = "starts_with";
const OPERATOR_WILDCARD: &str = "wildcard";

const SECONDS_PER_MINUTE: i64 = 60;
const SECONDS_PER_HOUR: i64 = 60 * SECONDS_PER_MINUTE;
/// Spreads emails off the exact hour so timestamps look organic.
const MINUTE_JITTER_STEP: i64 = 7;

/// Default feed column width on the 1-12 scale (mirrors `CreateFeedInput`'s serde default).
const DEFAULT_FEED_COLUMN_WIDTH: i32 = 4;

/// Returns true when the app was launched with `CURIER_DEMO_DATA=1`.
pub fn is_demo_mode() -> bool {
    std::env::var(DEMO_DATA_ENV_VAR)
        .map(|value| value.trim() == DEMO_DATA_ENV_ENABLED_VALUE)
        .unwrap_or(false)
}

/// Map the platform app data directory to the directory the app should use.
/// In demo mode this is a `demo/` subdirectory so the real database and
/// settings are never touched; otherwise it is the directory unchanged.
pub fn data_dir_for_mode(platform_app_data_dir: &Path, demo_mode: bool) -> PathBuf {
    if demo_mode {
        platform_app_data_dir.join(DEMO_DATA_DIR_NAME)
    } else {
        platform_app_data_dir.to_path_buf()
    }
}

/// Resolve the data directory for this app instance: the single choke point
/// used by setup and by every command that needs the data directory.
pub fn resolve_data_dir(app_handle: &tauri::AppHandle) -> std::result::Result<PathBuf, String> {
    use tauri::Manager;

    let platform_app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    Ok(data_dir_for_mode(&platform_app_data_dir, is_demo_mode()))
}

/// Account configs for the two demo accounts, keyed the way `AppState`'s
/// multi-account cache keys them (by email). They carry no client id,
/// secret or tokens, so nothing can attempt an OAuth call with them.
pub fn demo_account_configs() -> Vec<(String, OAuthConfig)> {
    [DEMO_PRIMARY_ACCOUNT_EMAIL, DEMO_SECONDARY_ACCOUNT_EMAIL]
        .iter()
        .map(|email| {
            let config = OAuthConfig {
                email: Some((*email).to_string()),
                ..OAuthConfig::google_defaults()
            };
            ((*email).to_string(), config)
        })
        .collect()
}

/// What `seed_demo_data` did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DemoSeedSummary {
    /// True when the database already held emails and nothing was written.
    pub skipped_already_seeded: bool,
    pub emails_inserted: usize,
    pub bodies_stored: usize,
    pub tags_created: usize,
    pub tagging_rules_created: usize,
    pub feeds_created: usize,
    pub tags_applied: usize,
}

impl fmt::Display for DemoSeedSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.skipped_already_seeded {
            return write!(formatter, "demo database already seeded; nothing written");
        }
        write!(
            formatter,
            "seeded {} emails, {} bodies, {} tags, {} tagging rules, {} feeds, {} tag applications",
            self.emails_inserted,
            self.bodies_stored,
            self.tags_created,
            self.tagging_rules_created,
            self.feeds_created,
            self.tags_applied,
        )
    }
}

/// Seed the demo database. Idempotent: a database that already contains any
/// email is left untouched and the summary reports `skipped_already_seeded`.
pub fn seed_demo_data(db: &DatabaseService) -> Result<DemoSeedSummary> {
    let (existing_email_count, _, _) = db
        .get_cache_stats()
        .context("Failed to check whether the demo database is empty")?;
    if existing_email_count > 0 {
        debug!("DEMO: Database already holds {} emails; skipping seed", existing_email_count);
        return Ok(DemoSeedSummary {
            skipped_already_seeded: true,
            ..DemoSeedSummary::default()
        });
    }

    info!("DEMO: Seeding demo database");
    let now = chrono::Utc::now().timestamp();

    let tag_ids = create_demo_tags(db)?;
    let rules = create_demo_tagging_rules(db, &tag_ids)?;
    let feeds_created = create_demo_feeds(db, &tag_ids)?;
    set_demo_account_colors(db)?;

    let mut emails_inserted: usize = 0;
    let mut bodies_stored: usize = 0;
    let mut tags_applied: usize = 0;

    for (index, template) in DEMO_EMAILS.iter().enumerate() {
        let uid = demo_uid_for(index);
        let summary = template.to_summary(uid, now);

        let email_id = db
            .upsert_email_with_context(&summary, template.account.email(), DEMO_FOLDER_ID)
            .with_context(|| format!("Failed to insert demo email '{}'", template.subject))?;
        emails_inserted += 1;

        let body = EmailBody {
            email_id,
            html_body: Some(template.html_body()),
            text_body: Some(template.text_body()),
        };
        db.store_email_body_lru(email_id, &body, DEMO_BODY_CACHE_CAPACITY)
            .with_context(|| format!("Failed to store body for demo email '{}'", template.subject))?;
        bodies_stored += 1;

        let inserted_summary = EmailSummary {
            id: Some(email_id),
            ..summary
        };
        tags_applied += apply_rules_to_email(db, &inserted_summary, &rules)?;
    }

    let summary = DemoSeedSummary {
        skipped_already_seeded: false,
        emails_inserted,
        bodies_stored,
        tags_created: tag_ids.len(),
        tagging_rules_created: rules.len(),
        feeds_created,
        tags_applied,
    };
    info!("DEMO: {}", summary);
    Ok(summary)
}

// ============ Accounts ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DemoAccount {
    Primary,
    Secondary,
}

impl DemoAccount {
    fn email(self) -> &'static str {
        match self {
            DemoAccount::Primary => DEMO_PRIMARY_ACCOUNT_EMAIL,
            DemoAccount::Secondary => DEMO_SECONDARY_ACCOUNT_EMAIL,
        }
    }

    fn color(self) -> &'static str {
        match self {
            DemoAccount::Primary => DEMO_PRIMARY_ACCOUNT_COLOR,
            DemoAccount::Secondary => DEMO_SECONDARY_ACCOUNT_COLOR,
        }
    }

    /// Short, filesystem/URL-safe label used inside synthetic message ids.
    fn slug(self) -> &'static str {
        match self {
            DemoAccount::Primary => "personal",
            DemoAccount::Secondary => "work",
        }
    }
}

const DEMO_ACCOUNTS: [DemoAccount; 2] = [DemoAccount::Primary, DemoAccount::Secondary];

fn set_demo_account_colors(db: &DatabaseService) -> Result<()> {
    for account in DEMO_ACCOUNTS {
        db.set_account_color(account.email(), account.color())
            .with_context(|| format!("Failed to set demo color for {}", account.email()))?;
    }
    Ok(())
}

// ============ Tags ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DemoTag {
    Newsletters,
    Receipts,
    GitHub,
    Personal,
    Calendar,
}

impl DemoTag {
    fn name(self) -> &'static str {
        match self {
            DemoTag::Newsletters => "Newsletters",
            DemoTag::Receipts => "Receipts",
            DemoTag::GitHub => "GitHub",
            DemoTag::Personal => "Personal",
            DemoTag::Calendar => "Calendar",
        }
    }

    fn color(self) -> &'static str {
        match self {
            DemoTag::Newsletters => "#06b6d4",
            DemoTag::Receipts => "#f59e0b",
            DemoTag::GitHub => "#a855f7",
            DemoTag::Personal => "#10b981",
            DemoTag::Calendar => "#3b82f6",
        }
    }
}

const DEMO_TAGS: [DemoTag; 5] = [
    DemoTag::Newsletters,
    DemoTag::Receipts,
    DemoTag::GitHub,
    DemoTag::Personal,
    DemoTag::Calendar,
];

fn create_demo_tags(db: &DatabaseService) -> Result<HashMap<DemoTag, i64>> {
    let mut tag_ids: HashMap<DemoTag, i64> = HashMap::with_capacity(DEMO_TAGS.len());
    for tag in DEMO_TAGS {
        let created = db
            .create_tag(tag.name(), Some(tag.color()), None)
            .with_context(|| format!("Failed to create demo tag '{}'", tag.name()))?;
        tag_ids.insert(tag, created.id);
    }
    Ok(tag_ids)
}

fn tag_id(tag_ids: &HashMap<DemoTag, i64>, tag: DemoTag) -> Result<i64> {
    tag_ids
        .get(&tag)
        .copied()
        .with_context(|| format!("Demo tag '{}' was not created", tag.name()))
}

// ============ Tagging rules ============

fn criterion(field: &str, operator: &str, value: &str) -> FilterCriterion {
    FilterCriterion {
        field: field.to_string(),
        operator: operator.to_string(),
        value: json!(value),
        negate: false,
    }
}

/// Senders whose address contains this are newsletters.
const NEWSLETTER_SENDER_MARKER: &str = "newsletter";
/// Subject prefix GitHub-style notifications carry.
const GITHUB_SUBJECT_PREFIX: &str = "[curier]";
/// Order-system sender marker.
const ORDERS_SENDER_MARKER: &str = "orders@";
/// Receipt subject marker.
const RECEIPT_SUBJECT_MARKER: &str = "receipt";
/// Calendar invite subject marker.
const CALENDAR_SUBJECT_MARKER: &str = "Invitation:";
/// People in the demo write from bare `example.net` addresses.
const PERSONAL_SENDER_PATTERN: &str = "*@example.net";

/// Rules are deliberately expressed with criteria that both the in-process
/// rule engine (`services::rule_engine`) and the feed SQL builder
/// (`commands::feeds::build_criteria_where_clause`) understand.
fn demo_tagging_rule_inputs(tag_ids: &HashMap<DemoTag, i64>) -> Result<Vec<CreateTaggingRuleInput>> {
    let rules = vec![
        CreateTaggingRuleInput {
            name: "Newsletters".to_string(),
            tag_id: tag_id(tag_ids, DemoTag::Newsletters)?,
            criteria: vec![criterion(FIELD_FROM, OPERATOR_CONTAINS, NEWSLETTER_SENDER_MARKER)],
            match_mode: MATCH_MODE_ALL.to_string(),
            priority: 10,
            enabled: true,
        },
        CreateTaggingRuleInput {
            name: "GitHub notifications".to_string(),
            tag_id: tag_id(tag_ids, DemoTag::GitHub)?,
            criteria: vec![criterion(FIELD_SUBJECT, OPERATOR_STARTS_WITH, GITHUB_SUBJECT_PREFIX)],
            match_mode: MATCH_MODE_ALL.to_string(),
            priority: 20,
            enabled: true,
        },
        CreateTaggingRuleInput {
            name: "Receipts and orders".to_string(),
            tag_id: tag_id(tag_ids, DemoTag::Receipts)?,
            criteria: vec![
                criterion(FIELD_FROM, OPERATOR_CONTAINS, ORDERS_SENDER_MARKER),
                criterion(FIELD_SUBJECT, OPERATOR_CONTAINS, RECEIPT_SUBJECT_MARKER),
            ],
            match_mode: MATCH_MODE_ANY.to_string(),
            priority: 30,
            enabled: true,
        },
        CreateTaggingRuleInput {
            name: "Calendar invites".to_string(),
            tag_id: tag_id(tag_ids, DemoTag::Calendar)?,
            criteria: vec![criterion(FIELD_SUBJECT, OPERATOR_CONTAINS, CALENDAR_SUBJECT_MARKER)],
            match_mode: MATCH_MODE_ALL.to_string(),
            priority: 40,
            enabled: true,
        },
        CreateTaggingRuleInput {
            name: "People".to_string(),
            tag_id: tag_id(tag_ids, DemoTag::Personal)?,
            criteria: vec![criterion(FIELD_FROM, OPERATOR_WILDCARD, PERSONAL_SENDER_PATTERN)],
            match_mode: MATCH_MODE_ALL.to_string(),
            priority: 50,
            enabled: true,
        },
    ];
    Ok(rules)
}

fn create_demo_tagging_rules(
    db: &DatabaseService,
    tag_ids: &HashMap<DemoTag, i64>,
) -> Result<Vec<TaggingRule>> {
    let mut created_rules: Vec<TaggingRule> = Vec::new();
    for input in demo_tagging_rule_inputs(tag_ids)? {
        let rule = db
            .create_tagging_rule(&input)
            .with_context(|| format!("Failed to create demo tagging rule '{}'", input.name))?;
        created_rules.push(rule);
    }
    Ok(created_rules)
}

/// Apply every matching rule to one email using the same evaluation and
/// `applied_by` convention as the `run_tagging_rules` command, so the
/// startup rule run neither duplicates nor strips the seeded tags.
fn apply_rules_to_email(db: &DatabaseService, email: &EmailSummary, rules: &[TaggingRule]) -> Result<usize> {
    let email_id = email
        .id
        .context("Demo email must have a database id before tags are applied")?;

    let mut applied: usize = 0;
    for rule in rules {
        if evaluate_email_against_rule(email, rule) {
            let applied_by = format!("{}{}", RULE_APPLIED_BY_PREFIX, rule.id);
            db.add_tag_to_email(email_id, rule.tag_id, &applied_by)
                .with_context(|| format!("Failed to tag demo email {} via rule '{}'", email_id, rule.name))?;
            applied += 1;
        }
    }
    Ok(applied)
}

// ============ Feeds ============

fn tag_criterion(tag_id: i64) -> FilterCriterion {
    FilterCriterion {
        field: FIELD_TAG.to_string(),
        operator: OPERATOR_EQUALS.to_string(),
        // `build_criteria_where_clause` reads the tag criterion value as an integer tag id.
        value: json!(tag_id),
        negate: false,
    }
}

/// (feed name, tag the feed shows) in column order.
const DEMO_FEEDS: [(&str, DemoTag); 4] = [
    ("Newsletters", DemoTag::Newsletters),
    ("Receipts", DemoTag::Receipts),
    ("GitHub", DemoTag::GitHub),
    ("People", DemoTag::Personal),
];

fn create_demo_feeds(db: &DatabaseService, tag_ids: &HashMap<DemoTag, i64>) -> Result<usize> {
    for (column_order, (feed_name, tag)) in DEMO_FEEDS.iter().enumerate() {
        let input = CreateFeedInput {
            name: (*feed_name).to_string(),
            account_scope: None,
            criteria: vec![tag_criterion(tag_id(tag_ids, *tag)?)],
            match_mode: MATCH_MODE_ALL.to_string(),
            column_order: column_order as i32,
            column_width: DEFAULT_FEED_COLUMN_WIDTH,
        };
        db.create_feed(&input)
            .with_context(|| format!("Failed to create demo feed '{}'", feed_name))?;
    }
    Ok(DEMO_FEEDS.len())
}

// ============ Emails ============

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemoEmailKind {
    Newsletter,
    Receipt,
    GitHubNotification,
    CalendarInvite,
    Personal,
}

struct DemoEmailTemplate {
    account: DemoAccount,
    kind: DemoEmailKind,
    from_name: &'static str,
    from_address: &'static str,
    subject: &'static str,
    snippet: &'static str,
    hours_ago: u32,
    is_read: bool,
    has_attachments: bool,
}

/// UIDs are unique per (account, folder); using the template index keeps them
/// unique across both accounts too, which keeps synthetic ids unambiguous.
fn demo_uid_for(template_index: usize) -> u32 {
    (template_index + 1) as u32
}

impl DemoEmailTemplate {
    fn message_id(&self, uid: u32) -> String {
        format!("{}-{}-{}", DEMO_MESSAGE_ID_PREFIX, self.account.slug(), uid)
    }

    fn date(&self, uid: u32, now: i64) -> i64 {
        let minute_jitter = (uid as i64 * MINUTE_JITTER_STEP) % SECONDS_PER_MINUTE;
        now - (self.hours_ago as i64 * SECONDS_PER_HOUR) - (minute_jitter * SECONDS_PER_MINUTE)
    }

    fn to_summary(&self, uid: u32, now: i64) -> EmailSummary {
        EmailSummary {
            id: None,
            uid,
            message_id: Some(self.message_id(uid)),
            subject: self.subject.to_string(),
            from_address: self.from_address.to_string(),
            from_name: Some(self.from_name.to_string()),
            date: self.date(uid, now),
            snippet: Some(self.snippet.to_string()),
            has_attachments: self.has_attachments,
            is_read: self.is_read,
            account_email: Some(self.account.email().to_string()),
        }
    }

    fn html_body(&self) -> String {
        let inner = match self.kind {
            DemoEmailKind::Newsletter => newsletter_html(self),
            DemoEmailKind::Receipt => receipt_html(self),
            DemoEmailKind::GitHubNotification => github_html(self),
            DemoEmailKind::CalendarInvite => calendar_html(self),
            DemoEmailKind::Personal => personal_html(self),
        };
        wrap_html(self.subject, &inner)
    }

    fn text_body(&self) -> String {
        format!(
            "{}\n\n{}\n\n{}\n\nView online: {}\n",
            self.subject,
            self.snippet,
            body_paragraph(self.kind),
            EXAMPLE_LINK,
        )
    }
}

const EXAMPLE_LINK: &str = "https://example.com/demo";

const HTML_ESCAPES: [(char, &str); 4] = [('&', "&amp;"), ('<', "&lt;"), ('>', "&gt;"), ('"', "&quot;")];

fn escape_html(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for character in raw.chars() {
        match HTML_ESCAPES.iter().find(|(needle, _)| *needle == character) {
            Some((_, replacement)) => escaped.push_str(replacement),
            None => escaped.push(character),
        }
    }
    escaped
}

fn wrap_html(title: &str, inner: &str) -> String {
    format!(
        "<div style=\"font-family: -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif; \
         max-width: 640px; margin: 0 auto; padding: 24px; color: #1f2937; line-height: 1.5;\">\
         <h1 style=\"font-size: 20px; margin: 0 0 16px 0;\">{}</h1>{}\
         <p style=\"font-size: 12px; color: #6b7280; margin-top: 32px;\">\
         This message is synthetic demo content generated by Curier's demo mode.</p></div>",
        escape_html(title),
        inner
    )
}

fn body_paragraph(kind: DemoEmailKind) -> &'static str {
    match kind {
        DemoEmailKind::Newsletter => {
            "This week we look at what changed, why it matters, and what to try next. \
             Read on for the highlights, then dig into the full issue when you have a minute."
        }
        DemoEmailKind::Receipt => {
            "Thanks for your order. Below is a summary of the charges. Keep this receipt for your records; \
             no action is needed on your part."
        }
        DemoEmailKind::GitHubNotification => {
            "You are receiving this because you are watching the repository. \
             Reply to this email directly or view it on the web to join the conversation."
        }
        DemoEmailKind::CalendarInvite => {
            "You have been invited to this event. Let the organizer know whether you can make it by responding below."
        }
        DemoEmailKind::Personal => {
            "Let me know what you think when you get a chance. No rush, but I'd love to hear back before the weekend."
        }
    }
}

fn newsletter_html(template: &DemoEmailTemplate) -> String {
    format!(
        "<p style=\"font-size: 15px;\">{}</p>\
         <h2 style=\"font-size: 16px; margin: 24px 0 8px 0;\">In this issue</h2>\
         <ul style=\"padding-left: 20px;\">\
           <li>A deep dive into the topic everyone was arguing about this week</li>\
           <li>Three tools worth trying, and one you can safely skip</li>\
           <li>Reader questions, answered</li>\
         </ul>\
         <p>{}</p>\
         <p style=\"margin: 24px 0;\"><a href=\"{}/issue\" style=\"display: inline-block; background: #0ea5e9; \
         color: #ffffff; padding: 10px 18px; border-radius: 6px; text-decoration: none; font-weight: 600;\">\
         Read the full issue</a></p>\
         <p style=\"font-size: 12px; color: #6b7280;\">You are receiving this because you subscribed with {}. \
         <a href=\"{}/unsubscribe\" style=\"color: #6b7280;\">Unsubscribe</a></p>",
        escape_html(template.snippet),
        body_paragraph(template.kind),
        EXAMPLE_LINK,
        escape_html(template.account.email()),
        EXAMPLE_LINK,
    )
}

/// Deterministic line items for receipts, picked by subject length so each
/// receipt shows a different but stable combination.
const RECEIPT_LINE_ITEMS: [(&str, &str); 6] = [
    ("USB-C charging cable (2 m)", "$12.99"),
    ("Mechanical keyboard keycap set", "$34.00"),
    ("Pro plan, monthly", "$24.00"),
    ("Paperback: Working Effectively with Legacy Code", "$41.50"),
    ("Pour-over coffee filters (100 ct)", "$8.75"),
    ("Desk lamp, warm white", "$29.95"),
];

fn receipt_html(template: &DemoEmailTemplate) -> String {
    let first_item_index = template.subject.len() % RECEIPT_LINE_ITEMS.len();
    let second_item_index = (first_item_index + 2) % RECEIPT_LINE_ITEMS.len();
    let rows: String = [first_item_index, second_item_index]
        .iter()
        .map(|&index| {
            let (description, amount) = RECEIPT_LINE_ITEMS[index];
            format!(
                "<tr><td style=\"padding: 8px; border-bottom: 1px solid #e5e7eb;\">{}</td>\
                 <td style=\"padding: 8px; border-bottom: 1px solid #e5e7eb; text-align: right;\">{}</td></tr>",
                description, amount
            )
        })
        .collect();

    format!(
        "<p>{}</p>\
         <table style=\"width: 100%; border-collapse: collapse; margin: 16px 0;\">\
           <thead><tr>\
             <th style=\"text-align: left; padding: 8px; border-bottom: 2px solid #d1d5db;\">Item</th>\
             <th style=\"text-align: right; padding: 8px; border-bottom: 2px solid #d1d5db;\">Amount</th>\
           </tr></thead>\
           <tbody>{}</tbody>\
         </table>\
         <p>{}</p>\
         <p><a href=\"{}/orders\" style=\"color: #2563eb;\">View order details</a></p>",
        escape_html(template.snippet),
        rows,
        body_paragraph(template.kind),
        EXAMPLE_LINK,
    )
}

fn github_html(template: &DemoEmailTemplate) -> String {
    format!(
        "<p><strong>{}</strong> commented:</p>\
         <blockquote style=\"border-left: 4px solid #d1d5db; margin: 16px 0; padding: 4px 16px; color: #374151;\">\
         {}</blockquote>\
         <p><a href=\"{}/curier/issues\" style=\"color: #2563eb;\">View it on the web</a></p>\
         <p style=\"font-size: 12px; color: #6b7280;\">{}</p>",
        escape_html(template.from_name),
        escape_html(template.snippet),
        EXAMPLE_LINK,
        body_paragraph(template.kind),
    )
}

fn calendar_html(template: &DemoEmailTemplate) -> String {
    format!(
        "<p>{}</p>\
         <table style=\"border-collapse: collapse; margin: 16px 0;\">\
           <tr><td style=\"padding: 4px 12px 4px 0; color: #6b7280;\">Organizer</td><td style=\"padding: 4px 0;\">{}</td></tr>\
           <tr><td style=\"padding: 4px 12px 4px 0; color: #6b7280;\">Where</td><td style=\"padding: 4px 0;\">Video call (link in invite)</td></tr>\
           <tr><td style=\"padding: 4px 12px 4px 0; color: #6b7280;\">Guests</td><td style=\"padding: 4px 0;\">{}</td></tr>\
         </table>\
         <p>{}</p>\
         <p style=\"margin: 24px 0;\">\
           <a href=\"{}/rsvp/yes\" style=\"display: inline-block; background: #16a34a; color: #ffffff; padding: 8px 16px; border-radius: 6px; text-decoration: none; margin-right: 8px;\">Yes</a>\
           <a href=\"{}/rsvp/maybe\" style=\"display: inline-block; background: #e5e7eb; color: #111827; padding: 8px 16px; border-radius: 6px; text-decoration: none; margin-right: 8px;\">Maybe</a>\
           <a href=\"{}/rsvp/no\" style=\"display: inline-block; background: #e5e7eb; color: #111827; padding: 8px 16px; border-radius: 6px; text-decoration: none;\">No</a>\
         </p>",
        escape_html(template.snippet),
        escape_html(template.from_name),
        escape_html(template.account.email()),
        body_paragraph(template.kind),
        EXAMPLE_LINK,
        EXAMPLE_LINK,
        EXAMPLE_LINK,
    )
}

fn personal_html(template: &DemoEmailTemplate) -> String {
    let first_name = template.from_name.split_whitespace().next().unwrap_or(template.from_name);
    format!(
        "<p>Hey,</p><p>{}</p><p>{}</p><p>Talk soon,<br>{}</p>",
        escape_html(template.snippet),
        body_paragraph(template.kind),
        escape_html(first_name),
    )
}

/// The full deterministic inbox. Offsets are in hours before "now" and
/// increase within each account so the newest mail appears first.
const DEMO_EMAILS: &[DemoEmailTemplate] = &[
    // ---------------- dakota@example.com ----------------
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Maya Chen",
        from_address: "maya.chen@example.net",
        subject: "Trail run Saturday?",
        snippet: "Forecast finally looks clear. Thinking the ridge loop at 7am before it gets hot. I can drive if you bring the coffee.",
        hours_ago: 1,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "The Pragmatic Engineer",
        from_address: "newsletter@pragmatic.example.com",
        subject: "The Pulse: what the latest round of platform layoffs tells us",
        snippet: "Three companies restructured their platform teams this month. The pattern is clearer than the press releases suggest.",
        hours_ago: 3,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Northwind Outfitters",
        from_address: "orders@shop.example.net",
        subject: "Your order has shipped",
        snippet: "Good news: order #48213 left our warehouse this morning. Estimated delivery is Thursday between 9am and 6pm.",
        hours_ago: 5,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #12: Feed columns lose scroll position after sync",
        snippet: "Reproduced on macOS 15. The column resets to the top whenever the background sync writes new headers to the cache.",
        hours_ago: 7,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "TLDR",
        from_address: "newsletter@tldr.example.net",
        subject: "TLDR: new Rust async runtime benchmarks, WebGPU in Safari, a 3-line CSS trick",
        snippet: "Benchmarks for the new runtime landed overnight and the numbers are wild. Plus: WebGPU ships in Safari by default.",
        hours_ago: 9,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Jordan Ellis",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Board game night @ Fri 7pm",
        snippet: "Jordan Ellis has invited you to Board game night. Bring snacks; we are finally trying the long one everyone talks about.",
        hours_ago: 12,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Lumen Billing",
        from_address: "receipts@billing.example.com",
        subject: "Receipt from Lumen Cloud for $24.00",
        snippet: "Your monthly Pro plan payment of $24.00 was processed successfully. A PDF copy of this receipt is attached.",
        hours_ago: 15,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Sam Okafor",
        from_address: "sam.okafor@example.net",
        subject: "Re: that bread recipe",
        snippet: "Tried the overnight version and it worked way better. Attached a photo of the crumb so you can judge. 75% hydration next time?",
        hours_ago: 19,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Rust Weekly",
        from_address: "newsletter@rustweekly.example.org",
        subject: "Rust Weekly #512: async closures stabilize, cargo script, and a borrow checker retrospective",
        snippet: "A big week: async closures hit stable, cargo script gets an RFC update, and a ten-year retrospective on NLL.",
        hours_ago: 24,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Northwind Outfitters",
        from_address: "orders@shop.example.net",
        subject: "Order confirmed: #48213",
        snippet: "Thanks for your order! We will email you again once it ships. You can track progress any time from your account page.",
        hours_ago: 28,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #15: Add keyboard shortcuts for feed navigation",
        snippet: "Opened a PR that wires j/k and Enter in feed columns. It reuses the existing selection store so there is no new state.",
        hours_ago: 31,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Morning Brew",
        from_address: "newsletter@morningbrew.example.com",
        subject: "Markets rally, a chip shortage sequel, and the great office-return standoff",
        snippet: "Good morning. Stocks had their best day since spring, chipmakers are warning about supply again, and the RTO fight continues.",
        hours_ago: 33,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Aunt Rosa",
        from_address: "rosa.delgado@example.net",
        subject: "Thanksgiving plans",
        snippet: "Are you still able to come up on the Wednesday? Your uncle wants to know how many pies to bake, which is his way of saying hello.",
        hours_ago: 40,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Hacker Newsletter",
        from_address: "newsletter@hackernewsletter.example.net",
        subject: "Hacker Newsletter #730",
        snippet: "The best of the week: a tiny database engine in 1,000 lines, why your terminal is slow, and the economics of open source.",
        hours_ago: 46,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Maya Chen",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Dentist appointment @ Tue 2:30pm",
        snippet: "Reminder block for the cleaning appointment. Leave 20 minutes early; parking by the clinic has been a nightmare lately.",
        hours_ago: 52,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Brightline Books",
        from_address: "orders@books.example.org",
        subject: "Your order has shipped: Working Effectively with Legacy Code",
        snippet: "Your paperback is on the way and should arrive in 3-5 business days. Tracking details are available on your orders page.",
        hours_ago: 58,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #9: Dark mode contrast on tag chips is too low",
        snippet: "The cyan tag on the dark background fails WCAG AA. Suggest bumping the text to the 100 shade or adding a subtle border.",
        hours_ago: 63,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "The Pragmatic Engineer",
        from_address: "newsletter@pragmatic.example.com",
        subject: "How staff engineers actually spend their week",
        snippet: "We surveyed 400 staff-plus engineers about their calendars. Less code than you'd guess, far more writing than anyone admits.",
        hours_ago: 70,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Devon Park",
        from_address: "devon.park@example.net",
        subject: "Concert tickets",
        snippet: "Presale opens Thursday at 10. I have the code, you have the faster internet. Should we try for the balcony or the floor?",
        hours_ago: 75,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Lumen Billing",
        from_address: "receipts@billing.example.com",
        subject: "Receipt from Lumen Cloud for $12.99",
        snippet: "Your add-on storage payment of $12.99 was processed. This charge will appear on your statement as LUMEN CLOUD.",
        hours_ago: 82,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "TLDR",
        from_address: "newsletter@tldr.example.net",
        subject: "TLDR: the SQLite release everyone is upgrading for, plus a new terminal emulator",
        snippet: "SQLite's latest release makes concurrent writes less painful, and a Rust-based terminal emulator hits 1.0 after three years.",
        hours_ago: 90,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Sam Okafor",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Sunday brunch @ Sun 11am",
        snippet: "Sam Okafor has invited you to Sunday brunch at the place with the good biscuits. Reply so we can get a table big enough.",
        hours_ago: 97,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Rust Weekly",
        from_address: "newsletter@rustweekly.example.org",
        subject: "Rust Weekly #511: embedded HALs, a new linker, and the state of GUI crates",
        snippet: "Embedded Rust keeps growing up: three HAL crates reach 1.0, and a survey of GUI toolkits finds things are... getting better.",
        hours_ago: 104,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Maya Chen",
        from_address: "maya.chen@example.net",
        subject: "Photos from the lake",
        snippet: "Finally went through the camera roll. The one of the loons at sunset is going on the wall. Full album link inside.",
        hours_ago: 110,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Northwind Outfitters",
        from_address: "orders@shop.example.net",
        subject: "Your return has been processed",
        snippet: "We received your return for order #47990 and issued a refund of $29.95 to your original payment method.",
        hours_ago: 118,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #7: Support Outlook shared mailboxes",
        snippet: "Shared mailboxes show up in Graph under a different endpoint. Happy to take this on if the maintainers are open to it.",
        hours_ago: 125,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Morning Brew",
        from_address: "newsletter@morningbrew.example.com",
        subject: "The streaming bundle wars, a record IPO week, and robot baristas",
        snippet: "Good morning. Two streaming services are merging their bundles, IPOs are back, and your next latte might be made by a robot.",
        hours_ago: 130,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Devon Park",
        from_address: "devon.park@example.net",
        subject: "Book club: next pick",
        snippet: "Votes are in and the sci-fi one won by a landslide. First meeting is in three weeks, which is plenty if you skip the appendices.",
        hours_ago: 140,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Hacker Newsletter",
        from_address: "newsletter@hackernewsletter.example.net",
        subject: "Hacker Newsletter #729",
        snippet: "This week: a tour of a 1980s mainframe still in production, local-first software done right, and the physics of coffee.",
        hours_ago: 150,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Devon Park",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Climbing gym @ Wed 6pm",
        snippet: "Devon Park has invited you to Climbing gym. New bouldering set went up Monday and apparently the yellow route is brutal.",
        hours_ago: 160,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Brightline Books",
        from_address: "orders@books.example.org",
        subject: "Receipt for your Brightline Books order",
        snippet: "Thanks for shopping with us. Your order total was $41.50 including shipping. Your receipt is attached as a PDF.",
        hours_ago: 170,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "The Pragmatic Engineer",
        from_address: "newsletter@pragmatic.example.com",
        subject: "Inside a two-person startup that serves ten million users",
        snippet: "No ops team, no on-call rotation, and a bill under $2k a month. Here is how the architecture holds up and where it creaks.",
        hours_ago: 180,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Personal,
        from_name: "Aunt Rosa",
        from_address: "rosa.delgado@example.net",
        subject: "Your grandfather's photos",
        snippet: "I scanned the whole shoebox over the weekend. There is one of him at the shipyard in 1962 you have to see. Sending the album.",
        hours_ago: 195,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "TLDR",
        from_address: "newsletter@tldr.example.net",
        subject: "TLDR: the open-source email client wave, and why desktop apps are back",
        snippet: "A handful of new desktop mail clients launched this month. We look at what they share and what the big providers are missing.",
        hours_ago: 210,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Receipt,
        from_name: "Northwind Outfitters",
        from_address: "orders@shop.example.net",
        subject: "Order confirmed: #47990",
        snippet: "Thanks for your order! A desk lamp (warm white) and USB-C cable are being packed now. We will let you know when they ship.",
        hours_ago: 230,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Primary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Rust Weekly",
        from_address: "newsletter@rustweekly.example.org",
        subject: "Rust Weekly #510: Tauri 2 in production, const generics tips, crate of the week",
        snippet: "Case studies from three teams shipping Tauri 2 apps, a practical guide to const generics, and a crate you did not know you needed.",
        hours_ago: 250,
        is_read: true,
        has_attachments: false,
    },
    // ---------------- work@example.org ----------------
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #16: Cache feed query results per page",
        snippet: "Review requested. This memoizes feed pages keyed by criteria hash and invalidates on sync. Numbers in the PR description.",
        hours_ago: 1,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Hannah Lindqvist",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Sprint planning @ Mon 10am",
        snippet: "Hannah Lindqvist has invited you to Sprint planning. Please groom your tickets beforehand so we can keep this to an hour.",
        hours_ago: 2,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Hannah Lindqvist",
        from_address: "hannah.lindqvist@example.net",
        subject: "Quick sanity check on the migration plan",
        snippet: "Before I send this to the wider team: does the rollback step in section 3 actually work if the new column is already populated?",
        hours_ago: 4,
        is_read: false,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #14: Attachment download fails for filenames with unicode",
        snippet: "Filenames with accented characters are saved with replacement characters. Looks like a lossy conversion in the save path.",
        hours_ago: 6,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "The Pragmatic Engineer",
        from_address: "newsletter@pragmatic.example.com",
        subject: "Engineering org design: the platform team debate, revisited",
        snippet: "Platform teams are being restructured everywhere. We talked to leaders at six companies about what actually went wrong.",
        hours_ago: 8,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Tomás Rivera",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Design review: feed columns @ Tue 3pm",
        snippet: "Tomás Rivera has invited you to Design review: feed columns. Figma link in the description; comments welcome beforehand.",
        hours_ago: 11,
        is_read: false,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #12: Feed columns lose scroll position after sync",
        snippet: "Narrowed it down: the list remounts because the feed id changes type between number and string across the IPC boundary.",
        hours_ago: 14,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Receipt,
        from_name: "Lumen Billing",
        from_address: "receipts@billing.example.com",
        subject: "Receipt from Lumen Cloud for $240.00",
        snippet: "Your team plan payment of $240.00 for 10 seats was processed. An itemized invoice is attached for your expense report.",
        hours_ago: 18,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Tomás Rivera",
        from_address: "tomas.rivera@example.net",
        subject: "Column width mockups",
        snippet: "Attached three variants for the feed column widths. I lean toward the 4/4/4 split but the 6/3/3 one grows on you.",
        hours_ago: 21,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Ana Souza",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #15: Add keyboard shortcuts for feed navigation",
        snippet: "Approved with one nit: the shortcut list in the README is missing Escape. Nice work keeping it in the selection store.",
        hours_ago: 25,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Hannah Lindqvist",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: 1:1 Hannah / you @ Thu 1pm",
        snippet: "Recurring 1:1. Agenda doc is linked; add anything you want to cover and I will do the same before Thursday.",
        hours_ago: 29,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Rust Weekly",
        from_address: "newsletter@rustweekly.example.org",
        subject: "Rust Weekly #512: async closures stabilize, cargo script, and a borrow checker retrospective",
        snippet: "A big week: async closures hit stable, cargo script gets an RFC update, and a ten-year retrospective on NLL.",
        hours_ago: 34,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Ana Souza",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #13: Sync status indicator stuck on 'syncing' after network drop",
        snippet: "If the network drops mid-sync the status never resets. Probably need a timeout or a reset in the error path of sync_now.",
        hours_ago: 38,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Ana Souza",
        from_address: "ana.souza@example.net",
        subject: "Interview panel next week",
        snippet: "Can you take the systems design round on Wednesday? The candidate's background is mostly backend so it should be a good fit.",
        hours_ago: 43,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Ana Souza",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Systems design interview @ Wed 11am",
        snippet: "Ana Souza has invited you to Systems design interview. The rubric and the candidate packet are attached to the event.",
        hours_ago: 47,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #11: Migrate settings store to encrypted JSON",
        snippet: "Merged. Thanks for the thorough review on the key derivation; the HKDF salt is now a named constant as suggested.",
        hours_ago: 55,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "TLDR",
        from_address: "newsletter@tldr.example.net",
        subject: "TLDR: passkeys everywhere, a Postgres extension for vectors, and the end of a JS framework",
        snippet: "Passkey adoption crossed a milestone this week, a vector extension for Postgres hit 1.0, and a beloved framework is sunsetting.",
        hours_ago: 60,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Tomás Rivera",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Retro @ Fri 4pm",
        snippet: "Tomás Rivera has invited you to Retro. Board is open now; drop your sticky notes in before Friday so we can cluster them.",
        hours_ago: 66,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #10: Tagging rules should support 'to' field",
        snippet: "Feature request: filter on recipient so shared aliases can be routed to a feed. The schema already stores the field.",
        hours_ago: 72,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Hannah Lindqvist",
        from_address: "hannah.lindqvist@example.net",
        subject: "Re: Q4 roadmap draft",
        snippet: "Looks good overall. I would move the Outlook work up a quarter; two of the enterprise prospects asked about it again this week.",
        hours_ago: 78,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Receipt,
        from_name: "Northwind Outfitters",
        from_address: "orders@shop.example.net",
        subject: "Your order has shipped",
        snippet: "Order #48102 (mechanical keyboard keycap set) is on its way to the office. Estimated delivery is Monday.",
        hours_ago: 84,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Ana Souza",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #8: Harden HTML sanitizer allowlist",
        snippet: "Added tests for the font and center tags and dropped the target attribute on anchors. Ready for another look.",
        hours_ago: 90,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Hannah Lindqvist",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: All-hands @ Thu 9am",
        snippet: "Hannah Lindqvist has invited you to All-hands. Quarterly numbers, two new hires, and a demo of the feeds feature.",
        hours_ago: 96,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Hacker Newsletter",
        from_address: "newsletter@hackernewsletter.example.net",
        subject: "Hacker Newsletter #730",
        snippet: "The best of the week: a tiny database engine in 1,000 lines, why your terminal is slow, and the economics of open source.",
        hours_ago: 102,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #6: Startup is slow with more than 5 accounts",
        snippet: "Profiled it: each account's credential load hits the keychain serially. Loading them in parallel cuts startup to under a second.",
        hours_ago: 110,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Tomás Rivera",
        from_address: "tomas.rivera@example.net",
        subject: "Lunch Thursday?",
        snippet: "The ramen place by the office finally reopened. Want to go after the all-hands? I will book a table if you are in.",
        hours_ago: 118,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Receipt,
        from_name: "Lumen Billing",
        from_address: "receipts@billing.example.com",
        subject: "Receipt from Lumen Cloud for $48.00",
        snippet: "Your payment of $48.00 for two additional seats was processed. This brings your plan to 12 seats total.",
        hours_ago: 126,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Priya Raman",
        from_address: "notifications@github.example.com",
        subject: "[curier] PR #5: Add Outlook provider via Microsoft Graph",
        snippet: "Merged after two rounds of review. Thanks for sticking with the retry logic on token refresh; it was worth it.",
        hours_ago: 134,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Ana Souza",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Incident review: sync outage @ Tue 11am",
        snippet: "Ana Souza has invited you to Incident review: sync outage. Blameless; please read the timeline doc before we meet.",
        hours_ago: 142,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "Morning Brew",
        from_address: "newsletter@morningbrew.example.com",
        subject: "A four-day week experiment ends, and the results are in",
        snippet: "Good morning. The largest four-day-week trial to date just wrapped up, and most of the participating companies kept it.",
        hours_ago: 150,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Ana Souza",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #4: Memory grows unbounded with many cached bodies",
        snippet: "Confirmed the LRU eviction only runs on insert, so a long-lived session that only reads never evicts. Patch incoming.",
        hours_ago: 160,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Personal,
        from_name: "Ana Souza",
        from_address: "ana.souza@example.net",
        subject: "Conference talk proposal",
        snippet: "Would you co-present the local-first email talk? I drafted an abstract; it is attached. Deadline for submissions is the 30th.",
        hours_ago: 172,
        is_read: true,
        has_attachments: true,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::GitHubNotification,
        from_name: "Lucas Moreau",
        from_address: "notifications@github.example.com",
        subject: "[curier] Issue #3: Add a demo mode for screenshots",
        snippet: "It would help contributors without Gmail if the app could boot with synthetic data. An env var toggle seems simplest.",
        hours_ago: 185,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::Newsletter,
        from_name: "The Pragmatic Engineer",
        from_address: "newsletter@pragmatic.example.com",
        subject: "The hidden cost of on-call, in numbers",
        snippet: "We collected pager data from 40 teams. The median engineer loses a full working day per month to interrupts alone.",
        hours_ago: 200,
        is_read: true,
        has_attachments: false,
    },
    DemoEmailTemplate {
        account: DemoAccount::Secondary,
        kind: DemoEmailKind::CalendarInvite,
        from_name: "Tomás Rivera",
        from_address: "calendar-invites@mail.example.org",
        subject: "Invitation: Roadmap offsite @ Fri 9am",
        snippet: "Tomás Rivera has invited you to Roadmap offsite. Full day; lunch provided. Bring the Q4 draft and an open mind.",
        hours_ago: 220,
        is_read: true,
        has_attachments: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::feeds::{build_criteria_where_clause, execute_feed_query};
    use std::sync::Mutex;

    /// Minimum emails per account so that the inbox's cache-first path is
    /// satisfied for the default page size (20) with room to spare.
    const MINIMUM_EMAILS_PER_ACCOUNT: usize = 25;
    const MINIMUM_EMAILS_PER_FEED: u32 = 3;
    const ALLOWED_SENDER_DOMAINS: [&str; 3] = ["example.com", "example.org", "example.net"];
    const FEED_PAGE_SIZE: u32 = 100;
    const LARGE_PAGE_SIZE: usize = 10_000;

    /// Serializes tests that mutate the process environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn seeded_db() -> (DatabaseService, DemoSeedSummary) {
        let db = DatabaseService::new_in_memory().expect("in-memory db");
        let summary = seed_demo_data(&db).expect("seed succeeds");
        (db, summary)
    }

    fn email_count_for_account(db: &DatabaseService, account_email: &str) -> usize {
        let (_, total) = db
            .get_cached_emails_by_folder_paginated(account_email, DEMO_FOLDER_ID, 0, LARGE_PAGE_SIZE)
            .expect("paginated query");
        total
    }

    fn sender_domain(address: &str) -> &str {
        address.rsplit('@').next().unwrap_or("")
    }

    fn is_allowed_domain(domain: &str) -> bool {
        ALLOWED_SENDER_DOMAINS
            .iter()
            .any(|allowed| domain == *allowed || domain.ends_with(&format!(".{}", allowed)))
    }

    #[test]
    fn seeding_an_empty_database_creates_expected_counts() {
        let (db, summary) = seeded_db();

        assert!(!summary.skipped_already_seeded);
        assert_eq!(summary.emails_inserted, DEMO_EMAILS.len());
        assert_eq!(summary.bodies_stored, DEMO_EMAILS.len());
        assert_eq!(summary.tags_created, DEMO_TAGS.len());
        assert_eq!(summary.feeds_created, DEMO_FEEDS.len());
        assert!(summary.tagging_rules_created > 0);
        assert!(summary.tags_applied > 0);

        for account in DEMO_ACCOUNTS {
            let count = email_count_for_account(&db, account.email());
            assert!(
                count >= MINIMUM_EMAILS_PER_ACCOUNT,
                "{} has only {} emails",
                account.email(),
                count
            );
        }

        assert_eq!(db.get_tags().expect("tags").len(), DEMO_TAGS.len());
        assert_eq!(db.get_feeds(None).expect("feeds").len(), DEMO_FEEDS.len());
        assert_eq!(db.get_tagging_rules().expect("rules").len(), summary.tagging_rules_created);

        let colors = db.get_account_colors().expect("colors");
        assert_eq!(colors.len(), DEMO_ACCOUNTS.len());
    }

    #[test]
    fn every_seeded_email_has_a_body_and_a_tag() {
        let (db, _) = seeded_db();

        let email_ids = db.get_all_email_ids().expect("email ids");
        assert_eq!(email_ids.len(), DEMO_EMAILS.len());

        for email_id in email_ids {
            let body = db
                .get_email_body_lru(email_id)
                .expect("body query")
                .unwrap_or_else(|| panic!("email {} has no body", email_id));
            let html = body.html_body.expect("html body");
            assert!(html.contains("<h1"), "html body should contain a heading");
            assert!(!html.contains("<img"), "demo bodies must not load images");
            assert!(body.text_body.is_some(), "text body should be set");

            // Every template kind is covered by a rule, so every email is tagged.
            let tags = db.get_email_tags(email_id).expect("tags query");
            assert!(!tags.is_empty(), "email {} has no tags", email_id);
        }
    }

    #[test]
    fn seeding_twice_is_a_no_op() {
        let (db, first) = seeded_db();
        let second = seed_demo_data(&db).expect("second seed succeeds");

        assert!(second.skipped_already_seeded);
        assert_eq!(second.emails_inserted, 0);

        let (email_count, body_count, _) = db.get_cache_stats().expect("stats");
        assert_eq!(email_count as usize, first.emails_inserted);
        assert_eq!(body_count as usize, first.bodies_stored);
        assert_eq!(db.get_tags().expect("tags").len(), DEMO_TAGS.len());
        assert_eq!(db.get_feeds(None).expect("feeds").len(), DEMO_FEEDS.len());
    }

    #[test]
    fn every_feed_returns_at_least_three_emails() {
        let (db, _) = seeded_db();

        for feed in db.get_feeds(None).expect("feeds") {
            let (emails, total_count) =
                execute_feed_query(&db, &feed, 1, FEED_PAGE_SIZE).expect("feed query");
            assert!(
                total_count >= MINIMUM_EMAILS_PER_FEED,
                "feed '{}' returned {} emails",
                feed.name,
                total_count
            );
            assert_eq!(emails.len() as u32, total_count);
        }
    }

    #[test]
    fn every_tagging_rule_criterion_is_understood_by_the_feed_sql_builder() {
        let (db, _) = seeded_db();

        for rule in db.get_tagging_rules().expect("rules") {
            let (where_clause, params) =
                build_criteria_where_clause(&rule.criteria, &rule.match_mode, &None);
            assert!(!where_clause.is_empty(), "rule '{}' produced no SQL", rule.name);
            assert_eq!(
                params.len(),
                rule.criteria.len(),
                "rule '{}' has a criterion the SQL builder dropped",
                rule.name
            );
        }
    }

    #[test]
    fn seeded_tags_match_what_the_rule_runner_would_apply() {
        let (db, _) = seeded_db();
        let rules = db.get_enabled_tagging_rules().expect("rules");

        for email_id in db.get_all_email_ids().expect("ids") {
            let email = db.get_email_by_id(email_id).expect("email").expect("exists");
            let expected_tag_ids: Vec<i64> = rules
                .iter()
                .filter(|rule| evaluate_email_against_rule(&email, rule))
                .map(|rule| rule.tag_id)
                .collect();
            let mut actual_tag_ids: Vec<i64> =
                db.get_email_tags(email_id).expect("tags").iter().map(|tag| tag.id).collect();
            actual_tag_ids.sort_unstable();
            let mut expected_sorted = expected_tag_ids;
            expected_sorted.sort_unstable();
            assert_eq!(actual_tag_ids, expected_sorted, "tags differ for '{}'", email.subject);
        }
    }

    #[test]
    fn no_sender_uses_a_non_example_domain() {
        for template in DEMO_EMAILS {
            let domain = sender_domain(template.from_address);
            assert!(
                is_allowed_domain(domain),
                "'{}' uses a non-example domain",
                template.from_address
            );
        }
    }

    #[test]
    fn uids_are_unique_per_account_and_folder() {
        let mut seen: std::collections::HashSet<(DemoAccount, u32)> = std::collections::HashSet::new();
        for (index, template) in DEMO_EMAILS.iter().enumerate() {
            assert!(seen.insert((template.account, demo_uid_for(index))));
        }
    }

    #[test]
    fn dates_are_deterministic_relative_to_now_and_in_the_past() {
        let now = 1_700_000_000_i64;
        for (index, template) in DEMO_EMAILS.iter().enumerate() {
            let uid = demo_uid_for(index);
            let first = template.to_summary(uid, now).date;
            let second = template.to_summary(uid, now).date;
            assert_eq!(first, second);
            assert!(first < now);
        }
    }

    #[test]
    fn snippets_are_within_the_expected_length_range() {
        const MIN_SNIPPET_LENGTH: usize = 80;
        const MAX_SNIPPET_LENGTH: usize = 140;
        for template in DEMO_EMAILS {
            let length = template.snippet.chars().count();
            assert!(
                (MIN_SNIPPET_LENGTH..=MAX_SNIPPET_LENGTH).contains(&length),
                "snippet for '{}' is {} chars",
                template.subject,
                length
            );
        }
    }

    #[test]
    fn demo_account_configs_carry_no_credentials() {
        let configs = demo_account_configs();
        assert_eq!(configs.len(), DEMO_ACCOUNTS.len());
        for (email, config) in configs {
            assert_eq!(config.email.as_deref(), Some(email.as_str()));
            assert!(config.client_id.is_empty());
            assert!(config.client_secret.is_empty());
            assert!(config.access_token.is_none());
            assert!(config.refresh_token.is_none());
        }
    }

    #[test]
    fn data_dir_for_mode_appends_demo_subdirectory_only_in_demo_mode() {
        let base = PathBuf::from("/tmp/curier-app-data");
        assert_eq!(data_dir_for_mode(&base, false), base);
        assert_eq!(data_dir_for_mode(&base, true), base.join(DEMO_DATA_DIR_NAME));
    }

    #[test]
    fn is_demo_mode_reflects_the_environment_variable() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let original = std::env::var(DEMO_DATA_ENV_VAR).ok();

        std::env::remove_var(DEMO_DATA_ENV_VAR);
        assert!(!is_demo_mode());

        std::env::set_var(DEMO_DATA_ENV_VAR, DEMO_DATA_ENV_ENABLED_VALUE);
        assert!(is_demo_mode());

        std::env::set_var(DEMO_DATA_ENV_VAR, "0");
        assert!(!is_demo_mode());

        match original {
            Some(value) => std::env::set_var(DEMO_DATA_ENV_VAR, value),
            None => std::env::remove_var(DEMO_DATA_ENV_VAR),
        }
    }
}
