use std::path::Path;

use anyhow::{anyhow, bail};
use websh_core::attestation::artifact::{Attestation, Subject, message_sha256};
use websh_core::crypto::pgp::{normalize_fingerprint, pretty_fingerprint};
use websh_site::ATTESTATIONS_PATH;

use crate::CliResult;
use crate::infra::json::write_json;

use super::gpg::{
    gpg_secret_key_fingerprint, pgp_fingerprint_from_key, sign_subject_with_gpg,
    verify_pgp_signature,
};
use super::subject::read_artifact;

pub(super) fn sign_missing_pgp_attestations(
    root: &Path,
    key: &Path,
    gpg_key: Option<&str>,
    signature_dir: &Path,
) -> CliResult<usize> {
    let mut artifact = read_artifact(root)?;
    artifact.validate_header()?;
    let routes = artifact
        .subjects
        .iter()
        .map(|subject| subject.route().to_string())
        .collect::<Vec<_>>();

    // Determine up-front whether any subject actually needs a new signature.
    // No work pending → skip the gpg probe and the fingerprint guard so an
    // attest-only build never invokes gpg unnecessarily.
    let mut pending_routes = Vec::new();
    for route in &routes {
        let subject = artifact
            .subject_for_route(route)
            .ok_or_else(|| anyhow!("attestation subject not found for route {route}"))?;
        subject.validate()?;
        if !subject_has_valid_pgp(root, subject)? {
            pending_routes.push(route.clone());
        }
    }
    if pending_routes.is_empty() {
        return Ok(0);
    }

    // gpg detection: missing binary or absent secret key on a release build
    // must not fail trunk. Warn and leave subjects pending so the build
    // produces a dist with `pending` markers — author re-signs later.
    let Some(active_fingerprint) = gpg_secret_key_fingerprint(gpg_key) else {
        println!(
            "attest: gpg unavailable or signer key not in keyring; \
             {} subject(s) left pending",
            pending_routes.len()
        );
        return Ok(0);
    };

    // Fingerprint guard: refuse to sign with a key that isn't the project
    // identity. Protects forks / co-authors from accidentally writing
    // attestations under their own keys.
    let expected_fingerprint = pgp_fingerprint_from_key(root, key)?;
    if normalize_fingerprint(&active_fingerprint) != expected_fingerprint {
        bail!(
            "attest: active gpg key fingerprint does not match the supplied public key.\n  \
             active:   {active}\n  \
             expected: {expected}\n  \
            Refusing to sign with a non-author key. Set WEBSH_NO_SIGN=1 to build without signing.",
            active = pretty_fingerprint(&active_fingerprint),
            expected = pretty_fingerprint(&expected_fingerprint),
        );
    }

    let mut signed = 0usize;
    for route in pending_routes {
        let subject = artifact
            .subject_for_route(&route)
            .expect("pending route survives the artifact roundtrip")
            .clone();

        let attestation = sign_subject_with_gpg(root, &subject, key, gpg_key, signature_dir)?;
        let subject = artifact
            .subject_for_route_mut(&route)
            .expect("subject exists after immutable lookup");
        subject
            .attestations_mut()
            .retain(|attestation| !matches!(attestation, Attestation::Pgp { .. }));
        subject.attestations_mut().push(attestation);
        signed += 1;
    }

    if signed > 0 {
        write_json(&root.join(ATTESTATIONS_PATH), &artifact)?;
    }
    Ok(signed)
}

fn subject_has_valid_pgp(root: &Path, subject: &Subject) -> CliResult<bool> {
    let message = subject.canonical_message()?;
    let message_hash = message_sha256(&message);
    Ok(subject.attestations().iter().any(|attestation| {
        let Attestation::Pgp {
            fingerprint,
            key_path,
            signature,
            message_sha256,
            verified,
            ..
        } = attestation
        else {
            return false;
        };
        *verified
            && message_sha256 == &message_hash
            && verify_pgp_signature(root, Path::new(key_path), signature, &message)
                .map(|verified_fingerprint| {
                    verified_fingerprint == normalize_fingerprint(fingerprint)
                })
                .unwrap_or(false)
    }))
}
