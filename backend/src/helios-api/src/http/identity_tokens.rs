use uuid::Uuid;

/// Normalize identity tokens for backend-side uniqueness checks.
///
/// Rules:
/// - Whitespace is trimmed; empty strings are treated as absent.
/// - If the token parses as a UUID, it is normalized to canonical lowercase form.
/// - Otherwise, it is normalized case-insensitively (ASCII lowercased) and sanitized to
///   characters safe for downstream identity segments (alphanumeric plus `-`/`_`/`.`).
///
/// This matches the API's intent that identity tokens (uuid/alias/hardware_id) must be unique
/// within a category, including "cross-field" collisions (e.g. an alias equal to another item's
/// uuid string).
pub(crate) fn normalize_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(uuid) = Uuid::parse_str(trimmed) {
        return Some(uuid.to_string());
    }
    let lower = trimmed.to_ascii_lowercase();
    let filtered: String = lower.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.').collect();
    if filtered.is_empty() { None } else { Some(filtered) }
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

    #[test]
    fn trims_and_rejects_empty() {
        assert_eq!(normalize_token("   "), None);
        assert_eq!(normalize_token("  abc  "), Some("abc".to_string()));
    }

    #[test]
    fn normalizes_uuid_tokens() {
        assert_eq!(normalize_token("550E8400-E29B-41D4-A716-446655440000"), Some("550e8400-e29b-41d4-a716-446655440000".to_string()));
    }

    #[test]
    fn normalizes_non_uuid_case_insensitively() {
        assert_eq!(normalize_token("CaM_01"), Some("cam_01".to_string()));
    }

    #[test]
    fn strips_disallowed_identity_chars() {
        assert_eq!(normalize_token(" Cam 01 "), Some("cam01".to_string()));
        assert_eq!(normalize_token("/Cam-01/"), Some("cam-01".to_string()));
        assert_eq!(normalize_token("cam#01"), Some("cam01".to_string()));
    }

    #[test]
    fn rejects_tokens_that_sanitize_to_empty() {
        assert_eq!(normalize_token("!!!"), None);
        assert_eq!(normalize_token("///"), None);
    }
}
