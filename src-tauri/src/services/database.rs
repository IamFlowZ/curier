use crate::models::email::{EmailBody, EmailSummary};
use crate::models::feeds::{
    CreateFeedInput, CreateTaggingRuleInput, Feed, FilterCriterion, Tag, TaggingRule,
    UpdateFeedInput,
};
use anyhow::{Context, Result};
use log::{debug, info};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension};
use std::path::PathBuf;

/// Customizer that sets SQLite pragmas on every new connection from the pool.
/// Per-connection pragmas (cache_size, temp_store, mmap_size) are not inherited
/// by new connections, so they must be set via an on_acquire hook.
#[derive(Debug)]
struct SqlitePragmaCustomizer;

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error>
    for SqlitePragmaCustomizer
{
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "cache_size", -8000)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "mmap_size", 268435456)?;
        Ok(())
    }
}

/// Map a row from the emails table to an EmailSummary.
/// Expects columns: id, uid, message_id, subject, from_address, from_name,
///                   date, snippet, has_attachments, is_read, account_email
pub fn email_summary_from_row(row: &rusqlite::Row) -> rusqlite::Result<EmailSummary> {
    Ok(EmailSummary {
        id: Some(row.get(0)?),
        uid: row.get(1)?,
        message_id: row.get(2)?,
        subject: row.get(3)?,
        from_address: row.get(4)?,
        from_name: row.get(5)?,
        date: row.get(6)?,
        snippet: row.get(7)?,
        has_attachments: row.get(8)?,
        is_read: row.get(9)?,
        account_email: row.get(10)?,
    })
}

/// Current schema version. Increment when adding new migrations.
const CURRENT_SCHEMA_VERSION: i64 = 2;

pub struct DatabaseService {
    pub pool: Pool<SqliteConnectionManager>,
}

impl DatabaseService {
    /// Create a new database service with connection pool
    pub fn new(db_path: PathBuf) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder()
            .max_size(3) // Single-user desktop app only needs a few connections
            .connection_customizer(Box::new(SqlitePragmaCustomizer))
            .build(manager)
            .context("Failed to create connection pool")?;

        let service = Self { pool };
        service.run_migrations()?;

        Ok(service)
    }

    /// Run versioned migrations. Each migration runs in its own transaction for atomicity.
    /// Existing databases (pre-versioning) are detected and set to version 1.
    fn run_migrations(&self) -> Result<()> {
        let mut conn = self.pool.get()?;

        // Create version tracking table (idempotent)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 0)",
            [],
        )?;

        // Ensure there's a version row
        let row_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_version",
            [],
            |row| row.get(0),
        )?;

        if row_count == 0 {
            // Check if this is an existing database or a fresh one
            let has_emails_table: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='emails'",
                    [],
                    |row| row.get::<_, i64>(0).map(|count| count > 0),
                )
                .unwrap_or(false);

            if has_emails_table {
                // Existing database with all old migrations already applied
                info!("DATABASE: Existing database detected, setting schema version to {}", CURRENT_SCHEMA_VERSION);
                conn.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![CURRENT_SCHEMA_VERSION],
                )?;
                return Ok(());
            }

            // Fresh database - start at version 0
            conn.execute("INSERT INTO schema_version (version) VALUES (0)", [])?;
        }

        let current_version: i64 = conn.query_row(
            "SELECT version FROM schema_version LIMIT 1",
            [],
            |row| row.get(0),
        )?;

        if current_version >= CURRENT_SCHEMA_VERSION {
            debug!("DATABASE: Schema up to date (v{})", current_version);
            return Ok(());
        }

        info!("DATABASE: Migrating schema from v{} to v{}", current_version, CURRENT_SCHEMA_VERSION);

        for version in (current_version + 1)..=CURRENT_SCHEMA_VERSION {
            info!("DATABASE: Running migration v{}", version);
            let tx = conn.transaction().context("Failed to begin migration transaction")?;
            Self::apply_migration(&tx, version)?;
            tx.execute("UPDATE schema_version SET version = ?1", params![version])?;
            tx.commit().with_context(|| format!("Failed to commit migration v{}", version))?;
            info!("DATABASE: Migration v{} complete", version);
        }

        info!("DATABASE: Schema migration complete (v{})", CURRENT_SCHEMA_VERSION);
        Ok(())
    }

    /// Apply a single migration by version number.
    /// Version 1 creates the full current schema (consolidates all prior migrations).
    fn apply_migration(conn: &rusqlite::Connection, version: i64) -> Result<()> {
        match version {
            1 => Self::migration_v1_full_schema(conn)?,
            2 => Self::migration_v2_tag_icons_and_account_colors(conn)?,
            _ => anyhow::bail!("Unknown migration version: {}", version),
        }
        Ok(())
    }

    /// V1: Full current schema - all tables, indexes, and initial data.
    /// This consolidates the original 11 incremental migrations (0-10) into a single
    /// canonical schema definition for fresh databases.
    fn migration_v1_full_schema(conn: &rusqlite::Connection) -> Result<()> {
        // Core emails table (final form with multi-account UNIQUE constraint)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS emails (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                uid INTEGER NOT NULL,
                message_id TEXT,
                subject TEXT NOT NULL,
                from_address TEXT NOT NULL,
                from_name TEXT,
                date INTEGER NOT NULL,
                snippet TEXT,
                has_attachments BOOLEAN NOT NULL DEFAULT 0,
                is_read BOOLEAN NOT NULL DEFAULT 0,
                account_email TEXT,
                folder_id TEXT DEFAULT 'INBOX',
                UNIQUE(account_email, folder_id, uid)
            )",
            [],
        )?;

        conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_date ON emails(date DESC)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_account_folder_uid ON emails(account_email, folder_id, uid)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_account ON emails(account_email)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_emails_folder ON emails(folder_id)", [])?;

        // Email bodies with LRU tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS email_bodies (
                email_id INTEGER PRIMARY KEY,
                html_body TEXT,
                text_body TEXT,
                last_accessed INTEGER NOT NULL DEFAULT 0,
                body_size INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(email_id) REFERENCES emails(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_bodies_lru ON email_bodies(last_accessed DESC)", [])?;

        // Sync metadata
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sync_metadata (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_email TEXT NOT NULL,
                folder_id TEXT NOT NULL,
                last_sync_timestamp INTEGER NOT NULL,
                last_message_date INTEGER,
                message_count INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(account_email, folder_id)
            )",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_sync_metadata_account ON sync_metadata(account_email)", [])?;

        // Cache stats
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cache_stats (
                id INTEGER PRIMARY KEY,
                total_headers INTEGER DEFAULT 0,
                total_bodies INTEGER DEFAULT 0,
                total_body_size_bytes INTEGER DEFAULT 0,
                last_eviction_timestamp INTEGER,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO cache_stats (id, total_headers, total_bodies, total_body_size_bytes, updated_at)
             VALUES (1, 0, 0, 0, strftime('%s', 'now'))",
            [],
        )?;

        // Tags
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                color TEXT,
                created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
            )",
            [],
        )?;

        // Email-tag associations
        conn.execute(
            "CREATE TABLE IF NOT EXISTS email_tags (
                email_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                applied_by TEXT NOT NULL,
                applied_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                PRIMARY KEY (email_id, tag_id),
                FOREIGN KEY (email_id) REFERENCES emails(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            )",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_email_tags_email ON email_tags(email_id)", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_email_tags_tag ON email_tags(tag_id)", [])?;

        // Tagging rules
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tagging_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                tag_id INTEGER NOT NULL,
                criteria_json TEXT NOT NULL,
                match_mode TEXT NOT NULL DEFAULT 'all',
                priority INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            )",
            [],
        )?;

        // Feeds
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                account_scope TEXT,
                criteria_json TEXT NOT NULL,
                match_mode TEXT NOT NULL DEFAULT 'all',
                column_order INTEGER NOT NULL DEFAULT 0,
                column_width INTEGER DEFAULT 320,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_feeds_account ON feeds(account_scope)", [])?;

        // Rule execution tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS rule_executions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                rule_id INTEGER NOT NULL,
                executed_at INTEGER NOT NULL,
                emails_matched INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (rule_id) REFERENCES tagging_rules(id) ON DELETE CASCADE
            )",
            [],
        )?;

        Ok(())
    }

    /// V2: Add icon column to tags and account_colors table.
    fn migration_v2_tag_icons_and_account_colors(conn: &rusqlite::Connection) -> Result<()> {
        // Add icon column to tags table
        conn.execute("ALTER TABLE tags ADD COLUMN icon TEXT", [])?;

        // Create account_colors table for user-configured account colors
        conn.execute(
            "CREATE TABLE IF NOT EXISTS account_colors (
                account_email TEXT PRIMARY KEY,
                color TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    /// Delete all data associated with an account (emails, bodies, sync metadata, account color).
    /// Email tags and rule executions are cleaned up via ON DELETE CASCADE.
    pub fn delete_account_data(&self, account_email: &str) -> Result<()> {
        let conn = self.pool.get()?;

        // Delete emails (cascades to email_bodies, email_tags)
        conn.execute(
            "DELETE FROM emails WHERE account_email = ?1",
            params![account_email],
        )?;

        // Delete sync metadata
        conn.execute(
            "DELETE FROM sync_metadata WHERE account_email = ?1",
            params![account_email],
        )?;

        // Delete account color
        conn.execute(
            "DELETE FROM account_colors WHERE account_email = ?1",
            params![account_email],
        )?;

        info!("DATABASE: Deleted all data for account: {}", account_email);
        Ok(())
    }

    /// Insert or update email summary
    pub fn upsert_email(&self, email: &EmailSummary) -> Result<i64> {
        let conn = self.pool.get()?;

        conn.execute(
            "INSERT INTO emails (uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(uid) DO UPDATE SET
                message_id = excluded.message_id,
                subject = excluded.subject,
                from_address = excluded.from_address,
                from_name = excluded.from_name,
                date = excluded.date,
                snippet = excluded.snippet,
                has_attachments = excluded.has_attachments,
                is_read = excluded.is_read",
            params![
                email.uid,
                email.message_id,
                email.subject,
                email.from_address,
                email.from_name,
                email.date,
                email.snippet,
                email.has_attachments,
                email.is_read,
            ],
        )?;

        let email_id = conn.last_insert_rowid();
        Ok(email_id)
    }

    /// Get recent emails (most recent first)
    pub fn get_recent_emails(&self, limit: usize) -> Result<Vec<EmailSummary>> {
        let conn = self.pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
             FROM emails
             ORDER BY date DESC
             LIMIT ?1",
        )?;

        let emails = stmt
            .query_map(params![limit], email_summary_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(emails)
    }

    /// Store email body
    pub fn store_email_body(&self, email_id: i64, body: &EmailBody) -> Result<()> {
        let conn = self.pool.get()?;

        conn.execute(
            "INSERT INTO email_bodies (email_id, html_body, text_body)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(email_id) DO UPDATE SET
                html_body = excluded.html_body,
                text_body = excluded.text_body",
            params![email_id, body.html_body, body.text_body],
        )?;

        Ok(())
    }

    /// Get email body by email ID
    pub fn get_email_body(&self, email_id: i64) -> Result<Option<EmailBody>> {
        let conn = self.pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT email_id, html_body, text_body
             FROM email_bodies
             WHERE email_id = ?1",
        )?;

        let body = stmt
            .query_row(params![email_id], |row| {
                Ok(EmailBody {
                    email_id: row.get(0)?,
                    html_body: row.get(1)?,
                    text_body: row.get(2)?,
                })
            })
            .optional()?;

        Ok(body)
    }

    /// Mark email as read
    pub fn mark_as_read(&self, email_id: i64) -> Result<()> {
        let conn = self.pool.get()?;

        conn.execute(
            "UPDATE emails SET is_read = 1 WHERE id = ?1",
            params![email_id],
        )?;

        Ok(())
    }

    // ========== Cache-Aware Email Storage ==========

    /// Insert or update email with account and folder context
    pub fn upsert_email_with_context(
        &self,
        email: &EmailSummary,
        account_email: &str,
        folder_id: &str,
    ) -> Result<i64> {
        let conn = self.pool.get()?;

        // First, try to find existing email by account, folder, and uid
        let existing_id: Option<i64> = conn
            .query_row(
                "SELECT id FROM emails WHERE account_email = ?1 AND folder_id = ?2 AND uid = ?3",
                params![account_email, folder_id, email.uid],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(id) = existing_id {
            // Update existing email
            conn.execute(
                "UPDATE emails SET
                    message_id = ?1,
                    subject = ?2,
                    from_address = ?3,
                    from_name = ?4,
                    date = ?5,
                    snippet = ?6,
                    has_attachments = ?7,
                    is_read = ?8
                 WHERE id = ?9",
                params![
                    email.message_id,
                    email.subject,
                    email.from_address,
                    email.from_name,
                    email.date,
                    email.snippet,
                    email.has_attachments,
                    email.is_read,
                    id,
                ],
            )?;
            Ok(id)
        } else {
            // Insert new email
            conn.execute(
                "INSERT INTO emails (uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email, folder_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    email.uid,
                    email.message_id,
                    email.subject,
                    email.from_address,
                    email.from_name,
                    email.date,
                    email.snippet,
                    email.has_attachments,
                    email.is_read,
                    account_email,
                    folder_id,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }

    /// Get email by UID, account, and folder
    pub fn get_email_by_uid_and_account(
        &self,
        uid: u32,
        account_email: &str,
        folder_id: &str,
    ) -> Result<Option<EmailSummary>> {
        let conn = self.pool.get()?;

        let email = conn
            .query_row(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE uid = ?1 AND account_email = ?2 AND folder_id = ?3",
                params![uid, account_email, folder_id],
                email_summary_from_row,
            )
            .optional()?;

        Ok(email)
    }

    /// Get cached emails for a specific account and folder
    pub fn get_cached_emails_by_folder(
        &self,
        account_email: &str,
        folder_id: &str,
        limit: usize,
    ) -> Result<Vec<EmailSummary>> {
        let conn = self.pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
             FROM emails
             WHERE account_email = ?1 AND folder_id = ?2
             ORDER BY date DESC
             LIMIT ?3",
        )?;

        let emails = stmt
            .query_map(params![account_email, folder_id, limit], email_summary_from_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(emails)
    }

    /// Get cached emails for a specific account and folder with pagination
    pub fn get_cached_emails_by_folder_paginated(
        &self,
        account_email: &str,
        folder_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<EmailSummary>, usize)> {
        self.search_cached_emails_by_folder_paginated(account_email, folder_id, offset, limit, None)
    }

    /// Get cached emails for a specific folder with optional search filter
    pub fn search_cached_emails_by_folder_paginated(
        &self,
        account_email: &str,
        folder_id: &str,
        offset: usize,
        limit: usize,
        search_query: Option<&str>,
    ) -> Result<(Vec<EmailSummary>, usize)> {
        let conn = self.pool.get()?;

        let search_filter = search_query
            .filter(|q| !q.trim().is_empty())
            .map(|q| format!("%{}%", q.to_lowercase()));

        if let Some(ref pattern) = search_filter {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails WHERE account_email = ?1 AND folder_id = ?2
                 AND (LOWER(subject) LIKE ?3 OR LOWER(from_name) LIKE ?3 OR LOWER(from_address) LIKE ?3 OR LOWER(snippet) LIKE ?3)",
                params![account_email, folder_id, pattern],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE account_email = ?1 AND folder_id = ?2
                 AND (LOWER(subject) LIKE ?3 OR LOWER(from_name) LIKE ?3 OR LOWER(from_address) LIKE ?3 OR LOWER(snippet) LIKE ?3)
                 ORDER BY date DESC
                 LIMIT ?4 OFFSET ?5",
            )?;

            let emails = stmt
                .query_map(params![account_email, folder_id, pattern, limit, offset], email_summary_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok((emails, total_count))
        } else {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails WHERE account_email = ?1 AND folder_id = ?2",
                params![account_email, folder_id],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE account_email = ?1 AND folder_id = ?2
                 ORDER BY date DESC
                 LIMIT ?3 OFFSET ?4",
            )?;

            let emails = stmt
                .query_map(params![account_email, folder_id, limit, offset], email_summary_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok((emails, total_count))
        }
    }

    /// Get all cached emails with pagination
    pub fn get_all_cached_emails_paginated(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<EmailSummary>, usize)> {
        self.search_all_cached_emails_paginated(offset, limit, None)
    }

    /// Get all cached emails with optional search filter
    pub fn search_all_cached_emails_paginated(
        &self,
        offset: usize,
        limit: usize,
        search_query: Option<&str>,
    ) -> Result<(Vec<EmailSummary>, usize)> {
        let conn = self.pool.get()?;

        let search_filter = search_query
            .filter(|q| !q.trim().is_empty())
            .map(|q| format!("%{}%", q.to_lowercase()));

        if let Some(ref pattern) = search_filter {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails
                 WHERE LOWER(subject) LIKE ?1 OR LOWER(from_name) LIKE ?1 OR LOWER(from_address) LIKE ?1 OR LOWER(snippet) LIKE ?1",
                params![pattern],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE LOWER(subject) LIKE ?1 OR LOWER(from_name) LIKE ?1 OR LOWER(from_address) LIKE ?1 OR LOWER(snippet) LIKE ?1
                 ORDER BY date DESC
                 LIMIT ?2 OFFSET ?3",
            )?;

            let emails = stmt
                .query_map(params![pattern, limit, offset], email_summary_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok((emails, total_count))
        } else {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails",
                [],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 ORDER BY date DESC
                 LIMIT ?1 OFFSET ?2",
            )?;

            let emails = stmt
                .query_map(params![limit, offset], email_summary_from_row)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok((emails, total_count))
        }
    }

    /// Evict old headers to enforce limit per folder
    pub fn evict_old_headers(
        &self,
        account_email: &str,
        folder_id: &str,
        limit: usize,
    ) -> Result<()> {
        let conn = self.pool.get()?;

        // Delete emails beyond the limit (keeping newest)
        conn.execute(
            "DELETE FROM emails
             WHERE account_email = ?1 AND folder_id = ?2
             AND id NOT IN (
                 SELECT id FROM emails
                 WHERE account_email = ?1 AND folder_id = ?2
                 ORDER BY date DESC
                 LIMIT ?3
             )",
            params![account_email, folder_id, limit],
        )?;

        Ok(())
    }

    /// Get email ID by message_id (for body cache lookup)
    pub fn get_email_id_by_message_id(&self, message_id: &str) -> Result<Option<i64>> {
        let conn = self.pool.get()?;

        let email_id = conn
            .query_row(
                "SELECT id FROM emails WHERE message_id = ?1 LIMIT 1",
                params![message_id],
                |row| row.get(0),
            )
            .optional()?;

        Ok(email_id)
    }

    // ========== LRU Body Caching ==========

    /// Store email body with LRU tracking
    pub fn store_email_body_lru(&self, email_id: i64, body: &EmailBody, max_bodies: usize) -> Result<()> {
        let conn = self.pool.get()?;

        let body_size = body.html_body.as_ref().map(|s| s.len()).unwrap_or(0)
            + body.text_body.as_ref().map(|s| s.len()).unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO email_bodies (email_id, html_body, text_body, last_accessed, body_size)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(email_id) DO UPDATE SET
                html_body = excluded.html_body,
                text_body = excluded.text_body,
                last_accessed = excluded.last_accessed,
                body_size = excluded.body_size",
            params![email_id, body.html_body, body.text_body, now, body_size],
        )?;

        // Trigger eviction if needed using configurable limit
        self.evict_old_bodies(max_bodies)?;

        Ok(())
    }

    /// Get email body with LRU timestamp update
    pub fn get_email_body_lru(&self, email_id: i64) -> Result<Option<EmailBody>> {
        let conn = self.pool.get()?;

        let body = conn
            .query_row(
                "SELECT email_id, html_body, text_body
                 FROM email_bodies
                 WHERE email_id = ?1",
                params![email_id],
                |row| {
                    Ok(EmailBody {
                        email_id: row.get(0)?,
                        html_body: row.get(1)?,
                        text_body: row.get(2)?,
                    })
                },
            )
            .optional()?;

        if body.is_some() {
            // Update last_accessed timestamp
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;

            conn.execute(
                "UPDATE email_bodies SET last_accessed = ?1 WHERE email_id = ?2",
                params![now, email_id],
            )?;
        }

        Ok(body)
    }

    /// Evict old bodies to enforce limit (LRU eviction)
    pub fn evict_old_bodies(&self, limit: usize) -> Result<()> {
        let conn = self.pool.get()?;

        // Count total bodies
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM email_bodies",
            [],
            |row| row.get(0),
        )?;

        if count as usize > limit {
            // Delete oldest bodies beyond limit
            conn.execute(
                "DELETE FROM email_bodies
                 WHERE email_id IN (
                     SELECT email_id FROM email_bodies
                     ORDER BY last_accessed ASC
                     LIMIT ?1
                 )",
                params![count as usize - limit],
            )?;
        }

        Ok(())
    }

    // ========== Sync Metadata ==========

    /// Update sync metadata after syncing a folder
    pub fn update_sync_metadata(
        &self,
        account_email: &str,
        folder_id: &str,
        last_message_date: Option<i64>,
    ) -> Result<()> {
        let conn = self.pool.get()?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Get message count for this folder
        let message_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM emails WHERE account_email = ?1 AND folder_id = ?2",
            params![account_email, folder_id],
            |row| row.get(0),
        )?;

        conn.execute(
            "INSERT INTO sync_metadata (account_email, folder_id, last_sync_timestamp, last_message_date, message_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(account_email, folder_id) DO UPDATE SET
                last_sync_timestamp = excluded.last_sync_timestamp,
                last_message_date = excluded.last_message_date,
                message_count = excluded.message_count,
                updated_at = excluded.updated_at",
            params![account_email, folder_id, now, last_message_date, message_count, now],
        )?;

        Ok(())
    }

    /// Get last sync timestamp for delta sync
    pub fn get_last_sync_timestamp(
        &self,
        account_email: &str,
        folder_id: &str,
    ) -> Result<Option<i64>> {
        let conn = self.pool.get()?;

        let timestamp = conn
            .query_row(
                "SELECT last_message_date FROM sync_metadata WHERE account_email = ?1 AND folder_id = ?2",
                params![account_email, folder_id],
                |row| row.get(0),
            )
            .optional()?;

        Ok(timestamp)
    }

    // ========== Cache Management ==========

    /// Get the database file path from the pool
    pub fn get_db_path(&self) -> Option<PathBuf> {
        // We can query the database filename via PRAGMA
        if let Ok(conn) = self.pool.get() {
            let path: Option<String> = conn
                .query_row("PRAGMA database_list", [], |row| row.get(2))
                .ok();
            path.map(PathBuf::from)
        } else {
            None
        }
    }

    /// Get cache statistics: email count, body count, and DB file size
    pub fn get_cache_stats(&self) -> Result<(i64, i64, u64)> {
        let conn = self.pool.get()?;

        let email_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM emails",
            [],
            |row| row.get(0),
        )?;

        let body_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM email_bodies",
            [],
            |row| row.get(0),
        )?;

        let db_size = if let Some(path) = self.get_db_path() {
            std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        Ok((email_count, body_count, db_size))
    }

    /// Clear all cache data and run VACUUM
    pub fn clear_all_cache(&self) -> Result<()> {
        let conn = self.pool.get()?;

        info!("Clearing all cache data...");

        // Delete all emails and bodies (cascade will handle bodies)
        conn.execute("DELETE FROM emails", [])?;
        conn.execute("DELETE FROM email_bodies", [])?;
        conn.execute("DELETE FROM sync_metadata", [])?;

        // Reset cache stats
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        conn.execute(
            "UPDATE cache_stats SET total_headers = 0, total_bodies = 0, total_body_size_bytes = 0, last_eviction_timestamp = ?1, updated_at = ?1 WHERE id = 1",
            params![now],
        )?;

        // VACUUM to reclaim disk space
        debug!("Running VACUUM to reclaim disk space...");
        conn.execute("VACUUM", [])?;

        info!("Cache cleared successfully");

        Ok(())
    }

    // ========== Tag Management ==========

    /// Create a new tag
    pub fn create_tag(&self, name: &str, color: Option<&str>, icon: Option<&str>) -> Result<Tag> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO tags (name, color, icon, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, color, icon, now],
        )?;

        let id = conn.last_insert_rowid();
        Ok(Tag {
            id,
            name: name.to_string(),
            color: color.map(String::from),
            icon: icon.map(String::from),
            created_at: now,
        })
    }

    /// Get all tags
    pub fn get_tags(&self) -> Result<Vec<Tag>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, color, icon, created_at FROM tags ORDER BY name ASC",
        )?;

        let tags = stmt
            .query_map([], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// Get a tag by ID
    pub fn get_tag(&self, tag_id: i64) -> Result<Option<Tag>> {
        let conn = self.pool.get()?;
        let tag = conn
            .query_row(
                "SELECT id, name, color, icon, created_at FROM tags WHERE id = ?1",
                params![tag_id],
                |row| {
                    Ok(Tag {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        color: row.get(2)?,
                        icon: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                },
            )
            .optional()?;

        Ok(tag)
    }

    /// Update a tag (single statement with conditional SET clauses)
    pub fn update_tag(&self, tag_id: i64, name: Option<&str>, color: Option<&str>, icon: Option<&str>) -> Result<()> {
        let conn = self.pool.get()?;

        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(name) = name {
            set_clauses.push("name = ?");
            param_values.push(Box::new(name.to_string()));
        }
        if let Some(color) = color {
            set_clauses.push("color = ?");
            param_values.push(Box::new(color.to_string()));
        }
        if let Some(icon) = icon {
            set_clauses.push("icon = ?");
            param_values.push(Box::new(icon.to_string()));
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        let sql = format!("UPDATE tags SET {} WHERE id = ?", set_clauses.join(", "));
        param_values.push(Box::new(tag_id));

        let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params.as_slice())?;

        Ok(())
    }

    /// Delete a tag
    pub fn delete_tag(&self, tag_id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("DELETE FROM tags WHERE id = ?1", params![tag_id])?;
        Ok(())
    }

    /// Add a tag to an email
    pub fn add_tag_to_email(&self, email_id: i64, tag_id: i64, applied_by: &str) -> Result<()> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT OR IGNORE INTO email_tags (email_id, tag_id, applied_by, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![email_id, tag_id, applied_by, now],
        )?;

        Ok(())
    }

    /// Remove a tag from an email
    pub fn remove_tag_from_email(&self, email_id: i64, tag_id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM email_tags WHERE email_id = ?1 AND tag_id = ?2",
            params![email_id, tag_id],
        )?;
        Ok(())
    }

    /// Remove a tag from an email only if it was applied by a specific rule.
    /// Returns true if a row was actually deleted.
    pub fn remove_rule_applied_tag(&self, email_id: i64, tag_id: i64, rule_id: i64) -> Result<bool> {
        let conn = self.pool.get()?;
        let applied_by = format!("rule:{}", rule_id);
        let rows_deleted = conn.execute(
            "DELETE FROM email_tags WHERE email_id = ?1 AND tag_id = ?2 AND applied_by = ?3",
            params![email_id, tag_id, applied_by],
        )?;
        Ok(rows_deleted > 0)
    }

    /// Get all tags for an email
    pub fn get_email_tags(&self, email_id: i64) -> Result<Vec<Tag>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, t.icon, t.created_at
             FROM tags t
             INNER JOIN email_tags et ON t.id = et.tag_id
             WHERE et.email_id = ?1
             ORDER BY t.name ASC",
        )?;

        let tags = stmt
            .query_map(params![email_id], |row| {
                Ok(Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tags)
    }

    /// Get all email IDs that have a specific tag
    pub fn get_emails_with_tag(&self, tag_id: i64) -> Result<Vec<i64>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT email_id FROM email_tags WHERE tag_id = ?1",
        )?;

        let email_ids = stmt
            .query_map(params![tag_id], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;

        Ok(email_ids)
    }

    // ========== Tagging Rules Management ==========

    /// Create a new tagging rule
    pub fn create_tagging_rule(&self, input: &CreateTaggingRuleInput) -> Result<TaggingRule> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let criteria_json = serde_json::to_string(&input.criteria)?;

        conn.execute(
            "INSERT INTO tagging_rules (name, tag_id, criteria_json, match_mode, priority, enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                input.name,
                input.tag_id,
                criteria_json,
                input.match_mode,
                input.priority,
                input.enabled,
                now,
            ],
        )?;

        let id = conn.last_insert_rowid();
        Ok(TaggingRule {
            id,
            name: input.name.clone(),
            tag_id: input.tag_id,
            criteria: input.criteria.clone(),
            match_mode: input.match_mode.clone(),
            priority: input.priority,
            enabled: input.enabled,
            created_at: now,
            updated_at: now,
        })
    }

    /// Get all tagging rules
    pub fn get_tagging_rules(&self) -> Result<Vec<TaggingRule>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, tag_id, criteria_json, match_mode, priority, enabled, created_at, updated_at
             FROM tagging_rules
             ORDER BY priority DESC, name ASC",
        )?;

        let rules = stmt
            .query_map([], |row| {
                let criteria_json: String = row.get(3)?;
                let criteria: Vec<FilterCriterion> =
                    serde_json::from_str(&criteria_json).unwrap_or_default();

                Ok(TaggingRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    tag_id: row.get(2)?,
                    criteria,
                    match_mode: row.get(4)?,
                    priority: row.get(5)?,
                    enabled: row.get::<_, i32>(6)? != 0,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rules)
    }

    /// Get enabled tagging rules only
    pub fn get_enabled_tagging_rules(&self) -> Result<Vec<TaggingRule>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, tag_id, criteria_json, match_mode, priority, enabled, created_at, updated_at
             FROM tagging_rules
             WHERE enabled = 1
             ORDER BY priority DESC, name ASC",
        )?;

        let rules = stmt
            .query_map([], |row| {
                let criteria_json: String = row.get(3)?;
                let criteria: Vec<FilterCriterion> =
                    serde_json::from_str(&criteria_json).unwrap_or_default();

                Ok(TaggingRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    tag_id: row.get(2)?,
                    criteria,
                    match_mode: row.get(4)?,
                    priority: row.get(5)?,
                    enabled: true,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rules)
    }

    /// Update a tagging rule
    pub fn update_tagging_rule(&self, rule: &TaggingRule) -> Result<()> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let criteria_json = serde_json::to_string(&rule.criteria)?;

        conn.execute(
            "UPDATE tagging_rules SET
                name = ?1,
                tag_id = ?2,
                criteria_json = ?3,
                match_mode = ?4,
                priority = ?5,
                enabled = ?6,
                updated_at = ?7
             WHERE id = ?8",
            params![
                rule.name,
                rule.tag_id,
                criteria_json,
                rule.match_mode,
                rule.priority,
                rule.enabled,
                now,
                rule.id,
            ],
        )?;

        Ok(())
    }

    /// Delete a tagging rule
    pub fn delete_tagging_rule(&self, rule_id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("DELETE FROM tagging_rules WHERE id = ?1", params![rule_id])?;
        Ok(())
    }

    /// Record rule execution
    pub fn record_rule_execution(&self, rule_id: i64, emails_matched: i32) -> Result<()> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT INTO rule_executions (rule_id, executed_at, emails_matched)
             VALUES (?1, ?2, ?3)",
            params![rule_id, now, emails_matched],
        )?;

        Ok(())
    }

    // ========== Feed Management ==========

    /// Create a new feed
    pub fn create_feed(&self, input: &CreateFeedInput) -> Result<Feed> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let criteria_json = serde_json::to_string(&input.criteria)?;

        // Get the next column order
        let max_order: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(column_order), -1) FROM feeds WHERE account_scope IS ?1",
                params![input.account_scope],
                |row| row.get(0),
            )
            .unwrap_or(-1);

        let column_order = if input.column_order == 0 {
            max_order + 1
        } else {
            input.column_order
        };

        conn.execute(
            "INSERT INTO feeds (name, account_scope, criteria_json, match_mode, column_order, column_width, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                input.name,
                input.account_scope,
                criteria_json,
                input.match_mode,
                column_order,
                input.column_width,
                now,
            ],
        )?;

        let id = conn.last_insert_rowid();
        Ok(Feed {
            id,
            name: input.name.clone(),
            account_scope: input.account_scope.clone(),
            criteria: input.criteria.clone(),
            match_mode: input.match_mode.clone(),
            column_order,
            column_width: input.column_width,
            created_at: now,
            updated_at: now,
        })
    }

    /// Get all feeds, optionally filtered by account scope
    pub fn get_feeds(&self, account_scope: Option<&str>) -> Result<Vec<Feed>> {
        let conn = self.pool.get()?;

        let (sql, use_scope) = match account_scope {
            Some(_) => (
                "SELECT id, name, account_scope, criteria_json, match_mode, column_order, column_width, created_at, updated_at
                 FROM feeds
                 WHERE account_scope = ?1
                 ORDER BY column_order ASC",
                true,
            ),
            None => (
                "SELECT id, name, account_scope, criteria_json, match_mode, column_order, column_width, created_at, updated_at
                 FROM feeds
                 ORDER BY column_order ASC",
                false,
            ),
        };

        let mut stmt = conn.prepare(sql)?;

        let feeds = if use_scope {
            stmt.query_map(params![account_scope], |row| {
                let criteria_json: String = row.get(3)?;
                let criteria: Vec<FilterCriterion> =
                    serde_json::from_str(&criteria_json).unwrap_or_default();

                Ok(Feed {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    account_scope: row.get(2)?,
                    criteria,
                    match_mode: row.get(4)?,
                    column_order: row.get(5)?,
                    column_width: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| {
                let criteria_json: String = row.get(3)?;
                let criteria: Vec<FilterCriterion> =
                    serde_json::from_str(&criteria_json).unwrap_or_default();

                Ok(Feed {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    account_scope: row.get(2)?,
                    criteria,
                    match_mode: row.get(4)?,
                    column_order: row.get(5)?,
                    column_width: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        Ok(feeds)
    }

    /// Get a single feed by ID
    pub fn get_feed(&self, feed_id: i64) -> Result<Option<Feed>> {
        let conn = self.pool.get()?;

        let feed = conn
            .query_row(
                "SELECT id, name, account_scope, criteria_json, match_mode, column_order, column_width, created_at, updated_at
                 FROM feeds
                 WHERE id = ?1",
                params![feed_id],
                |row| {
                    let criteria_json: String = row.get(3)?;
                    let criteria: Vec<FilterCriterion> =
                        serde_json::from_str(&criteria_json).unwrap_or_default();

                    Ok(Feed {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        account_scope: row.get(2)?,
                        criteria,
                        match_mode: row.get(4)?,
                        column_order: row.get(5)?,
                        column_width: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                },
            )
            .optional()?;

        Ok(feed)
    }

    /// Update a feed (single statement with conditional SET clauses)
    pub fn update_feed(&self, input: &UpdateFeedInput) -> Result<()> {
        let conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let mut set_clauses = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(ref name) = input.name {
            set_clauses.push("name = ?");
            param_values.push(Box::new(name.clone()));
        }
        if let Some(ref criteria) = input.criteria {
            let criteria_json = serde_json::to_string(criteria)?;
            set_clauses.push("criteria_json = ?");
            param_values.push(Box::new(criteria_json));
        }
        if let Some(ref match_mode) = input.match_mode {
            set_clauses.push("match_mode = ?");
            param_values.push(Box::new(match_mode.clone()));
        }
        if let Some(column_width) = input.column_width {
            set_clauses.push("column_width = ?");
            param_values.push(Box::new(column_width));
        }

        if set_clauses.is_empty() {
            return Ok(());
        }

        // Always update updated_at
        set_clauses.push("updated_at = ?");
        param_values.push(Box::new(now));

        let sql = format!("UPDATE feeds SET {} WHERE id = ?", set_clauses.join(", "));
        param_values.push(Box::new(input.id));

        let params: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, params.as_slice())?;

        Ok(())
    }

    /// Delete a feed
    pub fn delete_feed(&self, feed_id: i64) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute("DELETE FROM feeds WHERE id = ?1", params![feed_id])?;
        Ok(())
    }

    /// Reorder feeds by providing ordered feed IDs (wrapped in a transaction)
    pub fn reorder_feeds(&self, feed_ids: &[i64]) -> Result<()> {
        let mut conn = self.pool.get()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let tx = conn.transaction()?;
        for (order, feed_id) in feed_ids.iter().enumerate() {
            tx.execute(
                "UPDATE feeds SET column_order = ?1, updated_at = ?2 WHERE id = ?3",
                params![order as i32, now, feed_id],
            )?;
        }
        tx.commit()?;

        Ok(())
    }

    /// Get email by ID
    pub fn get_email_by_id(&self, email_id: i64) -> Result<Option<EmailSummary>> {
        let conn = self.pool.get()?;

        let email = conn
            .query_row(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE id = ?1",
                params![email_id],
                email_summary_from_row,
            )
            .optional()?;

        Ok(email)
    }

    // ========== Account Colors ==========

    /// Get all configured account colors
    pub fn get_account_colors(&self) -> Result<Vec<(String, String)>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT account_email, color FROM account_colors ORDER BY account_email ASC",
        )?;

        let colors = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(colors)
    }

    /// Set color for an account
    pub fn set_account_color(&self, account_email: &str, color: &str) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO account_colors (account_email, color) VALUES (?1, ?2)
             ON CONFLICT(account_email) DO UPDATE SET color = excluded.color",
            params![account_email, color],
        )?;
        Ok(())
    }

    /// Get all email IDs (for rule processing)
    pub fn get_all_email_ids(&self) -> Result<Vec<i64>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare("SELECT id FROM emails")?;

        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;

        Ok(ids)
    }
}
