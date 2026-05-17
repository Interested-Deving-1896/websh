use std::path::{Path, PathBuf};

use anyhow::bail;
use clap::{Args, Subcommand};

use websh_core::crypto::pgp::{
    EthereumIdentity, IdentityArtifact, PgpIdentity, normalize_fingerprint,
};
use websh_site::{APP_NAME, IDENTITY_PATH, PUBLIC_KEY_PATH};

use crate::CliResult;
use crate::infra::json::{read_json, write_json};

#[derive(Args)]
pub(crate) struct PgpCommand {
    #[command(subcommand)]
    command: PgpSubcommand,
}

#[derive(Subcommand)]
enum PgpSubcommand {
    Import {
        #[arg(long, default_value = PUBLIC_KEY_PATH)]
        key: PathBuf,
        #[arg(long, default_value = APP_NAME)]
        ens: String,
        #[arg(long, default_value = "")]
        address: String,
    },
    Verify,
}

pub(crate) fn run(root: &Path, command: PgpCommand) -> CliResult {
    match command.command {
        PgpSubcommand::Import { key, ens, address } => import(root, key, ens, address),
        PgpSubcommand::Verify => {
            verify_identity(root)?;
            println!("pgp: ok");
            Ok(())
        }
    }
}

pub(crate) fn verify_identity(root: &Path) -> CliResult {
    let identity = read_json::<IdentityArtifact>(&root.join(IDENTITY_PATH))?;
    let parsed = parse_key(&root.join(&identity.pgp.key_path))?;
    let expected = normalize_fingerprint(&identity.pgp.fingerprint);
    if parsed.fingerprint != expected {
        bail!(
            "PGP fingerprint mismatch: expected {}, got {}",
            expected,
            parsed.fingerprint
        );
    }
    Ok(())
}

fn import(root: &Path, key: PathBuf, ens: String, address: String) -> CliResult {
    let key_path = root.join(&key);
    let parsed = parse_key(&key_path)?;
    let identity = IdentityArtifact {
        version: 1,
        pgp: PgpIdentity {
            key_path: key.to_string_lossy().to_string(),
            fingerprint: parsed.fingerprint,
            user_ids: parsed.user_ids,
        },
        ethereum: EthereumIdentity { ens, address },
    };
    let path = root.join(IDENTITY_PATH);
    write_json(&path, &identity)?;
    println!("wrote {}", path.display());
    Ok(())
}

struct ParsedPgpKey {
    fingerprint: String,
    user_ids: Vec<String>,
}

fn parse_key(path: &Path) -> CliResult<ParsedPgpKey> {
    use pgp::composed::{Deserializable, SignedPublicKey};
    use pgp::types::KeyDetails;

    let (key, _headers) = SignedPublicKey::from_armor_file(path)?;
    let fingerprint = normalize_fingerprint(&key.fingerprint().to_string());
    let user_ids = key
        .details
        .users
        .iter()
        .map(|user| String::from_utf8_lossy(user.id.id()).to_string())
        .collect::<Vec<_>>();
    Ok(ParsedPgpKey {
        fingerprint,
        user_ids,
    })
}
