use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use websh_core::attestation::artifact::AttestationArtifact;
use websh_core::attestation::ledger::{CONTENT_LEDGER_PATH, CONTENT_LEDGER_ROUTE};
use websh_site::{ATTESTATIONS_PATH, PUBLIC_KEY_PATH};

use crate::CliResult;
use crate::infra::json::write_json;
use crate::workflows::content::{
    DEFAULT_CONTENT_DIR, build_manifest_from_sidecars, resolve_path, sync_content,
};

use super::discover::discover_subject_specs;
use super::sign::sign_missing_pgp_attestations;
use super::subject::{SubjectKind, build_subject, content_paths_or_default, read_artifact};
use super::verify::verify;
use super::{DEFAULT_GPG_SIGNER, DEFAULT_SIGNATURE_DIR};

pub(crate) struct AttestAllOptions {
    pub(crate) content_dir: PathBuf,
    pub(crate) key: PathBuf,
    pub(crate) gpg_key: Option<String>,
    pub(crate) signature_dir: PathBuf,
    pub(crate) no_sign: bool,
    pub(crate) issued_at: Option<String>,
}

pub(crate) fn run_default(root: &Path, no_sign: bool) -> CliResult {
    let no_sign = no_sign || no_sign_from_env();
    attest_all(
        root,
        AttestAllOptions {
            content_dir: PathBuf::from(DEFAULT_CONTENT_DIR),
            key: PathBuf::from(PUBLIC_KEY_PATH),
            gpg_key: Some(DEFAULT_GPG_SIGNER.to_string()),
            signature_dir: PathBuf::from(DEFAULT_SIGNATURE_DIR),
            no_sign,
            issued_at: None,
        },
    )
}

/// Trunk pre-build entrypoint. No-ops on dev profiles so `trunk serve`
/// and incremental dev builds stay fast.
pub(crate) fn attest_build(root: &Path, force: bool) -> CliResult {
    if !force && !profile_is_release() {
        let profile = std::env::var("TRUNK_PROFILE").unwrap_or_default();
        println!("attest: skipped (profile={profile})");
        return Ok(());
    }
    run_default(root, no_sign_from_env())
}

fn profile_is_release() -> bool {
    std::env::var("TRUNK_PROFILE")
        .map(|p| p == "release")
        .unwrap_or(false)
}

fn no_sign_from_env() -> bool {
    std::env::var("WEBSH_NO_SIGN")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

pub(crate) fn attest_all(root: &Path, options: AttestAllOptions) -> CliResult {
    let content_root = resolve_path(root, &options.content_dir);
    fs::create_dir_all(&content_root)
        .with_context(|| format!("create directory {}", content_root.display()))?;
    sync_content(root, &options.content_dir)?;
    let ledger = crate::workflows::content::generate_content_ledger(root, &options.content_dir)?;
    let manifest = build_manifest_from_sidecars(root, &options.content_dir)?;
    let specs = discover_subject_specs(root, &options.content_dir)?;

    let existing = read_artifact(root).unwrap_or_default();
    existing.validate_header()?;

    let mut artifact = AttestationArtifact {
        version: existing.version,
        scheme: existing.scheme.clone(),
        subjects: Vec::new(),
    };

    let homepage_paths = content_paths_or_default(root, "/", SubjectKind::Homepage, Vec::new())?;
    artifact.subjects.push(build_subject(
        root,
        &existing,
        "/".to_string(),
        SubjectKind::Homepage,
        homepage_paths,
        options.issued_at.clone(),
        None,
    )?);

    let mut routes = BTreeSet::from(["/".to_string()]);
    if !routes.insert(CONTENT_LEDGER_ROUTE.to_string()) {
        bail!("duplicate attestation route {CONTENT_LEDGER_ROUTE}");
    }
    artifact.subjects.push(build_subject(
        root,
        &existing,
        CONTENT_LEDGER_ROUTE.to_string(),
        SubjectKind::Ledger,
        vec![PathBuf::from(CONTENT_LEDGER_PATH)],
        options.issued_at.clone(),
        Some(&ledger),
    )?);

    for spec in specs {
        if !routes.insert(spec.route.clone()) {
            bail!("duplicate attestation route {}", spec.route);
        }
        artifact.subjects.push(build_subject(
            root,
            &existing,
            spec.route,
            spec.kind,
            spec.content_paths,
            options.issued_at.clone(),
            None,
        )?);
    }
    artifact.subjects.sort_by_key(|subject| subject.id());

    write_json(&root.join(ATTESTATIONS_PATH), &artifact)?;

    let mut signed = 0usize;
    if !options.no_sign {
        if root.join(&options.key).exists() {
            signed = sign_missing_pgp_attestations(
                root,
                &options.key,
                options.gpg_key.as_deref(),
                &options.signature_dir,
            )?;
        } else {
            println!(
                "pgp: pending; public key not found at {}",
                options.key.display()
            );
        }
    }

    verify(root, None)?;
    println!(
        "attest: {} subjects, {} manifest entries, {} ledger blocks, {} new pgp signatures",
        artifact.subjects.len(),
        manifest.entries.len(),
        ledger.block_count,
        signed
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{no_sign_from_env, profile_is_release};
    use std::sync::Mutex;

    // Env vars are process-global; serialize tests that touch them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard so a panic inside the test body still restores the
    /// previous value of the env var.
    struct EnvGuard {
        key: String,
        prev: Option<String>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(&self.key, v) },
                None => unsafe { std::env::remove_var(&self.key) },
            }
        }
    }

    fn with_env<F: FnOnce()>(key: &str, value: Option<&str>, f: F) {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var(key).ok();
        let _guard = EnvGuard {
            key: key.to_string(),
            prev,
        };
        match value {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        f();
        // _guard drops here (or on panic), restoring the previous value.
    }

    #[test]
    fn no_sign_from_env_recognizes_truthy_values() {
        for value in ["1", "true", "TRUE", "True", "yes", "  yes  "] {
            with_env("WEBSH_NO_SIGN", Some(value), || {
                assert!(no_sign_from_env(), "value `{value}` should be truthy");
            });
        }
    }

    #[test]
    fn no_sign_from_env_rejects_falsy_or_empty() {
        for value in ["", "0", "false", "no", "off"] {
            with_env("WEBSH_NO_SIGN", Some(value), || {
                assert!(!no_sign_from_env(), "value `{value}` should be falsy");
            });
        }
    }

    #[test]
    fn no_sign_from_env_false_when_unset() {
        with_env("WEBSH_NO_SIGN", None, || {
            assert!(!no_sign_from_env());
        });
    }

    #[test]
    fn profile_is_release_only_for_release() {
        for (value, expected) in [
            (Some("release"), true),
            (Some("dev"), false),
            (Some(""), false),
            (Some("Release"), false), // case-sensitive — TRUNK_PROFILE is `release` lowercase
        ] {
            with_env("TRUNK_PROFILE", value, || {
                assert_eq!(
                    profile_is_release(),
                    expected,
                    "profile=`{value:?}` expected={expected}"
                );
            });
        }
    }

    #[test]
    fn profile_is_release_false_when_unset() {
        with_env("TRUNK_PROFILE", None, || {
            assert!(!profile_is_release());
        });
    }
}
