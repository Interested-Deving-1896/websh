use std::fs;
use std::path::Path;

use anyhow::Context;
use websh_core::domain::{Fields, NodeMetadata};

use crate::CliResult;

use super::sidecar::matching_file_sidecar;

/// Merge frontmatter-derived fields into the prior authored section
/// per-field: each field present in `frontmatter` wins; unmentioned
/// fields are preserved from `prior`. This protects user edits to the
/// sidecar that the markdown frontmatter doesn't speak to (e.g.
/// `access`, `route`, `trust`).
pub(crate) fn merge_authored(prior: Fields, frontmatter: Fields) -> Fields {
    Fields {
        title: frontmatter.title.or(prior.title),
        kind: frontmatter.kind.or(prior.kind),
        renderer: frontmatter.renderer.or(prior.renderer),
        route: frontmatter.route.or(prior.route),
        language: frontmatter.language.or(prior.language),
        description: frontmatter.description.or(prior.description),
        date: frontmatter.date.or(prior.date),
        tags: frontmatter.tags.or(prior.tags),
        links: frontmatter.links.or(prior.links),
        icon: frontmatter.icon.or(prior.icon),
        thumbnail: frontmatter.thumbnail.or(prior.thumbnail),
        sort: frontmatter.sort.or(prior.sort),
        trust: frontmatter.trust.or(prior.trust),
        access: frontmatter.access.or(prior.access),
        // The remaining fields are derive-only; frontmatter shouldn't
        // touch them, but we honor whatever it contains over `prior`
        // for symmetry.
        page_size: frontmatter.page_size.or(prior.page_size),
        page_count: frontmatter.page_count.or(prior.page_count),
        rotation: frontmatter.rotation.or(prior.rotation),
        image_dimensions: frontmatter.image_dimensions.or(prior.image_dimensions),
        size_bytes: frontmatter.size_bytes.or(prior.size_bytes),
        modified_at: frontmatter.modified_at.or(prior.modified_at),
        content_sha256: frontmatter.content_sha256.or(prior.content_sha256),
        word_count: frontmatter.word_count.or(prior.word_count),
        child_count: frontmatter.child_count.or(prior.child_count),
    }
}

/// Split a markdown body into `(yaml_str, body_after_fence)` if it opens
/// with a YAML frontmatter block. Recognizes both LF and CRLF line
/// endings, and anchors the closing `---` fence to the start of a line
/// so an inline `---` in the body content can't false-close the block.
fn split_yaml_frontmatter(body: &str) -> Option<(&str, &str)> {
    let after_open = body
        .strip_prefix("---\n")
        .or_else(|| body.strip_prefix("---\r\n"))?;
    // Find a closing fence at the start of a line. Accept `---` followed
    // by any line terminator or by EOF.
    let mut search_from = 0usize;
    while let Some(rel) = after_open[search_from..].find("\n---") {
        let abs = search_from + rel + 1; // index of '-' in '---'
        let end_of_yaml = abs - 1; // exclude the leading '\n'
        let after_fence = &after_open[abs + 3..];
        // The character right after '---' must be a newline (LF/CRLF) or EOF.
        let is_terminated = after_fence.is_empty()
            || after_fence.starts_with('\n')
            || after_fence.starts_with("\r\n")
            // Tolerate trailing whitespace on the fence line.
            || after_fence
                .chars()
                .next()
                .map(|c| c == ' ' || c == '\t')
                .unwrap_or(false);
        if is_terminated {
            let yaml = &after_open[..end_of_yaml];
            // Skip past one trailing line terminator after the fence.
            let body_rest = if let Some(rest) = after_fence.strip_prefix("\r\n") {
                rest
            } else if let Some(rest) = after_fence.strip_prefix('\n') {
                rest
            } else {
                // Trailing whitespace before terminator — skip until newline.
                after_fence
                    .find('\n')
                    .map(|i| &after_fence[i + 1..])
                    .unwrap_or("")
            };
            return Some((yaml, body_rest));
        }
        search_from = abs + 3;
    }
    None
}

pub(crate) fn parse_yaml_frontmatter(body: &str) -> CliResult<Option<Fields>> {
    let Some((yaml, _)) = split_yaml_frontmatter(body) else {
        return Ok(None);
    };
    parse_frontmatter_fields(yaml).map(Some)
}

fn parse_frontmatter_fields(yaml: &str) -> CliResult<Fields> {
    serde_norway::from_str(yaml).context("frontmatter YAML parse")
}

pub(crate) fn strip_yaml_frontmatter(body: &str) -> &str {
    split_yaml_frontmatter(body)
        .map(|(_, rest)| rest)
        .unwrap_or(body)
}

/// Resolve the human-authored content date for a file. Sidecar metadata
/// (if present) wins; markdown files without a sidecar fall back to YAML
/// frontmatter.
pub(crate) fn content_entry_raw_date(
    content_root: &Path,
    path: &Path,
    rel_path: &str,
) -> Option<String> {
    if let Some(sidecar) = matching_file_sidecar(content_root, rel_path)
        && let Ok(body) = fs::read_to_string(&sidecar)
        && let Ok(metadata) = serde_json::from_str::<NodeMetadata>(&body)
        && let Some(date) = metadata.date()
        && !date.trim().is_empty()
    {
        return Some(date.to_string());
    }
    // Fallback for markdown: read frontmatter directly.
    if rel_path.ends_with(".md")
        && let Ok(body) = fs::read_to_string(path)
        && let Ok(Some(fields)) = parse_yaml_frontmatter(&body)
        && let Some(date) = fields.date.filter(|d| !d.trim().is_empty())
    {
        return Some(date);
    }
    None
}

#[cfg(test)]
mod tests {
    use websh_core::domain::{NodeKind, RendererKind, TrustLevel};

    use super::*;

    #[test]
    fn parse_yaml_frontmatter_deserializes_supported_metadata() {
        let body = r#"---
title: A note
kind: page
renderer: markdown_page
description: |
  First line
  Second line
date: 2026-05-03
tags:
  - rust
  - yaml
links:
  - label: Paper
    url: https://eprint.iacr.org/2026/001
    kind: paper
trust: trusted
access:
  recipients:
    - address: "0xabc"
page_size:
  width: 612
  height: 792
---
# Body
"#;

        let fields = parse_yaml_frontmatter(body)
            .expect("frontmatter parses")
            .expect("frontmatter exists");

        assert_eq!(fields.title.as_deref(), Some("A note"));
        assert_eq!(fields.kind, Some(NodeKind::Page));
        assert_eq!(fields.renderer, Some(RendererKind::MarkdownPage));
        assert_eq!(
            fields.description.as_deref(),
            Some("First line\nSecond line\n")
        );
        assert_eq!(fields.date.as_deref(), Some("2026-05-03"));
        assert_eq!(
            fields.tags.as_deref(),
            Some(["rust".to_string(), "yaml".to_string()].as_slice())
        );
        let links = fields.links.as_deref().expect("links parsed");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].label, "Paper");
        assert_eq!(links[0].url, "https://eprint.iacr.org/2026/001");
        assert_eq!(links[0].kind.as_deref(), Some("paper"));
        assert_eq!(fields.trust, Some(TrustLevel::Trusted));
        assert_eq!(
            fields
                .access
                .as_ref()
                .and_then(|access| access.recipients.first())
                .map(|recipient| recipient.address.as_str()),
            Some("0xabc")
        );
        assert_eq!(
            fields.page_size.map(|page| (page.width, page.height)),
            Some((612, 792))
        );
    }

    #[test]
    fn parse_yaml_frontmatter_ignores_bodies_without_frontmatter() {
        assert!(
            parse_yaml_frontmatter("# Body\n")
                .expect("body without frontmatter is valid")
                .is_none()
        );
    }

    #[test]
    fn parse_yaml_frontmatter_reports_yaml_errors_with_context() {
        let err = parse_yaml_frontmatter("---\nunknown: value\n---\n")
            .expect_err("unknown fields are rejected");

        assert_eq!(err.to_string(), "frontmatter YAML parse");
        assert!(format!("{err:#}").contains("unknown field"));
    }
}
