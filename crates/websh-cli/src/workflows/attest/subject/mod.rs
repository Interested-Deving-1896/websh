use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use websh_core::crypto::ack::short_hash;
use websh_site::ATTESTATIONS_PATH;

use crate::CliResult;
use crate::infra::json::write_json;

mod artifact;
mod build;
mod imports;
mod types;

use artifact::upsert_subject;
pub(super) use artifact::{read_ack, read_artifact};
pub(super) use build::{build_subject, content_paths_or_default};
use imports::{eth_import, pgp_import};
pub(super) use types::{SubjectKind, SubjectSpec};

pub(crate) enum SubjectAction {
    Set {
        route: String,
        kind: String,
        content_paths: Vec<PathBuf>,
        issued_at: Option<String>,
    },
    Message {
        route: String,
    },
    PgpImport {
        route: String,
        signature: PathBuf,
        key: PathBuf,
        signer: Option<String>,
    },
    EthImport {
        route: String,
        address: String,
        signature: String,
        signer: String,
    },
}

pub(crate) fn subject(root: &Path, action: SubjectAction) -> CliResult {
    match action {
        SubjectAction::Set {
            route,
            kind,
            content_paths,
            issued_at,
        } => subject_set(root, route, kind, content_paths, issued_at),
        SubjectAction::Message { route } => subject_message(root, route),
        SubjectAction::PgpImport {
            route,
            signature,
            key,
            signer,
        } => pgp_import(root, route, signature, key, signer),
        SubjectAction::EthImport {
            route,
            address,
            signature,
            signer,
        } => eth_import(root, route, address, signature, signer),
    }
}

fn subject_set(
    root: &Path,
    route: String,
    kind: String,
    content_paths: Vec<PathBuf>,
    issued_at: Option<String>,
) -> CliResult {
    let kind = SubjectKind::parse(&kind)?;
    if matches!(kind, SubjectKind::Ledger) {
        bail!(
            "ledger subjects are only built by `attest`; chain_head depends on the regenerated ledger artifact"
        );
    }
    let content_paths = content_paths_or_default(root, &route, kind, content_paths)?;
    let mut artifact = read_artifact(root).unwrap_or_default();
    artifact.validate_header()?;
    let subject = build_subject(
        root,
        &artifact,
        route.clone(),
        kind,
        content_paths,
        issued_at,
        None,
    )?;
    upsert_subject(&mut artifact, subject);

    let path = root.join(ATTESTATIONS_PATH);
    write_json(&path, &artifact)?;
    let subject = artifact
        .subject_for_route(&route)
        .expect("subject just inserted");
    let content_sha = subject.content_sha256()?;
    println!("wrote {} {}", path.display(), short_hash(&content_sha));
    Ok(())
}

fn subject_message(root: &Path, route: String) -> CliResult {
    let artifact = read_artifact(root)?;
    artifact.validate_header()?;
    let subject = artifact
        .subject_for_route(&route)
        .ok_or_else(|| anyhow!("attestation subject not found for route {route}"))?;
    subject.validate()?;
    let message = subject.canonical_message()?;
    println!("{message}");
    Ok(())
}
