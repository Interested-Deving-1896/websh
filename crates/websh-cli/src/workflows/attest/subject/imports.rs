use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use websh_core::attestation::artifact::{Attestation, message_sha256};
use websh_core::crypto::eth::verify_personal_sign;
use websh_site::ATTESTATIONS_PATH;

use crate::CliResult;
use crate::infra::json::write_json;
use crate::workflows::content::{artifact_path, resolve_path};

use super::super::gpg::{pgp_signer_from_key, verify_pgp_signature};
use super::artifact::read_artifact;

pub(super) fn pgp_import(
    root: &Path,
    route: String,
    signature: PathBuf,
    key: PathBuf,
    signer: Option<String>,
) -> CliResult {
    let mut artifact = read_artifact(root)?;
    artifact.validate_header()?;
    let subject = artifact
        .subject_for_route(&route)
        .ok_or_else(|| anyhow!("attestation subject not found for route {route}"))?
        .clone();
    subject.validate()?;
    let message = subject.canonical_message()?;

    let signature_path = resolve_path(root, &signature);
    let signature_body = std::fs::read_to_string(&signature_path)
        .with_context(|| format!("read {}", signature_path.display()))?;
    let fingerprint = verify_pgp_signature(root, &key, &signature_body, &message)?;
    let signer = signer.or_else(|| pgp_signer_from_key(root, &key).ok().flatten());
    let message_hash = message_sha256(&message);
    let key_path = artifact_path(root, &key)?;
    let signature_path = artifact_path(root, &signature).ok();

    let subject = artifact
        .subject_for_route_mut(&route)
        .expect("subject exists after immutable lookup");
    subject
        .attestations_mut()
        .retain(|attestation| !matches!(attestation, Attestation::Pgp { .. }));
    subject.attestations_mut().push(Attestation::Pgp {
        signer,
        fingerprint,
        key_path,
        signature: signature_body,
        signature_path,
        message_sha256: message_hash,
        verified: true,
    });

    write_json(&root.join(ATTESTATIONS_PATH), &artifact)?;
    println!("pgp: ok {route}");
    Ok(())
}

pub(super) fn eth_import(
    root: &Path,
    route: String,
    address: String,
    signature: String,
    signer: String,
) -> CliResult {
    let mut artifact = read_artifact(root)?;
    artifact.validate_header()?;
    let subject = artifact
        .subject_for_route(&route)
        .ok_or_else(|| anyhow!("attestation subject not found for route {route}"))?
        .clone();
    subject.validate()?;
    let message = subject.canonical_message()?;

    let verification = verify_personal_sign(&address, &message, &signature)?;
    let message_hash = message_sha256(&message);
    let subject = artifact
        .subject_for_route_mut(&route)
        .expect("subject exists after immutable lookup");
    subject.attestations_mut().retain(|attestation| {
        !matches!(attestation, Attestation::Ethereum { address: stored, .. } if stored.eq_ignore_ascii_case(&verification.expected_address))
    });
    subject.attestations_mut().push(Attestation::Ethereum {
        scheme: "eip191-personal-sign".to_string(),
        signer,
        address: verification.expected_address,
        signature,
        recovered_address: verification.recovered_address,
        message_sha256: message_hash,
        verified: true,
    });

    write_json(&root.join(ATTESTATIONS_PATH), &artifact)?;
    println!("ethereum: ok {route}");
    Ok(())
}
