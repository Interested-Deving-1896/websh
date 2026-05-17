//! Compose form value type, validation, and payload conversion.
//!
//! `ComposeForm` is the structured shape the CLI `mempool add` subcommand
//! and (historically) the browser modal compose flow consume. The browser
//! reader's raw-textarea Save path bypasses `ComposeForm` and goes
//! directly through `parse::parse_mempool_frontmatter` →
//! `manifest_entry::build_mempool_manifest_state`.

use crate::support::format::iso_date_prefix;

use super::categories::LEDGER_CATEGORIES;
use super::serialize::ComposePayload;

const ALLOWED_STATUSES: &[&str] = &["draft", "review"];
const ALLOWED_PRIORITIES: &[&str] = &["low", "med", "high"];

/// Characters in a `title` that the simple quoted-string YAML serializer
/// cannot round-trip through `parse_mempool_frontmatter`. Validation
/// rejects them outright rather than risk silent corruption on save.
const TITLE_RESERVED: &[char] = &['"', '\\', '\n', '\r', ':'];

/// Characters in a single `tag` that break the inline-list shape
/// `tags: [a, b, c]`. Same validation rationale as titles.
const TAG_RESERVED: &[char] = &['[', ']', ',', '"', '\n', '\r'];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposeForm {
    pub title: String,
    pub category: String,
    pub slug: String,
    pub status: String,
    pub modified: String,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ComposeError {
    #[error("title is required")]
    TitleEmpty,
    #[error("title contains reserved characters")]
    TitleHasReservedChars,
    #[error(
        "slug must start with an ASCII letter or number and contain only ASCII letters, numbers, or `-`"
    )]
    SlugInvalid,
    #[error("status is unknown")]
    StatusUnknown,
    #[error("modified must be an ISO date")]
    ModifiedNotIso,
    #[error("category is unknown")]
    CategoryUnknown,
    #[error("priority is unknown")]
    PriorityUnknown,
    #[error("tag contains reserved characters")]
    TagHasReservedChars,
}

pub fn validate_form(form: &ComposeForm) -> Vec<ComposeError> {
    let mut errors = Vec::new();
    if form.title.trim().is_empty() {
        errors.push(ComposeError::TitleEmpty);
    } else if form.title.chars().any(|c| TITLE_RESERVED.contains(&c)) {
        errors.push(ComposeError::TitleHasReservedChars);
    }
    if !slug_is_valid(&form.slug) {
        errors.push(ComposeError::SlugInvalid);
    }
    if !ALLOWED_STATUSES.contains(&form.status.as_str()) {
        errors.push(ComposeError::StatusUnknown);
    }
    if iso_date_prefix(&form.modified).is_none() {
        errors.push(ComposeError::ModifiedNotIso);
    }
    if !LEDGER_CATEGORIES.contains(&form.category.as_str()) {
        errors.push(ComposeError::CategoryUnknown);
    }
    if let Some(priority) = &form.priority
        && !ALLOWED_PRIORITIES.contains(&priority.as_str())
    {
        errors.push(ComposeError::PriorityUnknown);
    }
    if form
        .tags
        .iter()
        .any(|tag| tag.chars().any(|c| TAG_RESERVED.contains(&c)))
    {
        errors.push(ComposeError::TagHasReservedChars);
    }
    errors
}

fn slug_is_valid(slug: &str) -> bool {
    if slug.is_empty() {
        return false;
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

pub fn form_to_payload(form: &ComposeForm) -> ComposePayload {
    ComposePayload {
        title: form.title.trim().to_string(),
        status: form.status.clone(),
        modified: form.modified.clone(),
        priority: form.priority.clone(),
        tags: form.tags.clone(),
        body: form.body.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(mutate: impl FnOnce(&mut ComposeForm)) -> ComposeForm {
        let mut form = ComposeForm {
            title: "foo".into(),
            category: "writing".into(),
            slug: "foo".into(),
            status: "draft".into(),
            modified: "2026-04-28".into(),
            priority: None,
            tags: vec![],
            body: "body".into(),
        };
        mutate(&mut form);
        form
    }

    #[test]
    fn validate_form_rejects_empty_title() {
        let payload = sample(|p| p.title.clear());
        let errors = validate_form(&payload);
        assert!(errors.iter().any(|e| matches!(e, ComposeError::TitleEmpty)));
    }

    #[test]
    fn validate_form_rejects_unknown_status() {
        let payload = sample(|p| p.status = "published".into());
        let errors = validate_form(&payload);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ComposeError::StatusUnknown))
        );
    }

    #[test]
    fn validate_form_rejects_invalid_modified_date() {
        let payload = sample(|p| p.modified = "April 28".into());
        let errors = validate_form(&payload);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ComposeError::ModifiedNotIso))
        );
    }

    #[test]
    fn validate_form_accepts_minimal_valid() {
        let payload = sample(|_| {});
        assert!(validate_form(&payload).is_empty());
    }

    #[test]
    fn validate_form_rejects_title_with_reserved_chars() {
        for bad in ['"', '\\', '\n', '\r', ':'] {
            let payload = sample(|p| p.title = format!("hello {bad} world"));
            let errs = validate_form(&payload);
            assert!(
                errs.contains(&ComposeError::TitleHasReservedChars),
                "expected TitleHasReservedChars for char {:?}; got {:?}",
                bad,
                errs
            );
        }
    }

    #[test]
    fn validate_form_rejects_unknown_priority() {
        let payload = sample(|p| p.priority = Some("urgent".into()));
        let errs = validate_form(&payload);
        assert!(errs.contains(&ComposeError::PriorityUnknown));
    }

    #[test]
    fn validate_form_accepts_known_priority_or_none() {
        for value in [
            None,
            Some("low".into()),
            Some("med".into()),
            Some("high".into()),
        ] {
            let payload = sample(|p| p.priority = value.clone());
            assert!(
                validate_form(&payload).is_empty(),
                "expected no errors for priority {:?}",
                value
            );
        }
    }

    #[test]
    fn validate_form_rejects_tags_with_reserved_chars() {
        let payload = sample(|p| p.tags = vec!["good".into(), "bad[tag]".into()]);
        let errs = validate_form(&payload);
        assert!(errs.contains(&ComposeError::TagHasReservedChars));
    }
}
