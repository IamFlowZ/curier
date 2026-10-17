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

/// Glob character that matches any run of characters (including none).
const WILDCARD_CHARACTER: char = '*';

/// Match text against a wildcard pattern (`*` = any characters), case-insensitively.
/// A pattern without `*` must match the whole text exactly.
fn match_wildcard(text: &str, pattern: &str) -> bool {
    let text = text.to_lowercase();
    let pattern = pattern.to_lowercase();

    if !pattern.contains(WILDCARD_CHARACTER) {
        return text == pattern;
    }

    let parts: Vec<&str> = pattern.split(WILDCARD_CHARACTER).collect();
    let first_part = parts[0];
    let last_part = parts[parts.len() - 1];

    if !text.starts_with(first_part) {
        return false;
    }
    let mut remaining: &str = &text[first_part.len()..];

    for middle_part in &parts[1..parts.len() - 1] {
        if middle_part.is_empty() {
            continue;
        }
        match remaining.find(middle_part) {
            Some(position) => remaining = &remaining[position + middle_part.len()..],
            None => return false,
        }
    }

    remaining.ends_with(last_part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const TEST_DATE: i64 = 1_700_000_000;

    fn sample_email() -> EmailSummary {
        EmailSummary {
            id: Some(1),
            uid: 42,
            message_id: Some("msg-1".to_string()),
            subject: "Weekly Newsletter: Rust Updates".to_string(),
            from_address: "News@Example.com".to_string(),
            from_name: Some("Example News".to_string()),
            date: TEST_DATE,
            snippet: None,
            has_attachments: true,
            is_read: false,
            account_email: Some("me@example.com".to_string()),
        }
    }

    fn criterion(field: &str, operator: &str, value: Value) -> FilterCriterion {
        FilterCriterion {
            field: field.to_string(),
            operator: operator.to_string(),
            value,
            negate: false,
        }
    }

    fn negated(mut criterion: FilterCriterion) -> FilterCriterion {
        criterion.negate = true;
        criterion
    }

    fn rule(match_mode: &str, criteria: Vec<FilterCriterion>) -> TaggingRule {
        TaggingRule {
            id: 1,
            name: "test rule".to_string(),
            tag_id: 1,
            criteria,
            match_mode: match_mode.to_string(),
            priority: 0,
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    // ----- empty / degenerate rules -----

    #[test]
    fn rule_engine_empty_criteria_never_matches() {
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![])));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("any", vec![])));
    }

    #[test]
    fn rule_engine_unknown_match_mode_never_matches() {
        let always_true = criterion("subject", "contains", json!("newsletter"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("xor", vec![always_true])));
    }

    #[test]
    fn rule_engine_unknown_field_does_not_match() {
        let c = criterion("body", "contains", json!("anything"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_unknown_string_operator_does_not_match() {
        let c = criterion("subject", "regex", json!(".*"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_non_string_value_for_string_field_does_not_match() {
        let c = criterion("subject", "contains", json!(123));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    // ----- string operators -----

    #[test]
    fn rule_engine_matches_subject_contains_case_insensitively() {
        let c = criterion("subject", "contains", json!("RUST updates"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_subject_contains_rejects_missing_substring() {
        let c = criterion("subject", "contains", json!("python"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_matches_from_equals_case_insensitively() {
        let c = criterion("from", "equals", json!("news@example.com"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_from_equals_requires_whole_string() {
        let c = criterion("from", "equals", json!("news@example"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_matches_subject_starts_with_case_insensitively() {
        let c = criterion("subject", "starts_with", json!("weekly"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_starts_with_rejects_mid_string_match() {
        let c = criterion("subject", "starts_with", json!("newsletter"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_from_matches_against_address_not_display_name() {
        let c = criterion("from", "contains", json!("Example News"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    // ----- wildcard -----

    #[test]
    fn rule_engine_wildcard_matches_domain_suffix() {
        let c = criterion("from", "wildcard", json!("*@example.com"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_wildcard_matches_prefix() {
        let c = criterion("subject", "wildcard", json!("weekly*"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_wildcard_matches_prefix_middle_and_suffix() {
        let c = criterion("subject", "wildcard", json!("weekly*rust*updates"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_wildcard_rejects_wrong_suffix() {
        let c = criterion("from", "wildcard", json!("*@other.com"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_wildcard_without_star_is_exact_case_insensitive_match() {
        assert!(match_wildcard("hello", "hello"));
        assert!(match_wildcard("Hello", "hELLo"));
        assert!(!match_wildcard("hello world", "hello"));
        assert!(!match_wildcard("say hello", "hello"));
        assert!(!match_wildcard("", "hello"));
        assert!(match_wildcard("", ""));
    }

    #[test]
    fn rule_engine_wildcard_suffix_must_not_overlap_consumed_prefix() {
        assert!(!match_wildcard("ab", "ab*b"));
        assert!(match_wildcard("abb", "ab*b"));
        assert!(!match_wildcard("a", "a*a"));
        assert!(match_wildcard("aa", "a*a"));
    }

    #[test]
    fn rule_engine_wildcard_lone_star_matches_everything() {
        assert!(match_wildcard("", "*"));
        assert!(match_wildcard("anything at all", "*"));
    }

    #[test]
    fn rule_engine_wildcard_middle_parts_must_appear_in_order() {
        assert!(match_wildcard("a-b-c", "a*b*c"));
        assert!(!match_wildcard("a-c-b", "a*b*c"));
    }

    #[test]
    fn rule_engine_wildcard_consecutive_stars_are_tolerated() {
        assert!(match_wildcard("abc", "a**c"));
    }

    // ----- boolean fields -----

    #[test]
    fn rule_engine_is_read_matches_bool_value() {
        let unread = criterion("is_read", "equals", json!(false));
        let read = criterion("is_read", "equals", json!(true));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![unread])));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![read])));
    }

    #[test]
    fn rule_engine_has_attachments_matches_bool_value() {
        let with = criterion("has_attachments", "equals", json!(true));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![with])));
    }

    #[test]
    fn rule_engine_boolean_field_with_non_bool_value_does_not_match() {
        let c = criterion("is_read", "equals", json!("false"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    // ----- date operators -----

    #[test]
    fn rule_engine_date_before_is_strict() {
        assert!(match_date_criterion(TEST_DATE, &criterion("date", "before", json!(TEST_DATE + 1))));
        assert!(!match_date_criterion(TEST_DATE, &criterion("date", "before", json!(TEST_DATE))));
    }

    #[test]
    fn rule_engine_date_after_is_strict() {
        assert!(match_date_criterion(TEST_DATE, &criterion("date", "after", json!(TEST_DATE - 1))));
        assert!(!match_date_criterion(TEST_DATE, &criterion("date", "after", json!(TEST_DATE))));
    }

    #[test]
    fn rule_engine_date_between_is_inclusive() {
        let c = criterion("date", "between", json!([TEST_DATE, TEST_DATE]));
        assert!(match_date_criterion(TEST_DATE, &c));
        let outside = criterion("date", "between", json!([TEST_DATE + 1, TEST_DATE + 10]));
        assert!(!match_date_criterion(TEST_DATE, &outside));
    }

    #[test]
    fn rule_engine_date_between_requires_exactly_two_bounds() {
        let one = criterion("date", "between", json!([TEST_DATE - 1]));
        let three = criterion("date", "between", json!([0, TEST_DATE, i64::MAX]));
        assert!(!match_date_criterion(TEST_DATE, &one));
        assert!(!match_date_criterion(TEST_DATE, &three));
    }

    #[test]
    fn rule_engine_date_with_non_numeric_value_does_not_match() {
        assert!(!match_date_criterion(TEST_DATE, &criterion("date", "before", json!("yesterday"))));
    }

    #[test]
    fn rule_engine_date_field_dispatches_through_evaluate() {
        let c = criterion("date", "after", json!(TEST_DATE - 100));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    // ----- negate -----

    #[test]
    fn rule_engine_negate_inverts_a_match() {
        let c = negated(criterion("subject", "contains", json!("newsletter")));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_negate_inverts_a_non_match() {
        let c = negated(criterion("subject", "contains", json!("python")));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    #[test]
    fn rule_engine_negated_unknown_field_becomes_true() {
        // Documents current behaviour: an unknown field evaluates to false,
        // and negation turns that into a match.
        let c = negated(criterion("nonexistent", "contains", json!("x")));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![c])));
    }

    // ----- combinators -----

    #[test]
    fn rule_engine_all_mode_requires_every_criterion() {
        let hit = criterion("subject", "contains", json!("newsletter"));
        let miss = criterion("from", "equals", json!("nobody@nowhere.org"));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("all", vec![hit.clone(), miss])));
        let hit2 = criterion("has_attachments", "equals", json!(true));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("all", vec![hit, hit2])));
    }

    #[test]
    fn rule_engine_any_mode_requires_at_least_one_criterion() {
        let hit = criterion("subject", "contains", json!("newsletter"));
        let miss = criterion("from", "equals", json!("nobody@nowhere.org"));
        assert!(evaluate_email_against_rule(&sample_email(), &rule("any", vec![miss.clone(), hit])));
        assert!(!evaluate_email_against_rule(&sample_email(), &rule("any", vec![miss.clone(), miss])));
    }
}
