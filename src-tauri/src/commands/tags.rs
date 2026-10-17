use crate::models::feeds::{
    CreateTaggingRuleInput, RuleExecutionResult, Tag, TaggingRule,
};
use crate::services::rule_engine::evaluate_email_against_rule;
use crate::state::AppState;
use log::debug;
use tauri::State;

// ========== Tag CRUD Commands ==========

#[tauri::command]
pub async fn create_tag(
    state: State<'_, AppState>,
    name: String,
    color: Option<String>,
    icon: Option<String>,
) -> Result<Tag, String> {
    debug!("create_tag: Creating tag '{}'", name);

    let db = state.database.lock().await;
    let tag = db
        .create_tag(&name, color.as_deref(), icon.as_deref())
        .map_err(|e| format!("Failed to create tag: {}", e))?;

    debug!("create_tag: Tag '{}' created with id {}", name, tag.id);
    Ok(tag)
}

#[tauri::command]
pub async fn get_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    debug!("get_tags: Fetching all tags");

    let db = state.database.lock().await;
    let tags = db
        .get_tags()
        .map_err(|e| format!("Failed to get tags: {}", e))?;

    debug!("get_tags: Found {} tags", tags.len());
    Ok(tags)
}

#[tauri::command]
pub async fn update_tag(
    state: State<'_, AppState>,
    tag_id: i64,
    name: Option<String>,
    color: Option<String>,
    icon: Option<String>,
) -> Result<(), String> {
    debug!("update_tag: Updating tag {}", tag_id);

    let db = state.database.lock().await;
    db.update_tag(tag_id, name.as_deref(), color.as_deref(), icon.as_deref())
        .map_err(|e| format!("Failed to update tag: {}", e))?;

    debug!("update_tag: Tag {} updated", tag_id);
    Ok(())
}

#[tauri::command]
pub async fn delete_tag(state: State<'_, AppState>, tag_id: i64) -> Result<(), String> {
    debug!("delete_tag: Deleting tag {}", tag_id);

    let db = state.database.lock().await;
    db.delete_tag(tag_id)
        .map_err(|e| format!("Failed to delete tag: {}", e))?;

    debug!("delete_tag: Tag {} deleted", tag_id);
    Ok(())
}

// ========== Email Tag Assignment Commands ==========

#[tauri::command]
pub async fn add_tag_to_email(
    state: State<'_, AppState>,
    email_id: i64,
    tag_id: i64,
) -> Result<(), String> {
    debug!("add_tag_to_email: Adding tag {} to email {}", tag_id, email_id);

    let db = state.database.lock().await;
    db.add_tag_to_email(email_id, tag_id, "manual")
        .map_err(|e| format!("Failed to add tag to email: {}", e))?;

    debug!("add_tag_to_email: Tag {} added to email {}", tag_id, email_id);
    Ok(())
}

#[tauri::command]
pub async fn remove_tag_from_email(
    state: State<'_, AppState>,
    email_id: i64,
    tag_id: i64,
) -> Result<(), String> {
    debug!(
        "remove_tag_from_email: Removing tag {} from email {}",
        tag_id, email_id
    );

    let db = state.database.lock().await;
    db.remove_tag_from_email(email_id, tag_id)
        .map_err(|e| format!("Failed to remove tag from email: {}", e))?;

    debug!(
        "remove_tag_from_email: Tag {} removed from email {}",
        tag_id, email_id
    );
    Ok(())
}

#[tauri::command]
pub async fn get_email_tags(
    state: State<'_, AppState>,
    email_id: i64,
) -> Result<Vec<Tag>, String> {
    debug!("get_email_tags: Fetching tags for email {}", email_id);

    let db = state.database.lock().await;
    let tags = db
        .get_email_tags(email_id)
        .map_err(|e| format!("Failed to get email tags: {}", e))?;

    debug!("get_email_tags: Found {} tags for email {}", tags.len(), email_id);
    Ok(tags)
}

// ========== Tagging Rules Commands ==========

#[tauri::command]
pub async fn create_tagging_rule(
    state: State<'_, AppState>,
    input: CreateTaggingRuleInput,
) -> Result<TaggingRule, String> {
    debug!("create_tagging_rule: Creating rule '{}'", input.name);

    let db = state.database.lock().await;
    let rule = db
        .create_tagging_rule(&input)
        .map_err(|e| format!("Failed to create tagging rule: {}", e))?;

    debug!(
        "create_tagging_rule: Rule '{}' created with id {}",
        rule.name, rule.id
    );
    Ok(rule)
}

#[tauri::command]
pub async fn get_tagging_rules(state: State<'_, AppState>) -> Result<Vec<TaggingRule>, String> {
    debug!("get_tagging_rules: Fetching all tagging rules");

    let db = state.database.lock().await;
    let rules = db
        .get_tagging_rules()
        .map_err(|e| format!("Failed to get tagging rules: {}", e))?;

    debug!("get_tagging_rules: Found {} rules", rules.len());
    Ok(rules)
}

#[tauri::command]
pub async fn update_tagging_rule(
    state: State<'_, AppState>,
    rule: TaggingRule,
) -> Result<(), String> {
    debug!("update_tagging_rule: Updating rule {}", rule.id);

    let db = state.database.lock().await;
    db.update_tagging_rule(&rule)
        .map_err(|e| format!("Failed to update tagging rule: {}", e))?;

    debug!("update_tagging_rule: Rule {} updated", rule.id);
    Ok(())
}

#[tauri::command]
pub async fn delete_tagging_rule(state: State<'_, AppState>, rule_id: i64) -> Result<(), String> {
    debug!("delete_tagging_rule: Deleting rule {}", rule_id);

    let db = state.database.lock().await;
    db.delete_tagging_rule(rule_id)
        .map_err(|e| format!("Failed to delete tagging rule: {}", e))?;

    debug!("delete_tagging_rule: Rule {} deleted", rule_id);
    Ok(())
}

#[tauri::command]
pub async fn run_tagging_rules(
    state: State<'_, AppState>,
    email_ids: Option<Vec<i64>>,
) -> Result<RuleExecutionResult, String> {
    debug!("run_tagging_rules: Running tagging rules");

    let db = state.database.lock().await;

    // Get enabled rules
    let rules = db
        .get_enabled_tagging_rules()
        .map_err(|e| format!("Failed to get tagging rules: {}", e))?;

    if rules.is_empty() {
        debug!("run_tagging_rules: No enabled rules found");
        return Ok(RuleExecutionResult {
            rules_executed: 0,
            emails_processed: 0,
            tags_applied: 0,
            tags_removed: 0,
        });
    }

    // Get email IDs to process
    let ids_to_process = match email_ids {
        Some(ids) => ids,
        None => db
            .get_all_email_ids()
            .map_err(|e| format!("Failed to get email IDs: {}", e))?,
    };

    let mut total_tags_applied = 0;
    let mut total_tags_removed = 0;
    let emails_processed = ids_to_process.len() as i32;

    // Process each email against each rule
    for email_id in &ids_to_process {
        if let Some(email) = db
            .get_email_by_id(*email_id)
            .map_err(|e| format!("Failed to get email: {}", e))?
        {
            for rule in &rules {
                if evaluate_email_against_rule(&email, rule) {
                    // Apply tag
                    let applied_by = format!("rule:{}", rule.id);
                    if db
                        .add_tag_to_email(*email_id, rule.tag_id, &applied_by)
                        .is_ok()
                    {
                        total_tags_applied += 1;
                    }
                } else {
                    // Remove tag if it was applied by this rule (preserves manually applied tags)
                    if let Ok(true) = db.remove_rule_applied_tag(*email_id, rule.tag_id, rule.id) {
                        total_tags_removed += 1;
                    }
                }
            }
        }
    }

    // Record rule executions
    for rule in &rules {
        let _ = db.record_rule_execution(rule.id, 0);
    }

    let result = RuleExecutionResult {
        rules_executed: rules.len() as i32,
        emails_processed,
        tags_applied: total_tags_applied,
        tags_removed: total_tags_removed,
    };

    debug!(
        "run_tagging_rules: Executed {} rules on {} emails, applied {} tags, removed {} tags",
        result.rules_executed, result.emails_processed, result.tags_applied, result.tags_removed
    );

    Ok(result)
}

// Rule evaluation logic is in crate::services::rule_engine
