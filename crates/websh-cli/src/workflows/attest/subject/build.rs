use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use websh_core::attestation::artifact::{
    AttestationArtifact, BundleSubject, ContentFile, DocumentSubject, Envelope, HomepageSubject,
    LedgerSubject, PageSubject, Subject,
};
use websh_core::attestation::ledger::ContentLedger;
use websh_site::PUBLIC_KEY_PATH;

use crate::CliResult;
use crate::workflows::content::{build_content_files, collect_files_recursive};

use super::super::{DEFAULT_HOMEPAGE_CONTENT, today_utc};
use super::artifact::read_ack;
use super::types::SubjectKind;

pub(in crate::workflows::attest) fn build_subject(
    root: &Path,
    existing: &AttestationArtifact,
    route: String,
    kind: SubjectKind,
    content_paths: Vec<PathBuf>,
    issued_at: Option<String>,
    ledger: Option<&ContentLedger>,
) -> CliResult<Subject> {
    let content_files = build_content_files(root, &content_paths)?;
    let prior = existing.subject_for_route(&route);

    if issued_at.is_none()
        && let Some(prior) = prior
    {
        let subject = build_unattested_subject(
            root,
            kind,
            route.clone(),
            prior.issued_at().to_string(),
            content_files.clone(),
            ledger,
        )?;
        if subject_matches_prior(prior, &subject) {
            return Ok(with_prior_attestations(subject, prior));
        }
    }

    let issued_at = issued_at.unwrap_or_else(today_utc);
    let subject = build_unattested_subject(root, kind, route, issued_at, content_files, ledger)?;

    Ok(if let Some(prior) = prior {
        if subject_matches_prior(prior, &subject) {
            with_prior_attestations(subject, prior)
        } else {
            subject
        }
    } else {
        subject
    })
}

fn build_unattested_subject(
    root: &Path,
    kind: SubjectKind,
    route: String,
    issued_at: String,
    content_files: Vec<ContentFile>,
    ledger: Option<&ContentLedger>,
) -> CliResult<Subject> {
    let env = Envelope {
        route,
        issued_at,
        content_files,
        attestations: Vec::new(),
    };

    let subject = match kind {
        SubjectKind::Homepage => {
            let ack = read_ack(root)?;
            Subject::Homepage(HomepageSubject {
                env,
                ack_combined_root: ack.combined_root,
            })
        }
        SubjectKind::Ledger => {
            let ledger =
                ledger.context("ledger subject requires a ContentLedger to bind chain_head")?;
            Subject::Ledger(LedgerSubject {
                env,
                chain_head: ledger.chain_head.clone(),
            })
        }
        SubjectKind::Document => Subject::Document(DocumentSubject { env }),
        SubjectKind::Page => Subject::Page(PageSubject { env }),
        SubjectKind::Bundle => Subject::Bundle(BundleSubject { env }),
    };

    Ok(subject)
}

fn subject_matches_prior(prior: &Subject, subject: &Subject) -> bool {
    match (prior.canonical_message(), subject.canonical_message()) {
        (Ok(prior_msg), Ok(new_msg)) => prior_msg == new_msg,
        _ => false,
    }
}

fn with_prior_attestations(mut subject: Subject, prior: &Subject) -> Subject {
    subject
        .attestations_mut()
        .extend(prior.attestations().iter().cloned());
    subject
}

pub(in crate::workflows::attest) fn content_paths_or_default(
    root: &Path,
    route: &str,
    kind: SubjectKind,
    paths: Vec<PathBuf>,
) -> CliResult<Vec<PathBuf>> {
    let raw = if paths.is_empty() {
        if !matches!(kind, SubjectKind::Homepage) || route != "/" {
            bail!("non-homepage subjects require at least one --content path");
        }
        let mut defaults = DEFAULT_HOMEPAGE_CONTENT
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if root.join(PUBLIC_KEY_PATH).exists() {
            defaults.push(PathBuf::from(PUBLIC_KEY_PATH));
        }
        defaults
    } else {
        paths
    };
    expand_content_paths(root, raw)
}

/// Expand `paths` so each directory entry is replaced by the recursive list
/// of files it contains. File entries pass through unchanged. Order is
/// preserved across the input list, with files inside an expanded directory
/// emitted in the canonical sort order produced by
/// `manifest::collect_files_recursive`. Duplicates (same canonical
/// filesystem location reached via multiple input paths) are dropped.
fn expand_content_paths(root: &Path, raw_paths: Vec<PathBuf>) -> CliResult<Vec<PathBuf>> {
    let mut seen = BTreeSet::new();
    let mut expanded = Vec::new();
    for path in raw_paths {
        let abs = if path.is_absolute() {
            path.clone()
        } else {
            root.join(&path)
        };
        if abs.is_dir() {
            let mut files = Vec::new();
            collect_files_recursive(&abs, &mut files)?;
            for file in files {
                let key = file.canonicalize().unwrap_or_else(|_| file.clone());
                if seen.insert(key) {
                    expanded.push(file);
                }
            }
        } else if abs.is_file() {
            let key = abs.canonicalize().unwrap_or_else(|_| abs.clone());
            if seen.insert(key) {
                expanded.push(path);
            }
        } else {
            bail!("attestation content path not found: {}", path.display());
        }
    }
    Ok(expanded)
}
