use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use serde::Deserialize;

use crate::CliResult;
use crate::workflows::content::DEFAULT_CONTENT_DIR;

const MEMPOOL_MOUNT_DECL_PATH: &str = ".websh/mounts/mempool.mount.json";

#[derive(Deserialize)]
struct MountDeclarationFile {
    backend: String,
    repo: Option<String>,
    branch: Option<String>,
    root: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct MempoolMountInfo {
    /// `owner/repo`, e.g., `0xwonj/websh-mempool`.
    pub(crate) repo: String,
    /// Branch to read from / write to.
    pub(crate) branch: String,
    /// Sub-path inside the repo whose subtree is exposed at mount root.
    /// Empty string means the repo root itself.
    pub(crate) root_prefix: String,
}

/// Read the mempool mount declaration from
/// `<root>/content/.websh/mounts/mempool.mount.json` and return the resolved
/// repo / branch / root prefix. Errors when the file is missing, malformed,
/// references a non-github backend, or omits the repo field.
pub(crate) fn read_mempool_mount_declaration(root: &Path) -> CliResult<MempoolMountInfo> {
    let path = mempool_mount_decl_path(root);
    if !path.exists() {
        bail!(
            "mempool mount declaration not found at {} — run `websh-cli mount init` first",
            path.display()
        );
    }
    let body =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let decl: MountDeclarationFile =
        serde_json::from_str(&body).with_context(|| format!("parse {}", path.display()))?;

    if decl.backend != "github" {
        bail!(
            "mempool mount at {} declares backend `{}`; expected `github`",
            path.display(),
            decl.backend
        );
    }
    let repo = decl
        .repo
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("{} is missing required `repo` field", path.display()))?;
    let branch = decl
        .branch
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());
    let root_prefix = decl.root.unwrap_or_default();

    Ok(MempoolMountInfo {
        repo,
        branch,
        root_prefix,
    })
}

fn mempool_mount_decl_path(root: &Path) -> PathBuf {
    root.join(DEFAULT_CONTENT_DIR).join(MEMPOOL_MOUNT_DECL_PATH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_mount(root: &Path, body: &str) {
        let p = mempool_mount_decl_path(root);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn reads_minimal_declaration() {
        let root = tempdir();
        write_mount(
            &root,
            r#"{"backend":"github","mount_at":"/mempool","repo":"0xwonj/m","branch":"main","root":"","writable":true,"name":"mempool"}"#,
        );
        let info = read_mempool_mount_declaration(&root).unwrap();
        assert_eq!(info.repo, "0xwonj/m");
        assert_eq!(info.branch, "main");
        assert_eq!(info.root_prefix, "");
    }

    #[test]
    fn defaults_branch_to_main_when_missing() {
        let root = tempdir();
        write_mount(
            &root,
            r#"{"backend":"github","mount_at":"/mempool","repo":"0xwonj/m"}"#,
        );
        let info = read_mempool_mount_declaration(&root).unwrap();
        assert_eq!(info.branch, "main");
    }

    #[test]
    fn rejects_non_github_backend() {
        let root = tempdir();
        write_mount(&root, r#"{"backend":"ipfs","mount_at":"/x","repo":"x/y"}"#);
        let err = read_mempool_mount_declaration(&root).unwrap_err();
        assert!(err.to_string().contains("backend `ipfs`"));
    }

    #[test]
    fn rejects_missing_repo() {
        let root = tempdir();
        write_mount(&root, r#"{"backend":"github","mount_at":"/mempool"}"#);
        let err = read_mempool_mount_declaration(&root).unwrap_err();
        assert!(err.to_string().contains("missing required `repo`"));
    }

    #[test]
    fn rejects_empty_repo_string() {
        let root = tempdir();
        write_mount(
            &root,
            r#"{"backend":"github","mount_at":"/mempool","repo":""}"#,
        );
        let err = read_mempool_mount_declaration(&root).unwrap_err();
        assert!(err.to_string().contains("missing required `repo`"));
    }

    #[test]
    fn errors_when_file_missing() {
        let root = tempdir();
        let err = read_mempool_mount_declaration(&root).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut d = std::env::temp_dir();
        d.push(format!("websh-mempool-test-{}-{}", std::process::id(), id));
        if d.exists() {
            fs::remove_dir_all(&d).unwrap();
        }
        fs::create_dir_all(&d).unwrap();
        d
    }
}
