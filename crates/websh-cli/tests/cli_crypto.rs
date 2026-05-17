use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use websh_core::attestation::artifact::{
    Attestation, AttestationArtifact, Subject, compute_content_sha256,
};
use websh_core::attestation::ledger::{CONTENT_LEDGER_PATH, ContentLedger};
use websh_core::crypto::ack::{ACK_RECEIPTS_DIR, AckArtifact, slugify_name};
use websh_core::crypto::pgp::normalize_fingerprint;
use websh_site::{ACK_ARTIFACT_PATH, ACK_COMMITMENT_JSON, ATTESTATIONS_PATH};

fn temp_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("websh-{name}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn cli(root: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_websh-cli"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run websh-cli");
    assert!(
        output.status.success(),
        "websh-cli {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cli_with_env(root: &Path, args: &[&str], envs: &[(&str, &str)]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_websh-cli"));
    command.arg("--root").arg(root).args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().expect("run websh-cli");
    assert!(
        output.status.success(),
        "websh-cli {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn cli_output(root: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_websh-cli"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run websh-cli");
    assert!(
        output.status.success(),
        "websh-cli {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is utf8")
}

fn cli_fails(root: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_websh-cli"))
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .expect("run websh-cli");
    assert!(
        !output.status.success(),
        "websh-cli {:?} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_ack_artifact(root: &Path) {
    let path = root.join(ACK_ARTIFACT_PATH);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, ACK_COMMITMENT_JSON).unwrap();
}

fn write_homepage_content(root: &Path) {
    write_ack_artifact(root);
    fs::create_dir_all(root.join("crates/websh-web/src/features/home")).unwrap();
    fs::create_dir_all(root.join("assets/themes")).unwrap();
    fs::write(
        root.join("crates/websh-web/src/features/home/mod.rs"),
        "home",
    )
    .unwrap();
    fs::write(
        root.join("crates/websh-web/src/features/home/home.module.css"),
        "home-css",
    )
    .unwrap();
    fs::write(
        root.join("crates/websh-web/src/features/home/sections.rs"),
        "sections",
    )
    .unwrap();
    fs::write(root.join("assets/themes/dracula.css"), "theme").unwrap();
}

#[test]
fn cli_builds_ack_artifact_and_private_receipt() {
    let root = temp_root("ack");
    cli(&root, &["crypto", "ack", "init"]);
    cli(&root, &["crypto", "ack", "add", "--public", "coffee"]);
    cli(&root, &["crypto", "ack", "verify", "--name", "coffee"]);

    cli(
        &root,
        &["crypto", "ack", "add", "--private", "anonymous reviewer"],
    );

    let artifact_body = fs::read_to_string(root.join(ACK_ARTIFACT_PATH)).unwrap();
    assert!(!artifact_body.contains("anonymous reviewer"));
    let artifact: AckArtifact = serde_json::from_str(&artifact_body).unwrap();
    artifact.validate().unwrap();

    let receipt = root
        .join(ACK_RECEIPTS_DIR)
        .join(format!("{}.json", slugify_name("anonymous reviewer")));
    assert!(receipt.exists());
    cli(
        &root,
        &[
            "crypto",
            "ack",
            "verify",
            "--receipt",
            receipt.to_str().unwrap(),
        ],
    );

    cli(&root, &["crypto", "ack", "build"]);
    cli(
        &root,
        &["crypto", "ack", "receipt", "--name", "anonymous reviewer"],
    );

    cli(&root, &["crypto", "ack", "remove", "anonymous reviewer"]);
    assert!(!receipt.exists());

    cli(&root, &["crypto", "ack", "rm", "coffee"]);
    cli_fails(&root, &["crypto", "ack", "verify", "--name", "coffee"]);

    let artifact_body = fs::read_to_string(root.join(ACK_ARTIFACT_PATH)).unwrap();
    let artifact: AckArtifact = serde_json::from_str(&artifact_body).unwrap();
    artifact.validate().unwrap();
}

#[test]
fn cli_ack_handles_unicode_private_receipt_names() {
    let root = temp_root("ack-unicode");
    cli(&root, &["crypto", "ack", "init"]);
    cli(&root, &["crypto", "ack", "add", "--private", "익명 리뷰어"]);

    let receipt = root
        .join(ACK_RECEIPTS_DIR)
        .join(format!("{}.json", slugify_name("익명 리뷰어")));
    assert!(receipt.exists());
    assert!(receipt.file_name().unwrap().to_string_lossy().is_ascii());

    cli(
        &root,
        &[
            "crypto",
            "ack",
            "verify",
            "--receipt",
            receipt.to_str().unwrap(),
        ],
    );
    cli(&root, &["crypto", "ack", "remove", "익명 리뷰어"]);
    assert!(!receipt.exists());
}

#[test]
fn cli_attest_subject_set_builds_deterministic_content_hash() {
    let root = temp_root("attest-set");
    write_ack_artifact(&root);
    fs::write(root.join("a.txt"), "alpha").unwrap();
    fs::write(root.join("b.txt"), "beta").unwrap();

    cli(
        &root,
        &[
            "attest",
            "subject",
            "set",
            "--route",
            "/",
            "--kind",
            "homepage",
            "--issued-at",
            "2026-04-26",
            "--content",
            "b.txt",
            "--content",
            "a.txt",
        ],
    );

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let subject = artifact.subject_for_route("/").unwrap();
    assert_eq!(
        subject
            .content_files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "b.txt"]
    );
    assert_eq!(
        compute_content_sha256(subject.content_files()).unwrap(),
        subject.content_sha256().unwrap()
    );

    let message = cli_output(&root, &["attest", "subject", "message", "--route", "/"]);
    assert_eq!(message.trim_end(), subject.canonical_message().unwrap());

    let first_hash = subject.content_sha256().unwrap();
    cli(
        &root,
        &[
            "attest",
            "subject",
            "set",
            "--route",
            "/",
            "--kind",
            "homepage",
            "--issued-at",
            "2026-04-26",
            "--content",
            "a.txt",
            "--content",
            "b.txt",
        ],
    );
    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    assert_eq!(
        artifact
            .subject_for_route("/")
            .unwrap()
            .content_sha256()
            .unwrap(),
        first_hash
    );
}

#[test]
fn cli_attest_eth_import_rejects_invalid_signature() {
    let root = temp_root("attest-eth");
    write_ack_artifact(&root);
    fs::write(root.join("page.txt"), "page").unwrap();
    cli(
        &root,
        &[
            "attest",
            "subject",
            "set",
            "--route",
            "/",
            "--kind",
            "homepage",
            "--issued-at",
            "2026-04-26",
            "--content",
            "page.txt",
        ],
    );

    cli_fails(
        &root,
        &[
            "attest",
            "subject",
            "eth-import",
            "--route",
            "/",
            "--address",
            "0x742d35Cc6634C0532925a3b844Bc454e44f3A8B4",
            "--signature",
            "0x1234",
        ],
    );
}

#[test]
fn cli_attest_eth_import_accepts_valid_personal_sign_signature() {
    let root = temp_root("attest-eth-valid");
    write_ack_artifact(&root);
    fs::write(root.join("page.txt"), "page").unwrap();
    cli(
        &root,
        &[
            "attest",
            "subject",
            "set",
            "--route",
            "/",
            "--kind",
            "homepage",
            "--issued-at",
            "2026-04-26",
            "--content",
            "page.txt",
        ],
    );

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let message = artifact
        .subject_for_route("/")
        .unwrap()
        .canonical_message()
        .unwrap();
    let (address, signature) = eth_personal_sign_fixture(&message);

    cli(
        &root,
        &[
            "attest",
            "subject",
            "eth-import",
            "--route",
            "/",
            "--address",
            &address,
            "--signature",
            &signature,
            "--signer",
            "test.eth",
        ],
    );
    cli(&root, &["attest", "verify", "--route", "/"]);

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let ethereum = artifact
        .subject_for_route("/")
        .unwrap()
        .attestations()
        .iter()
        .find_map(|attestation| match attestation {
            Attestation::Ethereum {
                signer,
                address,
                recovered_address,
                verified,
                ..
            } => Some((signer, address, recovered_address, verified)),
            _ => None,
        })
        .expect("Ethereum attestation is stored");
    assert_eq!(ethereum.0, "test.eth");
    assert_eq!(ethereum.1, &address);
    assert_eq!(ethereum.2, &address);
    assert!(*ethereum.3);
}

#[test]
fn cli_attest_pgp_import_verifies_detached_signature() {
    let root = temp_root("attest-pgp");
    write_ack_artifact(&root);
    fs::write(root.join("page.txt"), "page").unwrap();
    cli(
        &root,
        &[
            "attest",
            "subject",
            "set",
            "--route",
            "/",
            "--kind",
            "homepage",
            "--issued-at",
            "2026-04-26",
            "--content",
            "page.txt",
        ],
    );
    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let message = artifact
        .subject_for_route("/")
        .unwrap()
        .canonical_message()
        .unwrap();
    let (key_path, signature_path, fingerprint) = write_pgp_fixture(&root, &message);

    cli(
        &root,
        &[
            "attest",
            "subject",
            "pgp-import",
            "--route",
            "/",
            "--signature",
            signature_path.to_str().unwrap(),
            "--key",
            key_path.to_str().unwrap(),
        ],
    );
    cli(&root, &["attest", "verify", "--route", "/"]);
    cli(&root, &["attest", "verify"]);

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let subject = artifact.subject_for_route("/").unwrap();
    let pgp = subject
        .attestations()
        .iter()
        .find_map(|attestation| match attestation {
            Attestation::Pgp {
                signer,
                fingerprint,
                ..
            } => Some((signer, fingerprint)),
            _ => None,
        })
        .expect("PGP attestation is stored");
    assert_eq!(pgp.0.as_deref(), Some("Test User <test@example.com>"));
    assert_eq!(pgp.1, &fingerprint);
}

#[test]
fn cli_attest_default_discovers_content_dir_and_manifest() {
    let root = temp_root("attest-default");
    write_homepage_content(&root);
    fs::create_dir_all(root.join("content/writing")).unwrap();
    fs::write(
        root.join("content/writing/hello.md"),
        "---\ntitle: Hello Attested World\ntags: [crypto, writing]\n---\n# Ignored Heading\nbody",
    )
    .unwrap();

    cli(&root, &["attest", "--no-sign", "--issued-at", "2026-04-26"]);
    cli(&root, &["attest", "verify"]);

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("content/manifest.json")).unwrap())
            .unwrap();
    let entries = manifest["entries"].as_array().unwrap();
    let ledger = entries
        .iter()
        .find(|entry| entry["path"] == ".websh/ledger.json")
        .expect("ledger artifact is exposed through manifest");
    assert_eq!(
        ledger["metadata"]["derived"]["title"]
            .as_str()
            .or_else(|| ledger["metadata"]["authored"]["title"].as_str()),
        Some("ledger")
    );
    let hello = entries
        .iter()
        .find(|entry| entry["path"] == "writing/hello.md")
        .expect("content file remains in manifest");
    assert_eq!(
        hello["metadata"]["authored"]["title"].as_str(),
        Some("Hello Attested World")
    );

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let ledger_subject = artifact.subject_for_route("/ledger").unwrap();
    assert!(matches!(ledger_subject, Subject::Ledger(_)));
    let ledger: ContentLedger =
        serde_json::from_str(&fs::read_to_string(root.join(CONTENT_LEDGER_PATH)).unwrap()).unwrap();
    ledger.validate().unwrap();
    if let Subject::Ledger(ledger_subject) = ledger_subject {
        assert_eq!(ledger_subject.chain_head, ledger.chain_head);
    }
    assert_eq!(
        ledger_subject
            .content_files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["content/.websh/ledger.json"]
    );
    let subject = artifact.subject_for_route("/writing/hello").unwrap();
    assert!(matches!(subject, Subject::Page(_)));
    let content_files: Vec<&str> = subject
        .content_files()
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert!(content_files.contains(&"content/writing/hello.md"));
    // sync also auto-generates the sidecar; it's part of the attested set.
    assert!(content_files.contains(&"content/writing/hello.meta.json"));
}

#[test]
fn cli_attest_default_updates_existing_subject_issued_at_when_content_changes() {
    let root = temp_root("attest-default-refresh-date");
    write_homepage_content(&root);
    fs::create_dir_all(root.join("content/writing")).unwrap();
    fs::write(
        root.join("content/writing/hello.md"),
        "---\ntitle: Hello\n---\nfirst body",
    )
    .unwrap();

    cli(&root, &["attest", "--no-sign", "--issued-at", "2000-01-01"]);

    fs::write(
        root.join("content/writing/hello.md"),
        "---\ntitle: Hello\n---\nrevised body",
    )
    .unwrap();
    cli(&root, &["attest", "--no-sign"]);

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    assert_eq!(
        artifact.subject_for_route("/").unwrap().issued_at(),
        "2000-01-01"
    );
    assert_ne!(
        artifact
            .subject_for_route("/writing/hello")
            .unwrap()
            .issued_at(),
        "2000-01-01"
    );
}

#[test]
fn cli_content_manifest_generates_manifest_without_attestation() {
    let root = temp_root("content-manifest");
    fs::create_dir_all(root.join("content/writing")).unwrap();
    fs::create_dir_all(root.join("content/talks")).unwrap();
    fs::write(
        root.join("content/writing/hello.md"),
        "---\ntitle: Hello Manifest\ndate: 2026-04-20\ntags: [notes, websh]\n---\n# Ignored\nbody",
    )
    .unwrap();
    fs::write(root.join("content/talks/slides.pdf"), b"%PDF").unwrap();
    fs::write(
        root.join("content/talks/slides.meta.json"),
        r#"{"schema":1,"kind":"document","authored":{"title":"ZK Talk","date":"2026-04-24","tags":["talk","zk"]},"derived":{}}"#,
    )
    .unwrap();

    let output = cli_output(&root, &["content", "manifest"]);
    assert!(output.contains("sidecars refreshed"));
    assert!(!root.join(ATTESTATIONS_PATH).exists());

    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join("content/manifest.json")).unwrap())
            .unwrap();
    let entries = manifest["entries"].as_array().unwrap();
    let file_entries: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["metadata"]["kind"] != "directory")
        .collect();
    assert_eq!(file_entries.len(), 2);

    let hello = file_entries
        .iter()
        .find(|e| e["path"] == "writing/hello.md")
        .unwrap();
    assert_eq!(hello["metadata"]["authored"]["title"], "Hello Manifest");
    assert_eq!(hello["metadata"]["authored"]["date"], "2026-04-20");

    let slides = file_entries
        .iter()
        .find(|e| e["path"] == "talks/slides.pdf")
        .unwrap();
    assert_eq!(slides["metadata"]["authored"]["title"], "ZK Talk");
    assert_eq!(slides["metadata"]["authored"]["date"], "2026-04-24");
    assert_eq!(slides["metadata"]["authored"]["tags"][0], "talk");
}

#[test]
fn cli_attest_default_can_sign_with_local_gpg() {
    let root = temp_root("attest-default-pgp");
    write_homepage_content(&root);

    cli(&root, &["attest", "--no-sign", "--issued-at", "2026-04-26"]);
    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let message = artifact
        .subject_for_route("/")
        .unwrap()
        .canonical_message()
        .unwrap();
    let ledger_message = artifact
        .subject_for_route("/ledger")
        .unwrap()
        .canonical_message()
        .unwrap();
    let (key_path, signature_dir, fingerprint) =
        write_pgp_fixture_set(&root, &[("root", &message), ("ledger", &ledger_message)]);

    let fake_bin = root.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let fake_gpg = fake_bin.join("gpg");
    fs::write(
        &fake_gpg,
        "#!/bin/sh\nif [ \"$1\" = \"--with-colons\" ] && [ \"$2\" = \"--list-secret-keys\" ]; then\n  printf 'sec:::::::::\\n'\n  printf 'fpr:::::::::%s:\\n' \"$WEBSH_FAKE_GPG_FINGERPRINT\"\n  exit 0\nfi\nout=\"\"\nin=\"\"\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--output\" ]; then\n    shift\n    out=\"$1\"\n  else\n    in=\"$1\"\n  fi\n  shift\ndone\nslug=$(basename \"$in\" .message.txt)\ncp \"$WEBSH_FAKE_GPG_SIGNATURE_DIR/$slug.sig.asc\" \"$out\"\n",
    )
    .unwrap();
    let mut perms = fs::metadata(&fake_gpg).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake_gpg, perms).unwrap();
    let old_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{old_path}", fake_bin.display());

    cli_with_env(
        &root,
        &[
            "attest",
            "--issued-at",
            "2026-04-26",
            "--key",
            key_path.to_str().unwrap(),
        ],
        &[
            ("PATH", &path),
            (
                "WEBSH_FAKE_GPG_SIGNATURE_DIR",
                signature_dir.to_str().unwrap(),
            ),
            ("WEBSH_FAKE_GPG_FINGERPRINT", &fingerprint),
        ],
    );
    cli(&root, &["attest", "verify"]);

    let artifact: AttestationArtifact =
        serde_json::from_str(&fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap()).unwrap();
    let pgp = artifact
        .subject_for_route("/")
        .unwrap()
        .attestations()
        .iter()
        .find_map(|attestation| match attestation {
            Attestation::Pgp {
                signer,
                fingerprint,
                ..
            } => Some((signer, fingerprint)),
            _ => None,
        })
        .expect("PGP attestation is stored");
    assert_eq!(pgp.0.as_deref(), Some("Test User <test@example.com>"));
    assert_eq!(pgp.1, &fingerprint);
    let ledger_pgp = artifact
        .subject_for_route("/ledger")
        .unwrap()
        .attestations()
        .iter()
        .find_map(|attestation| match attestation {
            Attestation::Pgp { fingerprint, .. } => Some(fingerprint),
            _ => None,
        })
        .expect("ledger PGP attestation is stored");
    assert_eq!(ledger_pgp, &fingerprint);
}

fn write_pgp_fixture(root: &Path, message: &str) -> (PathBuf, PathBuf, String) {
    let (key_path, signature_dir, fingerprint) =
        write_pgp_fixture_set(root, &[("subject", message)]);
    (key_path, signature_dir.join("subject.sig.asc"), fingerprint)
}

fn write_pgp_fixture_set(root: &Path, messages: &[(&str, &str)]) -> (PathBuf, PathBuf, String) {
    use pgp::composed::{ArmorOptions, DetachedSignature, KeyType, SecretKeyParamsBuilder};
    use pgp::crypto::hash::HashAlgorithm;
    use pgp::types::{KeyDetails, Password};

    let mut rng = rand08::thread_rng();
    let key_params = SecretKeyParamsBuilder::default()
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .primary_user_id("Test User <test@example.com>".into())
        .passphrase(None)
        .build()
        .unwrap();
    let secret = key_params.generate(&mut rng).unwrap();
    let public = secret.to_public_key();
    let fingerprint = normalize_fingerprint(&public.fingerprint().to_string());

    let key_path = root.join(".test-keys/test.asc");
    fs::create_dir_all(key_path.parent().unwrap()).unwrap();
    fs::write(
        &key_path,
        public.to_armored_string(ArmorOptions::default()).unwrap(),
    )
    .unwrap();

    let signature_dir = root.join(".test-signatures");
    fs::create_dir_all(&signature_dir).unwrap();
    for (slug, message) in messages {
        let signature = DetachedSignature::sign_binary_data(
            &mut rng,
            &secret.primary_key,
            &Password::empty(),
            HashAlgorithm::Sha256,
            message.as_bytes(),
        )
        .unwrap();
        fs::write(
            signature_dir.join(format!("{slug}.sig.asc")),
            signature
                .to_armored_string(ArmorOptions::default())
                .unwrap(),
        )
        .unwrap();
    }

    (key_path, signature_dir, fingerprint)
}

fn eth_personal_sign_fixture(message: &str) -> (String, String) {
    use alloy_primitives::{Address, eip191_hash_message};
    use k256::ecdsa::SigningKey;

    let private_key =
        hex::decode("4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318").unwrap();
    let signing_key = SigningKey::from_slice(&private_key).unwrap();
    let address = Address::from_private_key(&signing_key).to_checksum(None);
    let prehash = eip191_hash_message(message);
    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(prehash.as_slice())
        .unwrap();
    let mut bytes = [0u8; 65];
    bytes[..64].copy_from_slice(signature.to_bytes().as_slice());
    bytes[64] = 27 + recovery_id.to_byte();

    (address, format!("0x{}", hex::encode(bytes)))
}
