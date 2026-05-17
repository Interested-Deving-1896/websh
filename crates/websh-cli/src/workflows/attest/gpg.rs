use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use websh_core::attestation::artifact::{Attestation, Subject, message_sha256};
use websh_core::crypto::pgp::normalize_fingerprint;

use crate::CliResult;
use crate::workflows::content::{artifact_path, resolve_path};

/// Probe the local gpg keyring for a secret key matching `gpg_key`
/// (defaults to whichever key gpg considers active when `None`). Returns
/// the normalized fingerprint of the first matching secret key, or
/// `None` when gpg is missing, the key isn't present, or the colon
/// output couldn't be parsed.
pub(super) fn gpg_secret_key_fingerprint(gpg_key: Option<&str>) -> Option<String> {
    let mut command = Command::new("gpg");
    command.args(["--with-colons", "--list-secret-keys"]);
    if let Some(key) = gpg_key {
        command.arg(key);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }

    // Colon-list format (per `doc/DETAILS` in the gnupg source): each line
    // is `record-type:field2:...:fieldN`. For `fpr` records the fingerprint
    // sits at column 10 (1-indexed) — that is, the iterator yields the
    // record type, then 8 empty separator fields, and the 9th `next()` call
    // lands on the fingerprint. `fpr:` records immediately follow their
    // owning `sec:` block.
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let mut fields = line.split(':');
        if fields.next() == Some("fpr") {
            for _ in 0..8 {
                fields.next()?;
            }
            if let Some(fp) = fields.next()
                && !fp.is_empty()
            {
                return Some(fp.to_string());
            }
        }
    }
    None
}

pub(super) fn sign_subject_with_gpg(
    root: &Path,
    subject: &Subject,
    key: &Path,
    gpg_key: Option<&str>,
    signature_dir: &Path,
) -> CliResult<Attestation> {
    let message = subject.canonical_message()?;
    let signature_dir = resolve_path(root, signature_dir);
    fs::create_dir_all(&signature_dir)
        .with_context(|| format!("create directory {}", signature_dir.display()))?;
    let slug = slugify_route(subject.route());
    let message_path = signature_dir.join(format!("{slug}.message.txt"));
    let signature_path = signature_dir.join(format!("{slug}.sig.asc"));
    fs::write(&message_path, &message)
        .with_context(|| format!("write {}", message_path.display()))?;

    let mut command = Command::new("gpg");
    command
        .arg("--yes")
        .arg("--armor")
        .arg("--detach-sign")
        .arg("--output")
        .arg(&signature_path);
    if let Some(gpg_key) = gpg_key {
        command.arg("--local-user").arg(gpg_key);
    }
    command.arg(&message_path);

    let output = command.output().with_context(|| {
        format!(
            "run gpg for {}. Use --no-sign to regenerate pending subjects only",
            subject.route()
        )
    })?;
    if !output.status.success() {
        bail!(
            "gpg failed for {}\n{}",
            subject.route(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let signature_body = fs::read_to_string(&signature_path)
        .with_context(|| format!("read {}", signature_path.display()))?;
    let fingerprint = verify_pgp_signature(root, key, &signature_body, &message)?;
    let signer = pgp_signer_from_key(root, key)
        .ok()
        .flatten()
        .or_else(|| gpg_key.map(ToOwned::to_owned));
    Ok(Attestation::Pgp {
        signer,
        fingerprint,
        key_path: artifact_path(root, key)?,
        signature: signature_body,
        signature_path: artifact_path(root, &signature_path).ok(),
        message_sha256: message_sha256(&message),
        verified: true,
    })
}

pub(super) fn verify_pgp_signature(
    root: &Path,
    key_path: &Path,
    signature: &str,
    message: &str,
) -> CliResult<String> {
    use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
    use pgp::types::KeyDetails;

    let (key, _headers) = SignedPublicKey::from_armor_file(resolve_path(root, key_path))?;
    key.verify_bindings()?;
    let (signature, _headers) = DetachedSignature::from_armor_single(signature.as_bytes())?;

    if signature.verify(&key, message.as_bytes()).is_ok()
        || key
            .public_subkeys
            .iter()
            .any(|subkey| signature.verify(subkey, message.as_bytes()).is_ok())
    {
        return Ok(normalize_fingerprint(&key.fingerprint().to_string()));
    }

    bail!("PGP detached signature did not verify with the supplied key")
}

pub(super) fn pgp_signer_from_key(root: &Path, key_path: &Path) -> CliResult<Option<String>> {
    use pgp::composed::{Deserializable, SignedPublicKey};

    let (key, _headers) = SignedPublicKey::from_armor_file(resolve_path(root, key_path))?;
    Ok(key
        .details
        .users
        .iter()
        .map(|user| String::from_utf8_lossy(user.id.id()).trim().to_string())
        .find(|user_id| !user_id.is_empty()))
}

pub(super) fn pgp_fingerprint_from_key(root: &Path, key_path: &Path) -> CliResult<String> {
    use pgp::composed::{Deserializable, SignedPublicKey};
    use pgp::types::KeyDetails;

    let (key, _headers) = SignedPublicKey::from_armor_file(resolve_path(root, key_path))?;
    Ok(normalize_fingerprint(&key.fingerprint().to_string()))
}

fn slugify_route(route: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in route.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let slug = out.trim_matches('-');
    if slug.is_empty() {
        "root".to_string()
    } else {
        slug.to_string()
    }
}
