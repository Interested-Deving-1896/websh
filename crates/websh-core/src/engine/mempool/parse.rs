//! Frontmatter parsing for mempool authoring (compose / edit / promote).
//!
//! The parser is line-based and accepts only the keys mempool emits.
//! Writer (`serialize::serialize_mempool_file`) and reader stay in lockstep
//! so the round-trip is closed. Unknown YAML constructs (block scalars,
//! comments, multi-line lists) are deliberately not supported — if they
//! show up, the entry was hand-edited and the user should re-author via
//! the compose flow.

use crate::domain::VirtualPath;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawMempoolMeta {
    pub title: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub modified: Option<String>,
    pub tags: Vec<String>,
}

/// Parse mempool-file frontmatter. `None` when the input doesn't open with
/// a `---` fence; unknown keys are ignored; values are read as raw strings.
pub fn parse_mempool_frontmatter(body: &str) -> Option<RawMempoolMeta> {
    let mut lines = body.lines();
    if lines.next() != Some("---") {
        return None;
    }

    let mut meta = RawMempoolMeta::default();
    for line in lines {
        if line == "---" {
            return Some(meta);
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match key {
            "title" => meta.title = Some(value.to_string()),
            "category" => meta.category = Some(value.to_string()),
            "status" => meta.status = Some(value.to_string()),
            "priority" => meta.priority = Some(value.to_string()),
            "modified" => meta.modified = Some(value.to_string()),
            "tags" => meta.tags = parse_inline_tags(value),
            _ => {}
        }
    }
    Some(meta)
}

/// First path segment beneath `mempool_root`. Returns `"misc"` for files
/// directly under the root (no category folder).
pub fn category_for_mempool_path(path: &VirtualPath, mempool_root: &VirtualPath) -> String {
    let path_str = path.as_str();
    let prefix = mempool_root.as_str();
    let rel = path_str
        .strip_prefix(prefix)
        .unwrap_or(path_str)
        .trim_start_matches('/');
    let mut segments = rel.split('/');
    let first = segments.next().unwrap_or("");
    if segments.next().is_none() {
        return "misc".to_string();
    }
    if first.is_empty() {
        "misc".to_string()
    } else {
        first.to_string()
    }
}

/// Strip a leading `---\n...---\n` frontmatter block. Returns the remaining
/// body slice unchanged if no fence is present.
pub fn strip_frontmatter_block(body: &str) -> &str {
    let mut iter = body.splitn(3, "---\n");
    match (iter.next(), iter.next(), iter.next()) {
        (Some(""), Some(_meta), Some(rest)) => rest,
        _ => body,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MempoolFrontmatterError {
    #[error("promote: source body has no recognizable frontmatter")]
    MissingFrontmatter,
}

/// Mempool frontmatter → canonical `Fields`-shaped frontmatter.
/// Drops `status` / `priority` (mempool-only), renames `modified` → `date`.
/// Required because the canonical YAML parser rejects mempool-only keys.
pub fn transform_mempool_frontmatter(body: &str) -> Result<String, MempoolFrontmatterError> {
    let raw = parse_mempool_frontmatter(body).ok_or(MempoolFrontmatterError::MissingFrontmatter)?;
    let body_after = strip_frontmatter_block(body);

    let mut out = String::from("---\n");
    if let Some(title) = raw.title.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!(
            "title: \"{}\"\n",
            title.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    if let Some(date) = raw.modified.as_ref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("date: \"{date}\"\n"));
    }
    if !raw.tags.is_empty() {
        out.push_str(&format!("tags: [{}]\n", raw.tags.join(", ")));
    }
    out.push_str("---\n");
    if !body_after.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(body_after);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn parse_inline_tags(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if let Some(inner) = trimmed
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
    {
        return inner
            .split(',')
            .map(|tag| tag.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|tag| !tag.is_empty())
            .collect();
    }
    if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![trimmed.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn parses_full_frontmatter() {
        let raw = body(
            "---\n\
             title: \"On writing slow\"\n\
             category: writing\n\
             status: draft\n\
             priority: med\n\
             modified: \"2026-04-25\"\n\
             tags: [essay, writing-process]\n\
             ---\n\
             # On writing slow\n\nbody...\n",
        );
        let meta = parse_mempool_frontmatter(&raw).expect("parses");
        assert_eq!(meta.title.as_deref(), Some("On writing slow"));
        assert_eq!(meta.category.as_deref(), Some("writing"));
        assert_eq!(meta.status.as_deref(), Some("draft"));
        assert_eq!(meta.priority.as_deref(), Some("med"));
        assert_eq!(meta.modified.as_deref(), Some("2026-04-25"));
        assert_eq!(
            meta.tags,
            vec!["essay".to_string(), "writing-process".to_string()]
        );
    }

    #[test]
    fn parses_category_when_present() {
        let raw = body("---\ntitle: t\ncategory: papers\n---\n");
        let meta = parse_mempool_frontmatter(&raw).expect("parses");
        assert_eq!(meta.category.as_deref(), Some("papers"));
    }

    #[test]
    fn category_absent_returns_none() {
        let raw = body("---\ntitle: t\nstatus: draft\n---\n");
        let meta = parse_mempool_frontmatter(&raw).expect("parses");
        assert!(meta.category.is_none());
    }

    #[test]
    fn parses_minimal_frontmatter() {
        let raw = body("---\ntitle: foo\nstatus: draft\nmodified: 2026-04-22\n---\nbody\n");
        let meta = parse_mempool_frontmatter(&raw).expect("parses");
        assert_eq!(meta.title.as_deref(), Some("foo"));
        assert_eq!(meta.status.as_deref(), Some("draft"));
        assert!(meta.priority.is_none());
        assert_eq!(meta.modified.as_deref(), Some("2026-04-22"));
        assert!(meta.tags.is_empty());
    }

    #[test]
    fn returns_none_when_no_frontmatter_fence() {
        assert!(parse_mempool_frontmatter("# title\nbody\n").is_none());
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert!(parse_mempool_frontmatter("").is_none());
    }

    #[test]
    fn ignores_unknown_keys() {
        let raw =
            body("---\ntitle: foo\nstatus: draft\nmodified: 2026-04-22\nfuture: ignore\n---\n");
        let meta = parse_mempool_frontmatter(&raw).expect("parses");
        assert_eq!(meta.title.as_deref(), Some("foo"));
    }

    #[test]
    fn category_for_path_uses_first_segment_under_mempool() {
        let path = VirtualPath::from_absolute("/mempool/writing/foo.md").unwrap();
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        assert_eq!(category_for_mempool_path(&path, &mempool_root), "writing");
    }

    #[test]
    fn category_for_path_handles_root_level_files() {
        let path = VirtualPath::from_absolute("/mempool/loose.md").unwrap();
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        assert_eq!(category_for_mempool_path(&path, &mempool_root), "misc");
    }

    #[test]
    fn category_for_path_handles_nested_paths() {
        let path = VirtualPath::from_absolute("/mempool/papers/series/foo.md").unwrap();
        let mempool_root = VirtualPath::from_absolute("/mempool").unwrap();
        assert_eq!(category_for_mempool_path(&path, &mempool_root), "papers");
    }

    #[test]
    fn strip_frontmatter_block_drops_fence() {
        let raw = "---\ntitle: x\n---\n\nbody line\n";
        assert_eq!(strip_frontmatter_block(raw), "\nbody line\n");
    }

    #[test]
    fn strip_frontmatter_block_passes_through_when_no_fence() {
        let raw = "no fence here\nstill body\n";
        assert_eq!(strip_frontmatter_block(raw), raw);
    }

    #[test]
    fn transform_emits_canonical_keys_only() {
        let raw = "---\ntitle: x\nstatus: review\npriority: high\nmodified: 2026-04-25\ntags: [a, b]\n---\nbody\n";
        let out = transform_mempool_frontmatter(raw).expect("ok");
        assert!(out.contains("title: \"x\"\n"));
        assert!(out.contains("date: \"2026-04-25\"\n"));
        assert!(out.contains("tags: [a, b]\n"));
        assert!(!out.contains("status:"));
        assert!(!out.contains("priority:"));
        assert!(!out.contains("modified:"));
    }

    #[test]
    fn transform_returns_err_when_no_frontmatter() {
        assert!(transform_mempool_frontmatter("plain body").is_err());
    }
}
