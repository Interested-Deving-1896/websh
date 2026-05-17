//! Locale and language-tag helpers shared across routing and runtime code.

/// Normalize a browser/POSIX-style locale value to its language subtag.
///
/// Examples:
/// - `ko-KR` -> `ko`
/// - `ko_KR.UTF-8` -> `ko`
/// - ` en-US ` -> `en`
pub fn normalize_locale_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_codeset = trimmed.split('.').next().unwrap_or(trimmed);
    let without_modifier = without_codeset.split('@').next().unwrap_or(without_codeset);
    let language = without_modifier
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if language.len() >= 2 && language.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(language)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_browser_and_posix_locale_values() {
        assert_eq!(normalize_locale_tag("ko-KR").as_deref(), Some("ko"));
        assert_eq!(normalize_locale_tag("ko_KR.UTF-8").as_deref(), Some("ko"));
        assert_eq!(normalize_locale_tag(" en-US ").as_deref(), Some("en"));
        assert_eq!(normalize_locale_tag("C"), None);
        assert_eq!(normalize_locale_tag(""), None);
    }
}
