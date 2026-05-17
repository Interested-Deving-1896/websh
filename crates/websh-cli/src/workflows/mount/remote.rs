use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use anyhow::bail;

use crate::CliResult;
use crate::infra::gh::gh_status;

const EMPTY_MANIFEST_BODY: &str = "{\"entries\":[]}\n";

pub(super) fn push_empty_manifest(repo: &str, branch: &str, path_in_repo: &str) -> CliResult {
    let encoded = BASE64_STANDARD.encode(EMPTY_MANIFEST_BODY);
    let url = format!("repos/{repo}/contents/{path_in_repo}");
    let ok = gh_status([
        "api",
        &url,
        "-X",
        "PUT",
        "-f",
        "message=bootstrap: empty manifest",
        "-f",
        &format!("content={encoded}"),
        "-f",
        &format!("branch={branch}"),
    ])?;
    if !ok {
        bail!(
            "gh api failed pushing bootstrap manifest to {repo}@{branch}/{path_in_repo}; \
             check that `gh auth status` shows an authenticated account with \
             contents:write on this repository"
        );
    }
    Ok(())
}
