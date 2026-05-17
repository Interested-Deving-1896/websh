//! Path conventions for `/mempool/<category>/<slug>.md` and helpers for
//! deriving the canonical save path from a user-typed draft.

use std::sync::LazyLock;

use crate::domain::VirtualPath;

use super::categories::LEDGER_CATEGORIES;
use super::parse::parse_mempool_frontmatter;
use super::serialize::slug_from_title;

static MEMPOOL_ROOT: LazyLock<VirtualPath> =
    LazyLock::new(|| VirtualPath::from_absolute("/mempool").expect("mempool root is absolute"));

pub fn mempool_root() -> &'static VirtualPath {
    &MEMPOOL_ROOT
}

/// Characters in a `title` whose parsed value cannot survive the naive
/// `parse_mempool_frontmatter` round-trip. `derive_new_path` rejects them
/// so the user gets a clear error rather than a silently-wrong slug.
const TITLE_RESERVED: &[char] = &['"', '\\', '\n', '\r', ':'];

/// YAML frontmatter placeholder for the `/new` compose flow. The `today`
/// argument is injected so unit tests are deterministic; edge callers pass
/// their own wall-clock date.
///
/// The placeholder's `category` is `LEDGER_CATEGORIES[0]` so it stays in
/// sync with the default the CLI form derivation would pick.
pub fn placeholder_frontmatter(today: &str) -> String {
    let category = LEDGER_CATEGORIES[0];
    format!(
        "---\n\
         title: \"\"\n\
         category: {category}\n\
         status: draft\n\
         modified: {today}\n\
         ---\n\n"
    )
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MempoolPathError {
    #[error("frontmatter is missing the leading `---` fence")]
    MissingFrontmatter,
    #[error("title is required")]
    MissingTitle,
    #[error("title cannot contain \" \\ : or newlines")]
    TitleHasReservedChars,
    #[error("category is required")]
    MissingCategory,
    #[error("category must be one of: {allowed}")]
    UnknownCategory { allowed: String },
    #[error("title must produce a non-empty slug")]
    EmptySlug,
    #[error("cannot build path: {message}")]
    InvalidPath { message: String },
}

impl From<crate::domain::VirtualPathParseError> for MempoolPathError {
    fn from(source: crate::domain::VirtualPathParseError) -> Self {
        Self::InvalidPath {
            message: source.to_string(),
        }
    }
}

/// Parse `raw_body`'s frontmatter and derive the canonical save path for
/// a new mempool draft. Returns the human-readable error string the page
/// surfaces in `save_error`.
///
/// Contract:
/// - title required; trimmed; no [`TITLE_RESERVED`] chars.
/// - category required; ∈ [`LEDGER_CATEGORIES`].
/// - explicit `slug:` is ignored — slug is derived from title via
///   [`slug_from_title`].
pub fn derive_new_path(raw_body: &str) -> Result<VirtualPath, MempoolPathError> {
    let meta = parse_mempool_frontmatter(raw_body).ok_or(MempoolPathError::MissingFrontmatter)?;
    let title = meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(MempoolPathError::MissingTitle)?;
    if title.chars().any(|c| TITLE_RESERVED.contains(&c)) {
        return Err(MempoolPathError::TitleHasReservedChars);
    }
    let category = meta
        .category
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or(MempoolPathError::MissingCategory)?;
    if !LEDGER_CATEGORIES.contains(&category) {
        return Err(MempoolPathError::UnknownCategory {
            allowed: LEDGER_CATEGORIES.join(", "),
        });
    }
    let slug = slug_from_title(title);
    if slug.is_empty() {
        return Err(MempoolPathError::EmptySlug);
    }
    VirtualPath::from_absolute(format!("/mempool/{category}/{slug}.md")).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(title: &str, category: &str) -> String {
        format!("---\ntitle: \"{title}\"\ncategory: {category}\n---\nbody\n")
    }

    #[test]
    fn happy_path_writes_expected_path() {
        let raw = body("On writing slow", "writing");
        let path = derive_new_path(&raw).expect("ok");
        assert_eq!(path.as_str(), "/mempool/writing/on-writing-slow.md");
    }

    #[test]
    fn rejects_missing_frontmatter_fence() {
        let raw = "no frontmatter here\n";
        assert_eq!(
            derive_new_path(raw).unwrap_err(),
            MempoolPathError::MissingFrontmatter
        );
    }

    #[test]
    fn rejects_empty_title() {
        let raw = body("", "writing");
        assert_eq!(
            derive_new_path(&raw).unwrap_err(),
            MempoolPathError::MissingTitle
        );
    }

    #[test]
    fn rejects_title_with_dangerous_chars_in_value() {
        for bad in ['"', '\\', ':'] {
            let raw = format!("---\ntitle: hello{bad}world\ncategory: writing\n---\n");
            let err = derive_new_path(&raw).unwrap_err();
            assert_eq!(err, MempoolPathError::TitleHasReservedChars);
        }
    }

    #[test]
    fn rejects_missing_category() {
        let raw = "---\ntitle: x\n---\n";
        assert_eq!(
            derive_new_path(raw).unwrap_err(),
            MempoolPathError::MissingCategory
        );
    }

    #[test]
    fn rejects_unknown_category() {
        let raw = body("hello", "blog");
        let err = derive_new_path(&raw).unwrap_err();
        assert!(matches!(err, MempoolPathError::UnknownCategory { .. }));
    }

    #[test]
    fn ignores_explicit_slug_key() {
        let raw = "---\ntitle: \"Hello World\"\ncategory: writing\nslug: ignored\n---\n";
        let path = derive_new_path(raw).expect("ok");
        assert_eq!(path.as_str(), "/mempool/writing/hello-world.md");
    }

    #[test]
    fn placeholder_round_trips_through_parser() {
        let placeholder = placeholder_frontmatter("2026-04-29");
        let meta = parse_mempool_frontmatter(&placeholder).expect("placeholder must parse cleanly");
        assert_eq!(meta.category.as_deref(), Some(LEDGER_CATEGORIES[0]));
        assert_eq!(meta.modified.as_deref(), Some("2026-04-29"));
        assert_eq!(meta.status.as_deref(), Some("draft"));
    }

    #[test]
    fn placeholder_category_matches_default() {
        let placeholder = placeholder_frontmatter("2026-01-01");
        assert!(placeholder.contains(&format!("category: {}", LEDGER_CATEGORIES[0])));
    }

    #[test]
    fn placeholder_can_be_completed_into_a_valid_save() {
        let placeholder = placeholder_frontmatter("2026-04-29");
        let filled = placeholder.replace("title: \"\"", "title: \"My First Draft\"");
        let path = derive_new_path(&filled).expect("ok");
        assert_eq!(path.as_str(), "/mempool/writing/my-first-draft.md");
    }
}
