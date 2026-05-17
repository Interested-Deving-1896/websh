use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use regex::Regex;

use crate::CliResult;

use super::dotenv::load_dotenv;
use super::trunk::{run_output, run_trunk};

pub(super) fn pinata(
    root: &Path,
    dist_dir: PathBuf,
    name: Option<String>,
    no_build: bool,
    no_sign: bool,
    gateway: String,
    ens_url: String,
) -> CliResult {
    let mut envs = load_dotenv(root)?;

    // Manifest, ledger, attestation refresh, and PGP signing are now
    // owned by the trunk pre-build hook (`websh-cli attest build`). The
    // hook reads `WEBSH_NO_SIGN`, so propagate `--no-sign` through the
    // env vector that `run_trunk` passes to the trunk subprocess.
    if no_sign {
        envs.push(("WEBSH_NO_SIGN".to_string(), "1".to_string()));
    }

    if !no_build {
        println!("Cleaning previous Trunk build artifacts...");
        run_trunk(root, &["clean"], &envs)?;
        println!("Building release bundle...");
        run_trunk(root, &["build", "--release"], &envs)?;
    } else {
        println!(
            "Skipping build (--no-build); uploading existing dist as-is. \
             Run `cargo run --bin websh-cli -- attest build --force` first if \
             you need a refreshed attestation artifact without a rebuild."
        );
    }

    let dist_path = root.join(&dist_dir);
    if !dist_path.is_dir() {
        bail!(
            "upload directory does not exist: {}. Run without --no-build or check --dist-dir.",
            dist_path.display()
        );
    }

    let upload_name = name.unwrap_or_else(default_upload_name);
    println!(
        "Uploading {} to Pinata as {upload_name}...",
        dist_dir.display()
    );

    let dist_arg = dist_dir.to_string_lossy().into_owned();
    let output = run_output(
        root,
        "pinata",
        &["upload", &dist_arg, "--name", &upload_name],
        &envs,
    )?;
    if !output.stderr.trim().is_empty() {
        eprint!("{}", output.stderr);
    }
    if !output.stdout.trim().is_empty() {
        println!("{}", output.stdout.trim_end());
    }

    let cid = extract_cid(&format!("{}\n{}", output.stdout, output.stderr))
        .context("failed to extract CID from Pinata output")?;
    fs::write(root.join(".last-cid"), format!("{cid}\n")).context("write .last-cid")?;

    let gateway = gateway.trim_end_matches('/');

    println!();
    println!("CID: {cid}");
    println!("Gateway: {gateway}/ipfs/{cid}");
    println!();
    println!("Update ENS contenthash:");
    println!("  ipfs://{cid}");
    println!();
    println!("{ens_url}");

    Ok(())
}

fn default_upload_name() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("websh-{seconds}")
}

fn extract_cid(output: &str) -> Option<String> {
    let pattern = Regex::new(r"bafy[a-zA-Z0-9]+|Qm[a-zA-Z0-9]+").ok()?;
    pattern
        .find(output)
        .map(|matched| matched.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::extract_cid;

    #[test]
    fn extracts_cid_v1() {
        let output = r#"{"cid":"bafybeig7x4exampleaq7vm"}"#;
        assert_eq!(
            extract_cid(output).as_deref(),
            Some("bafybeig7x4exampleaq7vm")
        );
    }

    #[test]
    fn extracts_cid_v0() {
        let output = "IpfsHash: QmYwAPJzv5CZsnAzt8auVTLx7Uu";
        assert_eq!(
            extract_cid(output).as_deref(),
            Some("QmYwAPJzv5CZsnAzt8auVTLx7Uu")
        );
    }
}
