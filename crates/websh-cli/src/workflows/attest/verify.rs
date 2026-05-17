use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail};
use websh_core::attestation::artifact::{Attestation, Subject, message_sha256};
use websh_core::attestation::ledger::ContentLedger;
use websh_core::crypto::ack::short_hash;
use websh_core::crypto::eth::verify_personal_sign;
use websh_core::crypto::pgp::normalize_fingerprint;

use crate::CliResult;
use crate::infra::json::read_json;
use crate::workflows::content::build_content_files;

use super::gpg::verify_pgp_signature;
use super::subject::{read_ack, read_artifact};

pub(crate) fn verify(root: &Path, route: Option<String>) -> CliResult {
    let artifact = read_artifact(root)?;
    artifact.validate_header()?;
    if artifact.subjects.is_empty() {
        bail!("no attestation subjects");
    }

    if let Some(route) = route {
        let subject = artifact
            .subject_for_route(&route)
            .ok_or_else(|| anyhow!("attestation subject not found for route {route}"))?;
        verify_subject(root, subject)?;
        return Ok(());
    }

    for subject in &artifact.subjects {
        verify_subject(root, subject)?;
    }
    Ok(())
}

fn verify_subject(root: &Path, subject: &Subject) -> CliResult {
    subject.validate()?;

    let rebuilt = build_content_files(
        root,
        &subject
            .content_files()
            .iter()
            .map(|file| PathBuf::from(&file.path))
            .collect::<Vec<_>>(),
    )?;
    if rebuilt != subject.content_files() {
        bail!("content file metadata mismatch for {}", subject.id());
    }
    let content_sha256 = subject.content_sha256()?;

    match subject {
        Subject::Homepage(hp) => {
            let ack = read_ack(root)?;
            if ack.combined_root != hp.ack_combined_root {
                bail!("ACK root mismatch for {}", subject.id());
            }
        }
        Subject::Ledger(ls) => {
            let ledger_path = root.join(websh_core::attestation::ledger::CONTENT_LEDGER_PATH);
            let ledger: ContentLedger = read_json(&ledger_path)?;
            ledger.validate()?;
            if ledger.chain_head != ls.chain_head {
                bail!("chain_head mismatch for {}", subject.id());
            }
        }
        Subject::Document(_) | Subject::Page(_) | Subject::Bundle(_) => {}
    }

    let message = subject.canonical_message()?;
    let message_hash = message_sha256(&message);
    if subject.attestations().is_empty() {
        println!("{}: pending {}", subject.id(), short_hash(&content_sha256));
        return Ok(());
    }

    for attestation in subject.attestations() {
        if attestation.message_sha256() != message_hash {
            bail!("attestation message hash mismatch for {}", subject.id());
        }
        if !attestation.verified() {
            bail!("stored attestation is not verified for {}", subject.id());
        }

        match attestation {
            Attestation::Pgp {
                fingerprint,
                key_path,
                signature,
                ..
            } => {
                let verified_fingerprint =
                    verify_pgp_signature(root, Path::new(key_path), signature, &message)?;
                if normalize_fingerprint(fingerprint) != verified_fingerprint {
                    bail!("PGP fingerprint mismatch for {}", subject.id());
                }
                println!(
                    "{}: pgp ok {}",
                    subject.id(),
                    short_hash(&verified_fingerprint)
                );
            }
            Attestation::Ethereum {
                scheme,
                address,
                signature,
                recovered_address,
                ..
            } => {
                if scheme != "eip191-personal-sign" {
                    bail!("unsupported Ethereum scheme {scheme}");
                }
                let verification = verify_personal_sign(address, &message, signature)?;
                if !verification
                    .recovered_address
                    .eq_ignore_ascii_case(recovered_address)
                {
                    bail!("Ethereum recovered address mismatch for {}", subject.id());
                }
                println!(
                    "{}: ethereum ok {}",
                    subject.id(),
                    short_hash(&verification.recovered_address)
                );
            }
        }
    }
    Ok(())
}
