use std::io;
use std::path::Path;

use anyhow::{Context, bail};
use clap::Args;

use websh_core::mempool::{
    ComposeError, ComposeForm, LEDGER_CATEGORIES, form_to_payload, serialize_mempool_file,
    slug_from_title, validate_form,
};
use websh_core::support::format::format_date_iso;

use crate::CliResult;
use crate::infra::gh::{GhResourceStatus, require_gh};
use crate::infra::time::current_timestamp;

use crate::workflows::mempool::mount::read_mempool_mount_declaration;
use crate::workflows::mempool::path::MempoolEntryPath;
use crate::workflows::mempool::remote::{add_to_mempool_via_gh, gh_path_status};

#[derive(Args)]
pub(super) struct AddArgs {
    /// Category segment (e.g., `writing`, `papers`). Must be one of
    /// LEDGER_CATEGORIES.
    #[arg(long)]
    category: String,
    /// Slug. Defaults to a kebab-case derivation of `--title` if omitted.
    #[arg(long)]
    slug: Option<String>,
    /// Frontmatter title.
    #[arg(long)]
    title: String,
    /// Frontmatter status. `draft` or `review`.
    #[arg(long, default_value = "draft")]
    status: String,
    /// Frontmatter priority (`low`, `med`, `high`). Omitted if absent.
    #[arg(long)]
    priority: Option<String>,
    /// Frontmatter tags, comma-separated. Empty / absent → no tags.
    #[arg(long, default_value = "")]
    tags: String,
    /// Modified date, `YYYY-MM-DD`. Defaults to today.
    #[arg(long)]
    modified: Option<String>,
    /// Path to a file containing the markdown body, or `-` to read from stdin.
    #[arg(long)]
    body: String,
}

pub(super) fn add(root: &Path, args: AddArgs) -> CliResult {
    let mount = read_mempool_mount_declaration(root)?;
    require_gh()?;

    let body = read_body_source(&args.body)?;
    let form = build_form(&args, &body)?;

    let errors = validate_form(&form);
    if !errors.is_empty() {
        let messages: Vec<String> = errors.iter().map(humanize_compose_error).collect();
        bail!("invalid input:\n  - {}", messages.join("\n  - "));
    }

    let repo_path = format!("{}/{}.md", form.category, form.slug);
    let entry_path = MempoolEntryPath::parse(&repo_path)
        .with_context(|| format!("invalid mempool entry path `{repo_path}`"))?;
    match gh_path_status(&mount, entry_path.as_str())? {
        GhResourceStatus::Exists => {
            bail!(
                "{} already exists in {}@{} — pass a different --slug or edit via the browser",
                entry_path,
                mount.repo,
                mount.branch
            );
        }
        GhResourceStatus::Missing => {}
    }

    let file_body = serialize_mempool_file(&form_to_payload(&form));

    eprintln!("preflight: ok ({}/{})", form.category, form.slug);
    eprintln!("write:     {} ({} bytes)", entry_path, file_body.len());

    add_to_mempool_via_gh(&mount, entry_path.as_str(), &file_body)?;

    println!(
        "mempool add: {} → {}@{}",
        entry_path, mount.repo, mount.branch
    );
    Ok(())
}

/// Read the markdown body from `--body` argument: `-` means stdin, anything
/// else is a filesystem path.
fn read_body_source(spec: &str) -> CliResult<String> {
    if spec == "-" {
        let mut buf = String::new();
        io::Read::read_to_string(&mut io::stdin(), &mut buf).context("read body from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(spec).with_context(|| format!("read body from {spec}"))
    }
}

/// Build a `ComposeForm` from the parsed CLI args. Auto-derives slug from
/// title when `--slug` is omitted, defaults `modified` to today.
fn build_form(args: &AddArgs, body: &str) -> CliResult<ComposeForm> {
    let slug = args
        .slug
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slug_from_title(&args.title));

    let modified = args
        .modified
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format_date_iso(current_timestamp() / 1000));

    let tags: Vec<String> = args
        .tags
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();

    let priority = args.priority.clone().filter(|s| !s.is_empty());

    Ok(ComposeForm {
        title: args.title.trim().to_string(),
        category: args.category.clone(),
        slug,
        status: args.status.clone(),
        modified,
        priority,
        tags,
        body: body.to_string(),
    })
}

/// Translate a single `ComposeError` into a CLI-friendly message. Mirrors the
/// browser's compose modal field-error text where relevant.
fn humanize_compose_error(err: &ComposeError) -> String {
    match err {
        ComposeError::TitleEmpty => "title is required".to_string(),
        ComposeError::TitleHasReservedChars => {
            "title cannot contain \" \\ : or newlines".to_string()
        }
        ComposeError::SlugInvalid => {
            "slug must be kebab-case ASCII (a-z, 0-9, hyphens)".to_string()
        }
        ComposeError::StatusUnknown => "status must be `draft` or `review`".to_string(),
        ComposeError::ModifiedNotIso => "modified must be YYYY-MM-DD".to_string(),
        ComposeError::CategoryUnknown => {
            format!("category must be one of {}", LEDGER_CATEGORIES.join(", "))
        }
        ComposeError::PriorityUnknown => "priority must be `low`, `med`, or `high`".to_string(),
        ComposeError::TagHasReservedChars => {
            "tags cannot contain `[ ] \" ,` or newlines".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_add_args() -> AddArgs {
        AddArgs {
            category: "writing".into(),
            slug: None,
            title: "On writing slow".into(),
            status: "draft".into(),
            priority: None,
            tags: String::new(),
            modified: Some("2026-04-28".into()),
            body: "/dev/null".into(),
        }
    }

    #[test]
    fn build_form_auto_derives_slug_from_title() {
        let args = sample_add_args();
        let form = build_form(&args, "body").unwrap();
        assert_eq!(form.slug, "on-writing-slow");
        assert_eq!(form.category, "writing");
        assert_eq!(form.title, "On writing slow");
        assert_eq!(form.modified, "2026-04-28");
        assert_eq!(form.status, "draft");
        assert!(form.priority.is_none());
        assert!(form.tags.is_empty());
        assert_eq!(form.body, "body");
    }

    #[test]
    fn build_form_uses_explicit_slug_when_set() {
        let mut args = sample_add_args();
        args.slug = Some("custom-slug".into());
        let form = build_form(&args, "").unwrap();
        assert_eq!(form.slug, "custom-slug");
    }

    #[test]
    fn build_form_parses_comma_separated_tags() {
        let mut args = sample_add_args();
        args.tags = "essay, slow ,zk".into();
        let form = build_form(&args, "").unwrap();
        assert_eq!(form.tags, vec!["essay", "slow", "zk"]);
    }

    #[test]
    fn build_form_drops_empty_tags() {
        let mut args = sample_add_args();
        args.tags = ", , ".into();
        let form = build_form(&args, "").unwrap();
        assert!(form.tags.is_empty());
    }

    #[test]
    fn build_form_normalizes_priority() {
        let mut args = sample_add_args();
        args.priority = Some("med".into());
        let form = build_form(&args, "").unwrap();
        assert_eq!(form.priority.as_deref(), Some("med"));

        args.priority = Some(String::new());
        let form = build_form(&args, "").unwrap();
        assert!(form.priority.is_none());
    }

    #[test]
    fn validate_form_rejects_form_built_from_bad_args() {
        // Empty title → form has empty title → validate_form flags it.
        let mut args = sample_add_args();
        args.title = String::new();
        let form = build_form(&args, "").unwrap();
        let errs = validate_form(&form);
        assert!(errs.iter().any(|e| matches!(e, ComposeError::TitleEmpty)));
    }

    #[test]
    fn validate_form_rejects_unknown_category() {
        let mut args = sample_add_args();
        args.category = "fiction".into();
        // slug is auto-derived (form will pass slug validation), so the
        // only failure should be category.
        let form = build_form(&args, "").unwrap();
        let errs = validate_form(&form);
        assert!(
            errs.iter()
                .any(|e| matches!(e, ComposeError::CategoryUnknown))
        );
    }

    #[test]
    fn validate_form_rejects_invalid_modified() {
        let mut args = sample_add_args();
        args.modified = Some("April 28".into());
        let form = build_form(&args, "").unwrap();
        let errs = validate_form(&form);
        assert!(
            errs.iter()
                .any(|e| matches!(e, ComposeError::ModifiedNotIso))
        );
    }

    #[test]
    fn humanize_compose_error_covers_every_variant() {
        let variants = [
            ComposeError::TitleEmpty,
            ComposeError::TitleHasReservedChars,
            ComposeError::SlugInvalid,
            ComposeError::StatusUnknown,
            ComposeError::ModifiedNotIso,
            ComposeError::CategoryUnknown,
            ComposeError::PriorityUnknown,
            ComposeError::TagHasReservedChars,
        ];
        for v in variants {
            let msg = humanize_compose_error(&v);
            assert!(!msg.is_empty(), "variant {:?} produced empty message", v);
        }
    }
}
