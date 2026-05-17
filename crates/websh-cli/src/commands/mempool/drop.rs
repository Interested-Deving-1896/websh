use std::path::Path;

use anyhow::{Context, bail};
use clap::Args;

use crate::CliResult;
use crate::infra::gh::require_gh;

use crate::workflows::mempool::mount::read_mempool_mount_declaration;
use crate::workflows::mempool::path::MempoolEntryPath;
use crate::workflows::mempool::remote::{DropOutcome, drop_via_gh};

#[derive(Args)]
pub(super) struct DropArgs {
    /// Repo-relative path inside the mempool repo.
    #[arg(long)]
    path: String,
    /// Succeed silently if the entry no longer exists.
    #[arg(long, default_value_t = false)]
    if_exists: bool,
}

pub(super) fn drop_entry(root: &Path, args: DropArgs) -> CliResult {
    let mount = read_mempool_mount_declaration(root)?;
    require_gh()?;

    let entry_path = MempoolEntryPath::parse(&args.path)
        .with_context(|| format!("invalid --path `{}`", args.path))?;
    let outcome = drop_via_gh(&mount, entry_path.as_str())?;
    match outcome {
        DropOutcome::Removed { manifest, blob } => {
            println!(
                "mempool drop: removed {} from {} (manifest={}, blob={})",
                entry_path, mount.repo, manifest, blob,
            );
            Ok(())
        }
        DropOutcome::Absent => {
            if args.if_exists {
                println!("mempool drop: {} not present, nothing to do", entry_path);
                Ok(())
            } else {
                bail!("entry not found at {}", entry_path)
            }
        }
    }
}
