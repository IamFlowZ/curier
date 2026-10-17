use crate::models::email::EmailSummary;
use crate::models::feeds::{FilterCriterion, TaggingRule};

/// Evaluate an email against a tagging rule's criteria.
/// Returns true if the email matches the rule.
pub fn evaluate_email_against_rule(email: &EmailSummary, rule: &TaggingRule) -> bool {
    if rule.criteria.is_empty() {
        return false;
    }

    let results: Vec<bool> = rule
        .criteria
        .iter()
        .map(|criterion| {
            let matches = match criterion.field.as_str() {
                "from" => match_string_criterion(&email.from_address, criterion),
                "subject" => match_string_criterion(&email.subject, criterion),
                "is_read" => criterion.value.as_bool().map_or(false, |v| email.is_read == v),
                "has_attachments" => {
                    criterion.value.as_bool().map_or(false, |v| email.has_attachments == v)
                }
                "date" => match_date_criterion(email.date, criterion),
                _ => false,
            };

            if criterion.negate {
                !matches
            } else {
                matches
            }
        })
        .collect();

    match rule.match_mode.as_str() {
        "all" => results.iter().all(|&r| r),
        "any" => results.iter().any(|&r| r),
        _ => false,
    }
}

/// Match a string field against a criterion (case-insensitive)
fn match_string_criterion(text: &str, criterion: &FilterCriterion) -> bool {
    let value = match criterion.value.as_str() {
        Some(v) => v,
        None => return false,
    };

    let text_lower = text.to_lowercase();
    let value_lower = value.to_lowercase();

    match criterion.operator.as_str() {
        "equals" => text_lower == value_lower,
        "contains" => text_lower.contains(&value_lower),
        "starts_with" => text_lower.starts_with(&value_lower),
        "wildcard" => match_wildcard(&text_lower, &value_lower),
        _ => false,
    }
}

/// Match a date field against a criterion
fn match_date_criterion(date: i64, criterion: &FilterCriterion) -> bool {
    match criterion.operator.as_str() {
        "before" => criterion.value.as_i64().map_or(false, |v| date < v),
        "after" => criterion.value.as_i64().map_or(false, |v| date > v),
        "between" => {
            if let Some(arr) = criterion.value.as_array() {
                if arr.len() == 2 {
                    let start = arr[0].as_i64().unwrap_or(0);
                    let end = arr[1].as_i64().unwrap_or(i64::MAX);
                    return date >= start && date <= end;
                }
            }
            false
        }
        _ => false,
    }
}

/// Match text against a wildcard pattern (* = any characters)
fn match_wildcard(text: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.is_empty() {
        return true;
    }

    let mut remaining = text;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 && !pattern.ends_with('*') {
            if !remaining.ends_with(part) {
                return false;
            }
        } else {
            match remaining.find(part) {
                Some(pos) => {
                    remaining = &remaining[pos + part.len()..];
                }
                None => return false,
            }
        }
    }

    true
}
