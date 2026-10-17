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

/// Owner-only permission bits for the database files and the app data directory.
#[cfg(unix)]
const DATABASE_FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const APP_DATA_DIR_MODE: u32 = 0o700;

/// SQLite WAL/shared-memory sidecar suffixes that also hold cached mail data.
#[cfg(unix)]
const SQLITE_SIDECAR_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

/// Escape character used for `LIKE ... ESCAPE` so user-typed `%`, `_` and `\`
/// are matched literally in search queries.
const LIKE_ESCAPE_CHARACTER: char = '\\';

/// Customizer that sets SQLite pragmas on every new connection from the pool.
/// Per-connection pragmas (foreign_keys, cache_size, temp_store, mmap_size) are
/// not inherited by new connections, so they must be set via an on_acquire hook.
#[derive(Debug)]
struct SqlitePragmaCustomizer;

impl r2d2::CustomizeConnection<rusqlite::Connection, rusqlite::Error>
    for SqlitePragmaCustomizer
{
    fn on_acquire(&self, conn: &mut rusqlite::Connection) -> std::result::Result<(), rusqlite::Error> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enforce ON DELETE CASCADE regardless of how the bundled SQLite was compiled.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "cache_size", -8000)?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "mmap_size", 268435456)?;
        Ok(())
    }
}

/// Escape SQL `LIKE` metacharacters (`%`, `_`) and the escape character itself
/// so a user search for `100%` matches that literal text.
pub fn escape_like_pattern(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character == '%' || character == '_' || character == LIKE_ESCAPE_CHARACTER {
            escaped.push(LIKE_ESCAPE_CHARACTER);
        }
        escaped.push(character);
    }
    escaped
}

/// Build the lower-cased, escaped `%query%` pattern used by the search queries.
fn build_like_pattern(query: &str) -> String {
    format!("%{}%", escape_like_pattern(&query.to_lowercase()))
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
        // Ensure parent directory exists (owner-only on unix: it holds the mail cache)
        if let Some(parent) = db_path.parent() {
            Self::create_app_data_dir(parent)?;
        }

        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder()
            .max_size(3) // Single-user desktop app only needs a few connections
            .connection_customizer(Box::new(SqlitePragmaCustomizer))
            .build(manager)
            .context("Failed to create connection pool")?;

        let service = Self { pool };
        service.run_migrations()?;
        Self::restrict_database_file_permissions(&db_path);

        Ok(service)
    }

    #[cfg(unix)]
    fn create_app_data_dir(dir: &std::path::Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(APP_DATA_DIR_MODE)
            .create(dir)
            .with_context(|| format!("Failed to create app data directory {}", dir.display()))
    }

    #[cfg(not(unix))]
    fn create_app_data_dir(dir: &std::path::Path) -> Result<()> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create app data directory {}", dir.display()))
    }

    /// Make the database and its WAL/SHM sidecars readable only by the owner.
    /// Best-effort: a failure is logged, not fatal.
    #[cfg(unix)]
    fn restrict_database_file_permissions(db_path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut candidate_paths: Vec<PathBuf> = vec![db_path.to_path_buf()];
        let db_path_string = db_path.to_string_lossy().into_owned();
        for suffix in SQLITE_SIDECAR_SUFFIXES {
            candidate_paths.push(PathBuf::from(format!("{}{}", db_path_string, suffix)));
        }

        for path in candidate_paths {
            if !path.exists() {
                continue;
            }
            if let Err(e) = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(DATABASE_FILE_MODE)) {
                log::warn!("DATABASE: Failed to set permissions on {}: {}", path.display(), e);
            }
        }
    }

    #[cfg(not(unix))]
    fn restrict_database_file_permissions(_db_path: &std::path::Path) {}

    /// Create a database service backed by a private in-memory SQLite database.
    ///
    /// Uses a uniquely named shared-cache in-memory database so that every
    /// connection in the pool sees the same data (a plain `:memory:` database
    /// is private to a single connection). Some methods hold one pooled
    /// connection while acquiring a second, so the pool must allow more than one.
    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT_DB_ID: AtomicUsize = AtomicUsize::new(0);

        let db_id = NEXT_DB_ID.fetch_add(1, Ordering::SeqCst);
        let uri = format!("file:curier_test_{}?mode=memory&cache=shared", db_id);
        let manager = SqliteConnectionManager::file(uri);
        let pool = Pool::builder()
            .max_size(3)
            .connection_customizer(Box::new(SqlitePragmaCustomizer))
            .build(manager)
            .context("Failed to create in-memory connection pool")?;

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

    /// Store email body (test helper; production uses the LRU/paginated variants)
    #[cfg(test)]
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

    /// Get email body by email ID (test helper; production uses the LRU/paginated variants)
    #[cfg(test)]
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

    /// Mark email as read (test helper; production uses the LRU/paginated variants)
    #[cfg(test)]
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

    /// Get cached emails for a specific account and folder (test helper; production uses the LRU/paginated variants)
    #[cfg(test)]
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
            .map(build_like_pattern);

        if let Some(ref pattern) = search_filter {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails WHERE account_email = ?1 AND folder_id = ?2
                 AND (LOWER(subject) LIKE ?3 ESCAPE '\\' OR LOWER(from_name) LIKE ?3 ESCAPE '\\' OR LOWER(from_address) LIKE ?3 ESCAPE '\\' OR LOWER(snippet) LIKE ?3 ESCAPE '\\')",
                params![account_email, folder_id, pattern],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE account_email = ?1 AND folder_id = ?2
                 AND (LOWER(subject) LIKE ?3 ESCAPE '\\' OR LOWER(from_name) LIKE ?3 ESCAPE '\\' OR LOWER(from_address) LIKE ?3 ESCAPE '\\' OR LOWER(snippet) LIKE ?3 ESCAPE '\\')
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
            .map(build_like_pattern);

        if let Some(ref pattern) = search_filter {
            let total_count: usize = conn.query_row(
                "SELECT COUNT(*) FROM emails
                 WHERE LOWER(subject) LIKE ?1 ESCAPE '\\' OR LOWER(from_name) LIKE ?1 ESCAPE '\\' OR LOWER(from_address) LIKE ?1 ESCAPE '\\' OR LOWER(snippet) LIKE ?1 ESCAPE '\\'",
                params![pattern],
                |row| row.get(0),
            )?;

            let mut stmt = conn.prepare(
                "SELECT id, uid, message_id, subject, from_address, from_name, date, snippet, has_attachments, is_read, account_email
                 FROM emails
                 WHERE LOWER(subject) LIKE ?1 ESCAPE '\\' OR LOWER(from_name) LIKE ?1 ESCAPE '\\' OR LOWER(from_address) LIKE ?1 ESCAPE '\\' OR LOWER(snippet) LIKE ?1 ESCAPE '\\'
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

    /// Get email ID by provider message_id, scoped to the owning account
    /// (two accounts may legitimately hold the same provider message id).
    pub fn get_email_id_by_message_id(&self, account_email: &str, message_id: &str) -> Result<Option<i64>> {
        let conn = self.pool.get()?;

        let email_id = conn
            .query_row(
                "SELECT id FROM emails WHERE account_email = ?1 AND message_id = ?2 LIMIT 1",
                params![account_email, message_id],
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

        // Trigger eviction on the same connection: acquiring a second pooled
        // connection here could deadlock when the pool is exhausted.
        Self::evict_old_bodies_with_connection(&conn, max_bodies)?;

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

    /// Evict old bodies to enforce limit (LRU eviction). Production goes through
    /// `store_email_body_lru`, which evicts on its own connection.
    #[cfg(test)]
    pub fn evict_old_bodies(&self, limit: usize) -> Result<()> {
        let conn = self.pool.get()?;
        Self::evict_old_bodies_with_connection(&conn, limit)
    }

    fn evict_old_bodies_with_connection(conn: &rusqlite::Connection, limit: usize) -> Result<()> {
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

    /// Get a tag by ID (test helper)
    #[cfg(test)]
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

    /// Get all email IDs that have a specific tag (test helper)
    #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ACCOUNT_A: &str = "alice@example.com";
    const ACCOUNT_B: &str = "bob@example.com";
    const INBOX: &str = "INBOX";

    fn db() -> DatabaseService {
        DatabaseService::new_in_memory().expect("in-memory database")
    }

    fn email(uid: u32, subject: &str, date: i64) -> EmailSummary {
        EmailSummary {
            id: None,
            uid,
            message_id: Some(format!("msg-{}", uid)),
            subject: subject.to_string(),
            from_address: format!("sender{}@example.com", uid),
            from_name: Some(format!("Sender {}", uid)),
            date,
            snippet: Some(format!("snippet for {}", subject)),
            has_attachments: uid % 2 == 0,
            is_read: false,
            account_email: None,
        }
    }

    fn criterion(field: &str, operator: &str, value: serde_json::Value) -> FilterCriterion {
        FilterCriterion {
            field: field.to_string(),
            operator: operator.to_string(),
            value,
            negate: false,
        }
    }

    fn schema_version(db: &DatabaseService) -> i64 {
        let conn = db.pool.get().unwrap();
        conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0)).unwrap()
    }

    fn table_names(db: &DatabaseService) -> Vec<String> {
        let conn = db.pool.get().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
    }

    // ----- migrations -----

    #[test]
    fn database_fresh_migration_reaches_current_schema_version() {
        let db = db();
        assert_eq!(schema_version(&db), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn database_fresh_migration_creates_all_tables() {
        let names = table_names(&db());
        for expected in [
            "account_colors", "cache_stats", "email_bodies", "email_tags", "emails",
            "feeds", "rule_executions", "schema_version", "sync_metadata", "tagging_rules", "tags",
        ] {
            assert!(names.iter().any(|n| n == expected), "missing table {}", expected);
        }
    }

    #[test]
    fn database_migrations_are_idempotent_when_run_twice() {
        let db = db();
        db.run_migrations().expect("second migration run should be a no-op");
        assert_eq!(schema_version(&db), CURRENT_SCHEMA_VERSION);

        let conn = db.pool.get().unwrap();
        let version_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version_rows, 1, "schema_version must keep a single row");
    }

    #[test]
    fn database_migrations_preserve_data_when_run_twice() {
        let db = db();
        db.upsert_email_with_context(&email(1, "keep me", 10), ACCOUNT_A, INBOX).unwrap();
        db.run_migrations().unwrap();
        let (emails, total) = db.get_all_cached_emails_paginated(0, 10).unwrap();
        assert_eq!(total, 1);
        assert_eq!(emails[0].subject, "keep me");
    }

    #[test]
    fn database_legacy_unversioned_database_is_stamped_with_current_version() {
        // Simulate a pre-versioning database: emails table exists, no schema_version row.
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            DatabaseService::migration_v1_full_schema(&conn).unwrap();
            DatabaseService::migration_v2_tag_icons_and_account_colors(&conn).unwrap();
        }
        let service = DatabaseService { pool };
        service.run_migrations().expect("legacy database should be adopted");
        assert_eq!(schema_version(&service), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn database_v2_migration_adds_icon_column_to_tags() {
        let db = db();
        let tag = db.create_tag("work", Some("#ff0000"), Some("briefcase")).unwrap();
        let loaded = db.get_tag(tag.id).unwrap().unwrap();
        assert_eq!(loaded.icon.as_deref(), Some("briefcase"));
    }

    #[test]
    fn database_cache_stats_row_is_seeded_once() {
        let db = db();
        db.run_migrations().unwrap();
        let conn = db.pool.get().unwrap();
        let rows: i64 = conn.query_row("SELECT COUNT(*) FROM cache_stats", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
    }

    // ----- emails -----

    #[test]
    fn database_upsert_with_context_inserts_and_returns_row_id() {
        let db = db();
        let id = db.upsert_email_with_context(&email(7, "hello", 100), ACCOUNT_A, INBOX).unwrap();
        let loaded = db.get_email_by_id(id).unwrap().expect("email should exist");
        assert_eq!(loaded.uid, 7);
        assert_eq!(loaded.subject, "hello");
        assert_eq!(loaded.account_email.as_deref(), Some(ACCOUNT_A));
        assert_eq!(loaded.message_id.as_deref(), Some("msg-7"));
    }

    #[test]
    fn database_upsert_with_context_dedups_on_account_folder_uid() {
        let db = db();
        let first = db.upsert_email_with_context(&email(7, "v1", 100), ACCOUNT_A, INBOX).unwrap();
        let second = db.upsert_email_with_context(&email(7, "v2", 200), ACCOUNT_A, INBOX).unwrap();
        assert_eq!(first, second, "same account/folder/uid must update in place");

        let (_, total) = db.get_all_cached_emails_paginated(0, 10).unwrap();
        assert_eq!(total, 1);
        let loaded = db.get_email_by_id(first).unwrap().unwrap();
        assert_eq!(loaded.subject, "v2");
        assert_eq!(loaded.date, 200);
    }

    #[test]
    fn database_same_uid_in_different_accounts_are_separate_rows() {
        let db = db();
        let a = db.upsert_email_with_context(&email(7, "a", 100), ACCOUNT_A, INBOX).unwrap();
        let b = db.upsert_email_with_context(&email(7, "b", 100), ACCOUNT_B, INBOX).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn database_same_uid_in_different_folders_are_separate_rows() {
        let db = db();
        let a = db.upsert_email_with_context(&email(7, "a", 100), ACCOUNT_A, INBOX).unwrap();
        let b = db.upsert_email_with_context(&email(7, "b", 100), ACCOUNT_A, "SENT").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn database_get_email_by_uid_and_account_scopes_by_folder() {
        let db = db();
        db.upsert_email_with_context(&email(7, "inbox", 100), ACCOUNT_A, INBOX).unwrap();
        assert!(db.get_email_by_uid_and_account(7, ACCOUNT_A, INBOX).unwrap().is_some());
        assert!(db.get_email_by_uid_and_account(7, ACCOUNT_A, "SENT").unwrap().is_none());
        assert!(db.get_email_by_uid_and_account(7, ACCOUNT_B, INBOX).unwrap().is_none());
    }

    #[test]
    fn database_folder_pagination_orders_newest_first_and_reports_total() {
        let db = db();
        for i in 1..=5 {
            db.upsert_email_with_context(&email(i, &format!("s{}", i), i as i64 * 10), ACCOUNT_A, INBOX).unwrap();
        }
        let (page1, total) = db.get_cached_emails_by_folder_paginated(ACCOUNT_A, INBOX, 0, 2).unwrap();
        assert_eq!(total, 5);
        assert_eq!(page1.iter().map(|e| e.uid).collect::<Vec<_>>(), vec![5, 4]);

        let (page3, _) = db.get_cached_emails_by_folder_paginated(ACCOUNT_A, INBOX, 4, 2).unwrap();
        assert_eq!(page3.iter().map(|e| e.uid).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn database_search_is_case_insensitive_across_subject_name_address_and_snippet() {
        let db = db();
        db.upsert_email_with_context(&email(1, "Quarterly Report", 10), ACCOUNT_A, INBOX).unwrap();
        db.upsert_email_with_context(&email(2, "lunch", 20), ACCOUNT_A, INBOX).unwrap();

        let (by_subject, n) = db.search_cached_emails_by_folder_paginated(ACCOUNT_A, INBOX, 0, 10, Some("QUARTERLY")).unwrap();
        assert_eq!(n, 1);
        assert_eq!(by_subject[0].uid, 1);

        let (by_address, _) = db.search_all_cached_emails_paginated(0, 10, Some("sender2@")).unwrap();
        assert_eq!(by_address.len(), 1);
        assert_eq!(by_address[0].uid, 2);

        let (by_name, _) = db.search_all_cached_emails_paginated(0, 10, Some("sender 1")).unwrap();
        assert_eq!(by_name.len(), 1);

        let (by_snippet, _) = db.search_all_cached_emails_paginated(0, 10, Some("snippet for lunch")).unwrap();
        assert_eq!(by_snippet.len(), 1);
    }

    #[test]
    fn database_blank_search_query_is_treated_as_no_filter() {
        let db = db();
        db.upsert_email_with_context(&email(1, "a", 10), ACCOUNT_A, INBOX).unwrap();
        db.upsert_email_with_context(&email(2, "b", 20), ACCOUNT_A, INBOX).unwrap();
        let (_, n) = db.search_all_cached_emails_paginated(0, 10, Some("   ")).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn database_evict_old_headers_keeps_newest_per_folder() {
        let db = db();
        for i in 1..=5 {
            db.upsert_email_with_context(&email(i, "x", i as i64), ACCOUNT_A, INBOX).unwrap();
        }
        db.upsert_email_with_context(&email(99, "other account", 1), ACCOUNT_B, INBOX).unwrap();

        db.evict_old_headers(ACCOUNT_A, INBOX, 2).unwrap();

        let remaining = db.get_cached_emails_by_folder(ACCOUNT_A, INBOX, 10).unwrap();
        assert_eq!(remaining.iter().map(|e| e.uid).collect::<Vec<_>>(), vec![5, 4]);
        assert_eq!(db.get_cached_emails_by_folder(ACCOUNT_B, INBOX, 10).unwrap().len(), 1);
    }

    #[test]
    fn database_mark_as_read_flips_flag() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "x", 1), ACCOUNT_A, INBOX).unwrap();
        assert!(!db.get_email_by_id(id).unwrap().unwrap().is_read);
        db.mark_as_read(id).unwrap();
        assert!(db.get_email_by_id(id).unwrap().unwrap().is_read);
    }

    #[test]
    fn database_get_email_id_by_message_id_round_trips() {
        let db = db();
        let id = db.upsert_email_with_context(&email(3, "x", 1), ACCOUNT_A, INBOX).unwrap();
        assert_eq!(db.get_email_id_by_message_id(ACCOUNT_A, "msg-3").unwrap(), Some(id));
        assert_eq!(db.get_email_id_by_message_id(ACCOUNT_A, "missing").unwrap(), None);
    }

    #[test]
    fn database_get_email_id_by_message_id_is_scoped_to_account() {
        let db = db();
        let id_a = db.upsert_email_with_context(&email(3, "x", 1), ACCOUNT_A, INBOX).unwrap();
        let id_b = db.upsert_email_with_context(&email(3, "x", 1), ACCOUNT_B, INBOX).unwrap();
        assert_ne!(id_a, id_b);
        assert_eq!(db.get_email_id_by_message_id(ACCOUNT_A, "msg-3").unwrap(), Some(id_a));
        assert_eq!(db.get_email_id_by_message_id(ACCOUNT_B, "msg-3").unwrap(), Some(id_b));
        assert_eq!(db.get_email_id_by_message_id("nobody@example.com", "msg-3").unwrap(), None);
    }

    #[test]
    fn database_escape_like_pattern_escapes_metacharacters() {
        assert_eq!(escape_like_pattern("100%"), "100\\%");
        assert_eq!(escape_like_pattern("a_b"), "a\\_b");
        assert_eq!(escape_like_pattern("c:\\dir"), "c:\\\\dir");
        assert_eq!(escape_like_pattern("plain"), "plain");
    }

    #[test]
    fn database_search_treats_percent_and_underscore_literally() {
        let db = db();
        let mut discount = email(1, "Save 100% today", 1);
        discount.subject = "Save 100% today".to_string();
        let mut other = email(2, "Save 100 dollars today", 2);
        other.subject = "Save 100 dollars today".to_string();
        db.upsert_email_with_context(&discount, ACCOUNT_A, INBOX).unwrap();
        db.upsert_email_with_context(&other, ACCOUNT_A, INBOX).unwrap();

        let (hits, count) = db.search_cached_emails_by_folder_paginated(ACCOUNT_A, INBOX, 0, 10, Some("100%")).unwrap();
        assert_eq!(count, 1);
        assert_eq!(hits[0].subject, "Save 100% today");

        let (_, count) = db.search_all_cached_emails_paginated(0, 10, Some("100_")).unwrap();
        assert_eq!(count, 0, "underscore must not act as a single-char wildcard");

        let (_, count) = db.search_all_cached_emails_paginated(0, 10, Some("today")).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn database_foreign_keys_pragma_is_enabled_on_pooled_connections() {
        let db = db();
        let conn = db.pool.get().unwrap();
        let enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(enabled, 1);
    }

    #[test]
    fn database_store_email_body_lru_evicts_while_pool_is_otherwise_exhausted() {
        // Regression: eviction used to acquire a second pooled connection while
        // the first was still held, which deadlocks when the pool is exhausted.
        let db = db();
        let ids: Vec<i64> = (1..=2)
            .map(|i| db.upsert_email_with_context(&email(i, "a", i as i64), ACCOUNT_A, INBOX).unwrap())
            .collect();
        let max_size = db.pool.max_size() as usize;
        let held: Vec<_> = (0..max_size - 1).map(|_| db.pool.get().unwrap()).collect();

        for id in &ids {
            db.store_email_body_lru(*id, &EmailBody { email_id: *id, html_body: Some("x".into()), text_body: None }, 1).unwrap();
        }
        drop(held);

        let conn = db.pool.get().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM email_bodies", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn database_delete_account_data_removes_only_that_account() {
        let db = db();
        db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.upsert_email_with_context(&email(2, "b", 1), ACCOUNT_B, INBOX).unwrap();
        db.update_sync_metadata(ACCOUNT_A, INBOX, Some(1)).unwrap();
        db.set_account_color(ACCOUNT_A, "#123456").unwrap();

        db.delete_account_data(ACCOUNT_A).unwrap();

        assert_eq!(db.get_cached_emails_by_folder(ACCOUNT_A, INBOX, 10).unwrap().len(), 0);
        assert_eq!(db.get_cached_emails_by_folder(ACCOUNT_B, INBOX, 10).unwrap().len(), 1);
        assert_eq!(db.get_last_sync_timestamp(ACCOUNT_A, INBOX).unwrap(), None);
        assert!(db.get_account_colors().unwrap().is_empty());
    }

    #[test]
    fn database_deleting_emails_cascades_to_bodies_and_tags() {
        // `SqlitePragmaCustomizer` enables `PRAGMA foreign_keys` on every
        // pooled connection; this test guards the cascades that depend on it.
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.store_email_body(id, &EmailBody { email_id: id, html_body: Some("<p>x</p>".into()), text_body: None }).unwrap();
        let tag = db.create_tag("t", None, None).unwrap();
        db.add_tag_to_email(id, tag.id, "manual").unwrap();

        db.delete_account_data(ACCOUNT_A).unwrap();

        assert!(db.get_email_body(id).unwrap().is_none(), "email_bodies row should cascade");
        assert!(db.get_emails_with_tag(tag.id).unwrap().is_empty(), "email_tags row should cascade");
    }

    #[test]
    fn database_deleting_tag_cascades_to_email_tags_and_rules() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let tag = db.create_tag("t", None, None).unwrap();
        db.add_tag_to_email(id, tag.id, "manual").unwrap();
        db.create_tagging_rule(&rule_input("r", tag.id, 0, true)).unwrap();

        db.delete_tag(tag.id).unwrap();

        assert!(db.get_email_tags(id).unwrap().is_empty());
        assert!(db.get_tagging_rules().unwrap().is_empty());
    }

    // ----- bodies -----

    #[test]
    fn database_store_and_get_email_body_round_trips_and_overwrites() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let body = EmailBody { email_id: id, html_body: Some("<b>hi</b>".into()), text_body: Some("hi".into()) };
        db.store_email_body(id, &body).unwrap();
        let loaded = db.get_email_body(id).unwrap().unwrap();
        assert_eq!(loaded.html_body.as_deref(), Some("<b>hi</b>"));
        assert_eq!(loaded.text_body.as_deref(), Some("hi"));

        let updated = EmailBody { email_id: id, html_body: None, text_body: Some("plain only".into()) };
        db.store_email_body(id, &updated).unwrap();
        let loaded = db.get_email_body(id).unwrap().unwrap();
        assert_eq!(loaded.html_body, None);
        assert_eq!(loaded.text_body.as_deref(), Some("plain only"));
    }

    #[test]
    fn database_get_email_body_returns_none_when_missing() {
        assert!(db().get_email_body(12345).unwrap().is_none());
    }

    #[test]
    fn database_lru_eviction_removes_least_recently_accessed_bodies() {
        let db = db();
        let ids: Vec<i64> = (1..=3)
            .map(|i| db.upsert_email_with_context(&email(i, "a", i as i64), ACCOUNT_A, INBOX).unwrap())
            .collect();
        let conn = db.pool.get().unwrap();
        // Seed bodies with explicit, distinct last_accessed values so the test
        // does not depend on wall-clock resolution.
        for (i, id) in ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO email_bodies (email_id, html_body, text_body, last_accessed, body_size) VALUES (?1, 'h', 't', ?2, 2)",
                params![id, (i as i64 + 1) * 100],
            ).unwrap();
        }
        drop(conn);

        db.evict_old_bodies(2).unwrap();

        assert!(db.get_email_body(ids[0]).unwrap().is_none(), "oldest body should be evicted");
        assert!(db.get_email_body(ids[1]).unwrap().is_some());
        assert!(db.get_email_body(ids[2]).unwrap().is_some());
    }

    #[test]
    fn database_evict_old_bodies_is_noop_under_limit() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.store_email_body_lru(id, &EmailBody { email_id: id, html_body: Some("x".into()), text_body: None }, 5).unwrap();
        db.evict_old_bodies(5).unwrap();
        assert!(db.get_email_body(id).unwrap().is_some());
    }

    #[test]
    fn database_store_email_body_lru_records_body_size() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.store_email_body_lru(id, &EmailBody { email_id: id, html_body: Some("abcd".into()), text_body: Some("ef".into()) }, 10).unwrap();
        let conn = db.pool.get().unwrap();
        let size: i64 = conn.query_row("SELECT body_size FROM email_bodies WHERE email_id = ?1", params![id], |r| r.get(0)).unwrap();
        assert_eq!(size, 6);
    }

    #[test]
    fn database_get_email_body_lru_bumps_last_accessed() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let conn = db.pool.get().unwrap();
        conn.execute(
            "INSERT INTO email_bodies (email_id, html_body, text_body, last_accessed, body_size) VALUES (?1, 'h', 't', 1, 2)",
            params![id],
        ).unwrap();
        drop(conn);

        assert!(db.get_email_body_lru(id).unwrap().is_some());

        let conn = db.pool.get().unwrap();
        let last_accessed: i64 = conn.query_row("SELECT last_accessed FROM email_bodies WHERE email_id = ?1", params![id], |r| r.get(0)).unwrap();
        assert!(last_accessed > 1, "last_accessed should be refreshed to now");
    }

    // ----- sync metadata & cache -----

    #[test]
    fn database_sync_metadata_upserts_and_counts_messages() {
        let db = db();
        assert_eq!(db.get_last_sync_timestamp(ACCOUNT_A, INBOX).unwrap(), None);

        db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.update_sync_metadata(ACCOUNT_A, INBOX, Some(500)).unwrap();
        assert_eq!(db.get_last_sync_timestamp(ACCOUNT_A, INBOX).unwrap(), Some(500));

        db.update_sync_metadata(ACCOUNT_A, INBOX, Some(900)).unwrap();
        assert_eq!(db.get_last_sync_timestamp(ACCOUNT_A, INBOX).unwrap(), Some(900));

        let conn = db.pool.get().unwrap();
        let (rows, count): (i64, i64) = conn
            .query_row("SELECT COUNT(*), MAX(message_count) FROM sync_metadata WHERE account_email = ?1", params![ACCOUNT_A], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(rows, 1, "upsert must not duplicate rows");
        assert_eq!(count, 1);
    }

    #[test]
    fn database_clear_all_cache_wipes_emails_bodies_and_sync_metadata() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.store_email_body(id, &EmailBody { email_id: id, html_body: None, text_body: Some("t".into()) }).unwrap();
        db.update_sync_metadata(ACCOUNT_A, INBOX, Some(1)).unwrap();

        db.clear_all_cache().unwrap();

        let (emails, bodies, _) = db.get_cache_stats().unwrap();
        assert_eq!((emails, bodies), (0, 0));
        assert_eq!(db.get_last_sync_timestamp(ACCOUNT_A, INBOX).unwrap(), None);
    }

    #[test]
    fn database_cache_stats_count_emails_and_bodies() {
        let db = db();
        let a = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        db.upsert_email_with_context(&email(2, "b", 1), ACCOUNT_A, INBOX).unwrap();
        db.store_email_body(a, &EmailBody { email_id: a, html_body: None, text_body: Some("t".into()) }).unwrap();
        let (emails, bodies, _) = db.get_cache_stats().unwrap();
        assert_eq!((emails, bodies), (2, 1));
    }

    // ----- tags -----

    #[test]
    fn database_create_and_list_tags_sorted_by_name() {
        let db = db();
        db.create_tag("zeta", None, None).unwrap();
        db.create_tag("alpha", Some("#fff"), None).unwrap();
        let names: Vec<String> = db.get_tags().unwrap().into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }

    #[test]
    fn database_tag_names_are_unique() {
        let db = db();
        db.create_tag("dup", None, None).unwrap();
        assert!(db.create_tag("dup", None, None).is_err());
    }

    #[test]
    fn database_update_tag_only_touches_provided_fields() {
        let db = db();
        let tag = db.create_tag("orig", Some("#111"), Some("star")).unwrap();
        db.update_tag(tag.id, None, Some("#222"), None).unwrap();
        let loaded = db.get_tag(tag.id).unwrap().unwrap();
        assert_eq!(loaded.name, "orig");
        assert_eq!(loaded.color.as_deref(), Some("#222"));
        assert_eq!(loaded.icon.as_deref(), Some("star"));

        // No-op update must not error.
        db.update_tag(tag.id, None, None, None).unwrap();
    }

    #[test]
    fn database_delete_tag_removes_it() {
        let db = db();
        let tag = db.create_tag("gone", None, None).unwrap();
        db.delete_tag(tag.id).unwrap();
        assert!(db.get_tag(tag.id).unwrap().is_none());
    }

    #[test]
    fn database_add_tag_to_email_is_idempotent_and_preserves_first_applied_by() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let tag = db.create_tag("t", None, None).unwrap();
        db.add_tag_to_email(id, tag.id, "manual").unwrap();
        db.add_tag_to_email(id, tag.id, "rule:9").unwrap();

        assert_eq!(db.get_email_tags(id).unwrap().len(), 1);
        // The INSERT OR IGNORE keeps the original "manual" attribution, so a
        // rule-scoped removal must not delete it.
        assert!(!db.remove_rule_applied_tag(id, tag.id, 9).unwrap());
        assert_eq!(db.get_email_tags(id).unwrap().len(), 1);
    }

    #[test]
    fn database_remove_rule_applied_tag_only_removes_matching_rule() {
        let db = db();
        let id = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let tag = db.create_tag("t", None, None).unwrap();
        db.add_tag_to_email(id, tag.id, "rule:3").unwrap();

        assert!(!db.remove_rule_applied_tag(id, tag.id, 4).unwrap());
        assert!(db.remove_rule_applied_tag(id, tag.id, 3).unwrap());
        assert!(db.get_email_tags(id).unwrap().is_empty());
    }

    #[test]
    fn database_remove_tag_from_email_and_get_emails_with_tag() {
        let db = db();
        let a = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let b = db.upsert_email_with_context(&email(2, "b", 1), ACCOUNT_A, INBOX).unwrap();
        let tag = db.create_tag("t", None, None).unwrap();
        db.add_tag_to_email(a, tag.id, "manual").unwrap();
        db.add_tag_to_email(b, tag.id, "manual").unwrap();

        let mut tagged = db.get_emails_with_tag(tag.id).unwrap();
        tagged.sort();
        assert_eq!(tagged, vec![a, b]);

        db.remove_tag_from_email(a, tag.id).unwrap();
        assert_eq!(db.get_emails_with_tag(tag.id).unwrap(), vec![b]);
    }

    // ----- tagging rules -----

    fn rule_input(name: &str, tag_id: i64, priority: i32, enabled: bool) -> CreateTaggingRuleInput {
        CreateTaggingRuleInput {
            name: name.to_string(),
            tag_id,
            criteria: vec![criterion("subject", "contains", json!("x"))],
            match_mode: "any".to_string(),
            priority,
            enabled,
        }
    }

    #[test]
    fn database_tagging_rule_round_trips_criteria_json() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        let created = db.create_tagging_rule(&rule_input("r", tag.id, 0, true)).unwrap();

        let rules = db.get_tagging_rules().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, created.id);
        assert_eq!(rules[0].match_mode, "any");
        assert_eq!(rules[0].criteria.len(), 1);
        assert_eq!(rules[0].criteria[0].field, "subject");
        assert_eq!(rules[0].criteria[0].value, json!("x"));
        assert!(!rules[0].criteria[0].negate);
    }

    #[test]
    fn database_tagging_rules_sorted_by_priority_desc_then_name() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        db.create_tagging_rule(&rule_input("b-low", tag.id, 1, true)).unwrap();
        db.create_tagging_rule(&rule_input("a-low", tag.id, 1, true)).unwrap();
        db.create_tagging_rule(&rule_input("high", tag.id, 10, true)).unwrap();
        let names: Vec<String> = db.get_tagging_rules().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["high", "a-low", "b-low"]);
    }

    #[test]
    fn database_get_enabled_tagging_rules_filters_disabled() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        db.create_tagging_rule(&rule_input("on", tag.id, 0, true)).unwrap();
        db.create_tagging_rule(&rule_input("off", tag.id, 0, false)).unwrap();
        let enabled: Vec<String> = db.get_enabled_tagging_rules().unwrap().into_iter().map(|r| r.name).collect();
        assert_eq!(enabled, vec!["on"]);
    }

    #[test]
    fn database_update_and_delete_tagging_rule() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        let mut rule = db.create_tagging_rule(&rule_input("r", tag.id, 0, true)).unwrap();
        rule.name = "renamed".to_string();
        rule.enabled = false;
        rule.criteria.push(criterion("from", "equals", json!("a@b.c")));
        db.update_tagging_rule(&rule).unwrap();

        let loaded = db.get_tagging_rules().unwrap().remove(0);
        assert_eq!(loaded.name, "renamed");
        assert!(!loaded.enabled);
        assert_eq!(loaded.criteria.len(), 2);

        db.delete_tagging_rule(rule.id).unwrap();
        assert!(db.get_tagging_rules().unwrap().is_empty());
    }

    #[test]
    fn database_corrupt_criteria_json_degrades_to_empty_criteria() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        let rule = db.create_tagging_rule(&rule_input("r", tag.id, 0, true)).unwrap();
        let conn = db.pool.get().unwrap();
        conn.execute("UPDATE tagging_rules SET criteria_json = 'not json' WHERE id = ?1", params![rule.id]).unwrap();
        drop(conn);
        let loaded = db.get_tagging_rules().unwrap();
        assert!(loaded[0].criteria.is_empty());
    }

    #[test]
    fn database_record_rule_execution_inserts_row() {
        let db = db();
        let tag = db.create_tag("t", None, None).unwrap();
        let rule = db.create_tagging_rule(&rule_input("r", tag.id, 0, true)).unwrap();
        db.record_rule_execution(rule.id, 4).unwrap();
        let conn = db.pool.get().unwrap();
        let matched: i32 = conn.query_row("SELECT emails_matched FROM rule_executions WHERE rule_id = ?1", params![rule.id], |r| r.get(0)).unwrap();
        assert_eq!(matched, 4);
    }

    // ----- feeds -----

    fn feed_input(name: &str, scope: Option<&str>, column_order: i32) -> CreateFeedInput {
        CreateFeedInput {
            name: name.to_string(),
            account_scope: scope.map(String::from),
            criteria: vec![criterion("from", "wildcard", json!("*@example.com"))],
            match_mode: "all".to_string(),
            column_order,
            column_width: 4,
        }
    }

    #[test]
    fn database_create_feed_round_trips() {
        let db = db();
        let created = db.create_feed(&feed_input("Work", Some(ACCOUNT_A), 0)).unwrap();
        let loaded = db.get_feed(created.id).unwrap().unwrap();
        assert_eq!(loaded.name, "Work");
        assert_eq!(loaded.account_scope.as_deref(), Some(ACCOUNT_A));
        assert_eq!(loaded.column_width, 4);
        assert_eq!(loaded.criteria[0].operator, "wildcard");
    }

    #[test]
    fn database_create_feed_auto_assigns_next_column_order_per_scope() {
        let db = db();
        let f1 = db.create_feed(&feed_input("one", None, 0)).unwrap();
        let f2 = db.create_feed(&feed_input("two", None, 0)).unwrap();
        let scoped = db.create_feed(&feed_input("scoped", Some(ACCOUNT_A), 0)).unwrap();
        assert_eq!(f1.column_order, 0);
        assert_eq!(f2.column_order, 1);
        assert_eq!(scoped.column_order, 0, "column order is tracked independently per scope");

        let explicit = db.create_feed(&feed_input("explicit", None, 7)).unwrap();
        assert_eq!(explicit.column_order, 7);
    }

    #[test]
    fn database_get_feeds_filters_by_scope_and_orders_by_column() {
        let db = db();
        db.create_feed(&feed_input("global", None, 0)).unwrap();
        db.create_feed(&feed_input("a2", Some(ACCOUNT_A), 2)).unwrap();
        db.create_feed(&feed_input("a1", Some(ACCOUNT_A), 1)).unwrap();

        let scoped: Vec<String> = db.get_feeds(Some(ACCOUNT_A)).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(scoped, vec!["a1", "a2"]);

        // None returns every feed regardless of scope.
        assert_eq!(db.get_feeds(None).unwrap().len(), 3);
    }

    #[test]
    fn database_update_feed_applies_only_provided_fields_and_bumps_updated_at() {
        let db = db();
        let created = db.create_feed(&feed_input("orig", None, 0)).unwrap();
        let conn = db.pool.get().unwrap();
        conn.execute("UPDATE feeds SET updated_at = 1 WHERE id = ?1", params![created.id]).unwrap();
        drop(conn);

        db.update_feed(&UpdateFeedInput {
            id: created.id,
            name: Some("renamed".to_string()),
            criteria: None,
            match_mode: Some("any".to_string()),
            column_width: Some(8),
        }).unwrap();

        let loaded = db.get_feed(created.id).unwrap().unwrap();
        assert_eq!(loaded.name, "renamed");
        assert_eq!(loaded.match_mode, "any");
        assert_eq!(loaded.column_width, 8);
        assert_eq!(loaded.criteria.len(), 1, "criteria untouched");
        assert!(loaded.updated_at > 1);

        // Empty update is a no-op and must not error.
        db.update_feed(&UpdateFeedInput { id: created.id, name: None, criteria: None, match_mode: None, column_width: None }).unwrap();
    }

    #[test]
    fn database_reorder_feeds_assigns_sequential_order() {
        let db = db();
        let a = db.create_feed(&feed_input("a", None, 0)).unwrap();
        let b = db.create_feed(&feed_input("b", None, 0)).unwrap();
        let c = db.create_feed(&feed_input("c", None, 0)).unwrap();

        db.reorder_feeds(&[c.id, a.id, b.id]).unwrap();

        let names: Vec<String> = db.get_feeds(None).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }

    #[test]
    fn database_delete_feed_removes_it() {
        let db = db();
        let f = db.create_feed(&feed_input("x", None, 0)).unwrap();
        db.delete_feed(f.id).unwrap();
        assert!(db.get_feed(f.id).unwrap().is_none());
    }

    // ----- account colors -----

    #[test]
    fn database_account_color_upserts_and_lists_sorted() {
        let db = db();
        db.set_account_color(ACCOUNT_B, "#b").unwrap();
        db.set_account_color(ACCOUNT_A, "#a").unwrap();
        db.set_account_color(ACCOUNT_A, "#a2").unwrap();
        assert_eq!(
            db.get_account_colors().unwrap(),
            vec![(ACCOUNT_A.to_string(), "#a2".to_string()), (ACCOUNT_B.to_string(), "#b".to_string())]
        );
    }

    #[test]
    fn database_get_all_email_ids_lists_every_row() {
        let db = db();
        let a = db.upsert_email_with_context(&email(1, "a", 1), ACCOUNT_A, INBOX).unwrap();
        let b = db.upsert_email_with_context(&email(2, "b", 1), ACCOUNT_B, INBOX).unwrap();
        let mut ids = db.get_all_email_ids().unwrap();
        ids.sort();
        assert_eq!(ids, vec![a, b]);
    }
}
