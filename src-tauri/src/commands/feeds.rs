use crate::models::email::EmailSummary;
use crate::models::feeds::{CreateFeedInput, Feed, FilterCriterion, PaginatedFeedResponse, UpdateFeedInput};
use crate::services::database::email_summary_from_row;
use crate::state::AppState;
use log::debug;
use tauri::State;

// ========== Feed CRUD Commands ==========

#[tauri::command]
pub async fn create_feed(
    state: State<'_, AppState>,
    mut input: CreateFeedInput,
) -> Result<Feed, String> {
    debug!("create_feed: Creating feed '{}'", input.name);

    // Clamp column width to 1-12 range
    input.column_width = input.column_width.clamp(1, 12);

    let db = state.database.lock().await;
    let feed = db
        .create_feed(&input)
        .map_err(|e| format!("Failed to create feed: {}", e))?;

    debug!(
        "create_feed: Feed '{}' created with id {}",
        feed.name, feed.id
    );
    Ok(feed)
}

#[tauri::command]
pub async fn get_feeds(
    state: State<'_, AppState>,
    account_scope: Option<String>,
) -> Result<Vec<Feed>, String> {
    debug!(
        "get_feeds: Fetching feeds for scope {:?}",
        account_scope
    );

    let db = state.database.lock().await;
    let feeds = db
        .get_feeds(account_scope.as_deref())
        .map_err(|e| format!("Failed to get feeds: {}", e))?;

    debug!("get_feeds: Found {} feeds", feeds.len());
    Ok(feeds)
}

#[tauri::command]
pub async fn get_feed(state: State<'_, AppState>, feed_id: i64) -> Result<Option<Feed>, String> {
    debug!("get_feed: Fetching feed {}", feed_id);

    let db = state.database.lock().await;
    let feed = db
        .get_feed(feed_id)
        .map_err(|e| format!("Failed to get feed: {}", e))?;

    debug!("get_feed: Feed {} found: {}", feed_id, feed.is_some());
    Ok(feed)
}

#[tauri::command]
pub async fn update_feed(state: State<'_, AppState>, mut input: UpdateFeedInput) -> Result<(), String> {
    debug!("update_feed: Updating feed {}", input.id);

    // Clamp column width to 1-12 range if provided
    if let Some(width) = input.column_width {
        input.column_width = Some(width.clamp(1, 12));
    }

    let db = state.database.lock().await;
    db.update_feed(&input)
        .map_err(|e| format!("Failed to update feed: {}", e))?;

    debug!("update_feed: Feed {} updated", input.id);
    Ok(())
}

#[tauri::command]
pub async fn delete_feed(state: State<'_, AppState>, feed_id: i64) -> Result<(), String> {
    debug!("delete_feed: Deleting feed {}", feed_id);

    let db = state.database.lock().await;
    db.delete_feed(feed_id)
        .map_err(|e| format!("Failed to delete feed: {}", e))?;

    debug!("delete_feed: Feed {} deleted", feed_id);
    Ok(())
}

#[tauri::command]
pub async fn reorder_feeds(
    state: State<'_, AppState>,
    feed_ids: Vec<i64>,
) -> Result<(), String> {
    debug!("reorder_feeds: Reordering {} feeds", feed_ids.len());

    let db = state.database.lock().await;
    db.reorder_feeds(&feed_ids)
        .map_err(|e| format!("Failed to reorder feeds: {}", e))?;

    debug!("reorder_feeds: Feeds reordered");
    Ok(())
}

// ========== Feed Email Fetching ==========

#[tauri::command]
pub async fn fetch_feed_emails(
    state: State<'_, AppState>,
    feed_id: i64,
    page: u32,
    limit: u32,
) -> Result<PaginatedFeedResponse, String> {
    debug!(
        "fetch_feed_emails: Fetching emails for feed {}, page {}, limit {}",
        feed_id, page, limit
    );

    let db = state.database.lock().await;

    // Get the feed configuration
    let feed = db
        .get_feed(feed_id)
        .map_err(|e| format!("Failed to get feed: {}", e))?
        .ok_or_else(|| format!("Feed {} not found", feed_id))?;

    // Build and execute the query
    let (emails, total_count) = execute_feed_query(&db, &feed, page, limit)
        .map_err(|e| format!("Failed to fetch feed emails: {}", e))?;

    let has_more = (page * limit) < total_count;

    debug!(
        "fetch_feed_emails: Found {} emails (total: {})",
        emails.len(),
        total_count
    );

    Ok(PaginatedFeedResponse {
        emails,
        total_count,
        page,
        has_more,
    })
}

/// Execute a feed query with criteria filtering
fn execute_feed_query(
    db: &crate::services::database::DatabaseService,
    feed: &Feed,
    page: u32,
    limit: u32,
) -> anyhow::Result<(Vec<EmailSummary>, u32)> {
    let conn = db.pool.get()?;
    let offset = (page.saturating_sub(1)) * limit;

    // Build the WHERE clause based on criteria
    let (where_clause, mut where_params) =
        build_criteria_where_clause(&feed.criteria, &feed.match_mode, &feed.account_scope);

    debug!("execute_feed_query: WHERE clause: {}", where_clause);
    debug!("execute_feed_query: WHERE params: {:?}", where_params);
    debug!("execute_feed_query: Feed account_scope: {:?}", feed.account_scope);
    debug!("execute_feed_query: Feed criteria count: {}", feed.criteria.len());

    // Count query
    let count_sql = format!(
        "SELECT COUNT(*) FROM emails e {}",
        if where_clause.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clause)
        }
    );

    let total_count: u32 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let params_slice: Vec<&dyn rusqlite::ToSql> =
            where_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        stmt.query_row(params_slice.as_slice(), |row| row.get(0))?
    };

    // Main query - LIMIT and OFFSET are also parameterized
    let select_sql = format!(
        "SELECT e.id, e.uid, e.message_id, e.subject, e.from_address, e.from_name,
                e.date, e.snippet, e.has_attachments, e.is_read, e.account_email
         FROM emails e
         {}
         ORDER BY e.date DESC
         LIMIT ? OFFSET ?",
        if where_clause.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clause)
        },
    );

    // Append limit and offset to params
    where_params.push(limit.to_string());
    where_params.push(offset.to_string());

    let mut stmt = conn.prepare(&select_sql)?;
    let params_slice: Vec<&dyn rusqlite::ToSql> =
        where_params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let emails = stmt
        .query_map(params_slice.as_slice(), email_summary_from_row)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok((emails, total_count))
}

/// Build a WHERE clause from filter criteria
fn build_criteria_where_clause(
    criteria: &[FilterCriterion],
    match_mode: &str,
    account_scope: &Option<String>,
) -> (String, Vec<String>) {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    // Add account scope filter if specified
    if let Some(account) = account_scope {
        conditions.push("e.account_email = ?".to_string());
        params.push(account.clone());
    }

    // Add criteria conditions
    for criterion in criteria {
        if let Some((condition, criterion_params)) = criterion_to_sql(criterion) {
            conditions.push(condition);
            params.extend(criterion_params);
        }
    }

    if conditions.is_empty() {
        return (String::new(), params);
    }

    let joiner = if match_mode == "any" { " OR " } else { " AND " };

    // If we have account scope AND criteria, structure properly
    if account_scope.is_some() && criteria.len() > 0 {
        // Account scope is always AND with the criteria group
        let account_condition = conditions.remove(0);
        let criteria_conditions = conditions.join(joiner);
        if criteria_conditions.is_empty() {
            (account_condition, params)
        } else {
            (
                format!("{} AND ({})", account_condition, criteria_conditions),
                params,
            )
        }
    } else {
        (conditions.join(joiner), params)
    }
}

/// Convert a single criterion to a SQL condition with parameterized values.
/// Returns (condition_string, param_values) where condition uses `?` placeholders.
fn criterion_to_sql(criterion: &FilterCriterion) -> Option<(String, Vec<String>)> {
    let negate = if criterion.negate { "NOT " } else { "" };

    match criterion.field.as_str() {
        "from" => {
            let value = criterion.value.as_str()?;
            let condition = match criterion.operator.as_str() {
                "equals" => format!("{}LOWER(e.from_address) = LOWER(?)", negate),
                "contains" => format!("{}LOWER(e.from_address) LIKE LOWER(?)", negate),
                "starts_with" => format!("{}LOWER(e.from_address) LIKE LOWER(?)", negate),
                "wildcard" => format!("{}LOWER(e.from_address) GLOB LOWER(?)", negate),
                _ => return None,
            };
            let param = match criterion.operator.as_str() {
                "contains" => format!("%{}%", value),
                "starts_with" => format!("{}%", value),
                _ => value.to_string(),
            };
            Some((condition, vec![param]))
        }
        "to" => None,
        "subject" => {
            let value = criterion.value.as_str()?;
            let condition = match criterion.operator.as_str() {
                "equals" => format!("{}LOWER(e.subject) = LOWER(?)", negate),
                "contains" => format!("{}LOWER(e.subject) LIKE LOWER(?)", negate),
                "starts_with" => format!("{}LOWER(e.subject) LIKE LOWER(?)", negate),
                "wildcard" => format!("{}LOWER(e.subject) GLOB LOWER(?)", negate),
                _ => return None,
            };
            let param = match criterion.operator.as_str() {
                "contains" => format!("%{}%", value),
                "starts_with" => format!("{}%", value),
                _ => value.to_string(),
            };
            Some((condition, vec![param]))
        }
        "body" => None,
        "tag" => {
            let tag_id = criterion.value.as_i64()?;
            let condition = format!(
                "{}EXISTS (SELECT 1 FROM email_tags et WHERE et.email_id = e.id AND et.tag_id = ?)",
                negate
            );
            Some((condition, vec![tag_id.to_string()]))
        }
        "is_read" => {
            let value = criterion.value.as_bool()?;
            let sql_value = if value { "1" } else { "0" };
            let condition = format!("{}e.is_read = ?", negate);
            Some((condition, vec![sql_value.to_string()]))
        }
        "has_attachments" => {
            let value = criterion.value.as_bool()?;
            let sql_value = if value { "1" } else { "0" };
            let condition = format!("{}e.has_attachments = ?", negate);
            Some((condition, vec![sql_value.to_string()]))
        }
        "date" => {
            match criterion.operator.as_str() {
                "before" => {
                    let value = criterion.value.as_i64()?;
                    let condition = format!("{}e.date < ?", negate);
                    Some((condition, vec![value.to_string()]))
                }
                "after" => {
                    let value = criterion.value.as_i64()?;
                    let condition = format!("{}e.date > ?", negate);
                    Some((condition, vec![value.to_string()]))
                }
                "between" => {
                    let arr = criterion.value.as_array()?;
                    if arr.len() != 2 {
                        return None;
                    }
                    let start = arr[0].as_i64()?;
                    let end = arr[1].as_i64()?;
                    let condition = format!("{}(e.date >= ? AND e.date <= ?)", negate);
                    Some((condition, vec![start.to_string(), end.to_string()]))
                }
                _ => None,
            }
        }
        _ => None,
    }
}
