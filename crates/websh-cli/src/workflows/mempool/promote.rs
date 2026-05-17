use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use websh_core::mempool::transform_mempool_frontmatter;
use websh_site::{ATTESTATIONS_PATH, BOOTSTRAP_SITE};

use crate::CliResult;
use crate::infra::gh::{GhResourceStatus, require_gh};
use crate::infra::git::{git_output, git_status, run_git_best_effort};
use crate::workflows::content::DEFAULT_CONTENT_DIR;

use super::mount::{MempoolMountInfo, read_mempool_mount_declaration};
use super::path::MempoolEntryPath;
use super::remote::{DropOutcome, drop_via_gh, fetch_mempool_body, gh_path_status};

#[derive(Clone, Debug)]
pub(crate) struct PromoteOptions {
    pub(crate) path: String,
    pub(crate) keep_remote: bool,
    pub(crate) no_attest: bool,
    pub(crate) allow_branch_mismatch: bool,
    pub(crate) interaction: InteractionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InteractionMode {
    Interactive,
    NonInteractive,
}

impl InteractionMode {
    pub(crate) fn detect() -> Self {
        if std::env::var_os("CI").is_some() || !io::stdin().is_terminal() {
            Self::NonInteractive
        } else {
            Self::Interactive
        }
    }
}

/// Resolved promote target: where the entry comes from in the mempool repo
/// and where it lands in the bundle source on disk.
#[derive(Clone, Debug)]
struct PromoteTarget {
    /// Path inside the mempool repo, e.g., `writing/foo.md`.
    repo_path: String,
    /// Category segment, e.g., `writing`. Production code reads category
    /// indirectly through `bundle_disk_path`; tests assert this field
    /// directly to verify path-parsing extracts the right segment.
    #[cfg_attr(not(test), allow(dead_code))]
    category: String,
    /// `<category>/<slug>` (no extension), used in commit messages.
    slug_relpath: String,
    /// Filesystem path (relative to repo root) where the body lands:
    /// `content/<category>/<slug>.md`.
    bundle_disk_path: PathBuf,
}

impl PromoteTarget {
    fn sidecar_disk_path(&self) -> PathBuf {
        let mut path = self.bundle_disk_path.clone();
        let stem = path
            .file_stem()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        path.set_file_name(format!("{stem}.meta.json"));
        path
    }
}

/// Tracks which mutations have happened so the rollback knows what to undo
/// on partial failure.
#[derive(Default)]
struct PromoteCleanup {
    body_written: bool,
    sidecar_written: bool,
    ledger_written: bool,
    manifest_written: bool,
    attest_written: bool,
}

pub(crate) fn promote_entry(root: &Path, args: PromoteOptions) -> CliResult {
    let target = parse_promote_path(&args.path)?;
    let mount = read_mempool_mount_declaration(root)?;
    require_gh()?;

    // Step 0 — pre-flight (no mutation).
    ensure_promote_worktree_ready(root, &target)?;
    confirm_on_bundle_branch(root, args.interaction, args.allow_branch_mismatch)?;
    gh_verify_path_exists(&mount, &target)?;
    ensure_local_target_absent(root, &target)?;
    let baseline_status = git_status_snapshot(root)?;

    eprintln!("preflight: ok ({})", target.repo_path);

    // Step 1 — fetch + write + regenerate.
    let body = fetch_mempool_body(&mount, &target.repo_path)?;
    eprintln!("fetch:    {} ({} bytes)", target.repo_path, body.len());
    let canonical_body = transform_mempool_frontmatter(&body)?;

    let mut cleanup = PromoteCleanup::default();
    if let Err(e) = run_promote_steps(root, &target, &canonical_body, &args, &mut cleanup) {
        rollback(root, &target, &cleanup);
        return Err(e);
    }
    if let Err(e) =
        validate_promote_write_set(root, &target, cleanup.attest_written, &baseline_status)
    {
        rollback(root, &target, &cleanup);
        return Err(e);
    }

    // Step 2 — git commit.
    if let Err(e) = stage_and_commit(root, &target, cleanup.attest_written) {
        rollback(root, &target, &cleanup);
        return Err(e);
    }

    // Step 3 — drop the mempool original (default).
    if !args.keep_remote {
        match drop_via_gh(&mount, &target.repo_path) {
            Ok(DropOutcome::Removed { manifest, blob }) => println!(
                "mempool drop: removed {} from {} (manifest={}, blob={})",
                target.repo_path, mount.repo, manifest, blob
            ),
            Ok(DropOutcome::Absent) => println!(
                "mempool drop: {} already absent from {}",
                target.repo_path, mount.repo
            ),
            Err(e) => eprintln!(
                "mempool drop: {e} — re-run `websh-cli mempool drop --path {}` to retry",
                args.path
            ),
        }
    }

    println!("\nready. review the commit, then `git push` and `just pin` to deploy.");
    Ok(())
}

/// Parse `--path` into a structured PromoteTarget. Validates `<category>/<slug>.md`
/// shape with category in `LEDGER_CATEGORIES`.
fn parse_promote_path(repo_relative: &str) -> CliResult<PromoteTarget> {
    let entry_path = MempoolEntryPath::parse(repo_relative)
        .with_context(|| format!("invalid promote path `{repo_relative}`"))?;
    let repo_path = entry_path.as_str();
    let mut parts = repo_path.split('/');
    let category = parts.next().unwrap_or_default().to_string();
    let rest = parts.next().unwrap_or_default();
    let slug = rest
        .strip_suffix(".md")
        .expect("MempoolEntryPath validates .md extension")
        .to_string();
    let slug_relpath = format!("{category}/{slug}");
    let bundle_disk_path = PathBuf::from(DEFAULT_CONTENT_DIR)
        .join(&category)
        .join(format!("{slug}.md"));

    Ok(PromoteTarget {
        repo_path: repo_path.to_string(),
        category,
        slug_relpath,
        bundle_disk_path,
    })
}

fn ensure_promote_worktree_ready(root: &Path, target: &PromoteTarget) -> CliResult {
    let owned_dirty = git_status_for_paths(root, &command_owned_paths(target))?;
    if !owned_dirty.is_empty() {
        bail!(
            "mempool promote would overwrite command-owned path(s). Commit/stash/remove them before retrying:\n{}",
            owned_dirty.trim()
        );
    }

    let content_dirty = git_status_for_paths(root, &[PathBuf::from(DEFAULT_CONTENT_DIR)])?;
    if !content_dirty.is_empty() {
        bail!(
            "uncommitted changes in content/. Stage/stash them before retrying:\n{}",
            content_dirty.trim()
        );
    }
    Ok(())
}

fn git_status_for_paths(root: &Path, paths: &[PathBuf]) -> CliResult<String> {
    let mut args = vec![
        OsString::from("status"),
        OsString::from("--porcelain"),
        OsString::from("--untracked-files=all"),
        OsString::from("--"),
    ];
    for path in paths {
        args.push(path.as_os_str().to_os_string());
    }
    let out = git_output(root, args)?;
    if !out.success {
        bail!("git status failed: {}", out.stderr.trim());
    }
    Ok(out.stdout)
}

fn git_status_snapshot(root: &Path) -> CliResult<String> {
    git_status_for_paths(root, &[])
}

fn confirm_on_bundle_branch(
    root: &Path,
    interaction: InteractionMode,
    allow_branch_mismatch: bool,
) -> CliResult {
    let out = git_output(root, ["rev-parse", "--abbrev-ref", "HEAD"])?;
    if !out.success {
        bail!("git rev-parse failed (is this a git checkout?)");
    }
    let current = out.stdout.trim().to_string();
    let expected = BOOTSTRAP_SITE.branch;
    if current == expected {
        return Ok(());
    }
    if allow_branch_mismatch {
        eprintln!(
            "warn: HEAD is `{current}`, deploy branch is `{expected}`; continuing because \
             --allow-branch-mismatch was provided"
        );
        return Ok(());
    }
    if interaction == InteractionMode::NonInteractive {
        bail!(
            "branch mismatch: HEAD is `{current}`, deploy branch is `{expected}`. Re-run with \
             --allow-branch-mismatch to continue in non-interactive mode"
        );
    }
    eprint!("warn: HEAD is `{current}`, deploy branch is `{expected}`. Continue? [y/N] ");
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("read confirmation from stdin")?;
    let trimmed = answer.trim();
    if trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        bail!("aborted: not on `{expected}`")
    }
}

fn gh_verify_path_exists(mount: &MempoolMountInfo, target: &PromoteTarget) -> CliResult {
    match gh_path_status(mount, &target.repo_path)? {
        GhResourceStatus::Exists => Ok(()),
        GhResourceStatus::Missing => bail!(
            "{} not found in {}@{}",
            target.repo_path,
            mount.repo,
            mount.branch
        ),
    }
}

fn ensure_local_target_absent(root: &Path, target: &PromoteTarget) -> CliResult {
    let p = root.join(&target.bundle_disk_path);
    if p.exists() {
        bail!(
            "{} already exists locally — pick a different slug or `git rm` the existing file",
            target.bundle_disk_path.display()
        );
    }
    Ok(())
}

fn run_promote_steps(
    root: &Path,
    target: &PromoteTarget,
    body: &str,
    args: &PromoteOptions,
    cleanup: &mut PromoteCleanup,
) -> CliResult {
    // Ensure the parent directory exists, then write the body.
    let abs_path = root.join(&target.bundle_disk_path);
    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    cleanup.body_written = true; // Set before write — partial-write on disk-full counts.
    std::fs::write(&abs_path, body).with_context(|| format!("write {}", abs_path.display()))?;
    eprintln!("write:    {}", target.bundle_disk_path.display());

    if args.no_attest {
        // Direct sync + ledger + manifest regeneration. Set flags BEFORE
        // the calls so a mid-write failure still lets rollback restore
        // the prior state. Sync runs first so the just-written markdown
        // file gets a sidecar (with `authored` populated from its
        // frontmatter) before the manifest is folded.
        cleanup.sidecar_written = true;
        cleanup.manifest_written = true;
        crate::workflows::content::sync_content(root, Path::new(DEFAULT_CONTENT_DIR))?;
        cleanup.ledger_written = true;
        let ledger = crate::workflows::content::generate_content_ledger(
            root,
            Path::new(DEFAULT_CONTENT_DIR),
        )?;
        let manifest = crate::workflows::content::build_manifest_from_sidecars(
            root,
            Path::new(DEFAULT_CONTENT_DIR),
        )?;
        eprintln!(
            "ledger:   {} blocks -> content/.websh/ledger.json",
            ledger.block_count
        );
        eprintln!(
            "manifest: {} entries -> content/manifest.json",
            manifest.entries.len()
        );
    } else {
        // The default attestation workflow writes ledger.json, manifest.json,
        // and attestations.json sequentially; flag each as potentially-written
        // before invocation so a mid-flow signing failure rolls back all three.
        cleanup.sidecar_written = true;
        cleanup.ledger_written = true;
        cleanup.manifest_written = true;
        cleanup.attest_written = true;
        crate::workflows::attest::run_default(root, /*no_sign*/ false)?;
    }
    Ok(())
}

fn stage_and_commit(root: &Path, target: &PromoteTarget, did_attest: bool) -> CliResult {
    let paths = stage_paths(target, did_attest);

    let mut add_args = vec![OsString::from("add"), OsString::from("--")];
    for p in &paths {
        add_args.push(p.as_os_str().to_os_string());
    }
    if !git_status(root, add_args)? {
        bail!("git add failed");
    }

    let msg = format!("promote: {}", target.slug_relpath);
    if !git_status(root, ["commit", "-m", &msg])? {
        bail!("git commit failed");
    }
    Ok(())
}

fn validate_promote_write_set(
    root: &Path,
    target: &PromoteTarget,
    did_attest: bool,
    baseline_status: &str,
) -> CliResult {
    let baseline = porcelain_status_map(baseline_status);
    let current_status = git_status_snapshot(root)?;
    let current = porcelain_status_map(&current_status);
    let allowed = stage_paths(target, did_attest);

    let unexpected = current
        .iter()
        .filter(|(path, state)| baseline.get(*path) != Some(*state))
        .filter(|(path, _)| !allowed.iter().any(|allowed_path| allowed_path == *path))
        .map(|(path, _)| path.display().to_string())
        .collect::<Vec<_>>();

    if unexpected.is_empty() {
        return Ok(());
    }

    bail!(
        "mempool promote generated unexpected path changes outside its write-set:\n{}",
        unexpected.join("\n")
    )
}

fn porcelain_status_map(status: &str) -> std::collections::BTreeMap<PathBuf, String> {
    status
        .lines()
        .filter_map(|line| {
            let state = line.get(0..2)?.to_string();
            let raw_path = line.get(3..)?.rsplit(" -> ").next().unwrap_or("");
            if raw_path.is_empty() {
                return None;
            }
            Some((PathBuf::from(raw_path), state))
        })
        .collect()
}

fn stage_paths(target: &PromoteTarget, did_attest: bool) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = vec![
        target.bundle_disk_path.clone(),
        target.sidecar_disk_path(),
        PathBuf::from("content/.websh/ledger.json"),
        PathBuf::from("content/manifest.json"),
    ];
    if did_attest {
        paths.push(PathBuf::from(ATTESTATIONS_PATH));
    }
    paths
}

fn command_owned_paths(target: &PromoteTarget) -> Vec<PathBuf> {
    let mut paths = stage_paths(target, true);
    paths.sort();
    paths.dedup();
    paths
}

fn rollback(root: &Path, target: &PromoteTarget, c: &PromoteCleanup) {
    for path in rollback_paths(target, c) {
        let reset_args = git_args_with_path(["reset", "HEAD", "--"], &path);
        let _ = run_git_best_effort(root, reset_args);

        let checkout_args = git_args_with_path(["checkout", "HEAD", "--"], &path);
        let _ = run_git_best_effort(root, checkout_args);

        if !git_path_exists_in_head(root, &path) {
            let _ = std::fs::remove_file(root.join(&path));
        }
    }
    // .websh/local/crypto/attestations is gitignored; not restored.
}

fn rollback_paths(target: &PromoteTarget, c: &PromoteCleanup) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if c.body_written {
        paths.push(target.bundle_disk_path.clone());
    }
    if c.sidecar_written {
        paths.push(target.sidecar_disk_path());
    }
    if c.ledger_written {
        paths.push(PathBuf::from("content/.websh/ledger.json"));
    }
    if c.manifest_written {
        paths.push(PathBuf::from("content/manifest.json"));
    }
    if c.attest_written {
        paths.push(PathBuf::from(ATTESTATIONS_PATH));
    }
    paths.sort();
    paths.dedup();
    paths
}

fn git_args_with_path<const N: usize>(prefix: [&str; N], path: &Path) -> Vec<OsString> {
    let mut args = prefix
        .iter()
        .map(|part| OsString::from(*part))
        .collect::<Vec<_>>();
    args.push(path.as_os_str().to_os_string());
    args
}

fn git_path_exists_in_head(root: &Path, path: &Path) -> bool {
    let Some(spec) = git_head_pathspec(path) else {
        return false;
    };
    run_git_best_effort(root, ["cat-file", "-e", &spec])
}

fn git_head_pathspec(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .map(|component| component.as_os_str().to_str().map(|part| part.to_string()))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("HEAD:{}", parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::super::path::MempoolEntryPathError;
    use super::*;
    use std::fs;
    use std::process::{Command as Process, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_repo(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("websh-promote-{name}-{}-{id}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        run_git(&root, ["init"]);
        root
    }

    fn run_git<const N: usize>(root: &Path, args: [&str; N]) {
        let status = Process::new("git")
            .current_dir(root)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run git");
        assert!(status.success(), "git command failed");
    }

    fn commit_all(root: &Path) {
        run_git(root, ["add", "--all"]);
        let status = Process::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initial",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run git commit");
        assert!(status.success(), "git commit failed");
    }

    fn write(root: &Path, path: &str, body: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn promote_path_error(raw: &str) -> MempoolEntryPathError {
        parse_promote_path(raw)
            .unwrap_err()
            .downcast_ref::<MempoolEntryPathError>()
            .expect("parse error keeps typed path source")
            .clone()
    }

    #[test]
    fn parse_promote_path_extracts_category_slug_and_disk_path() {
        let t = parse_promote_path("writing/foo.md").unwrap();
        assert_eq!(t.repo_path, "writing/foo.md");
        assert_eq!(t.category, "writing");
        assert_eq!(t.slug_relpath, "writing/foo");
        assert_eq!(t.bundle_disk_path, PathBuf::from("content/writing/foo.md"));
    }

    #[test]
    fn parse_promote_path_rejects_leading_slash() {
        assert_eq!(
            promote_path_error("/papers/bar.md"),
            MempoolEntryPathError::Absolute
        );
    }

    #[test]
    fn sidecar_path_matches_generated_content_sidecar() {
        let t = parse_promote_path("writing/foo.md").unwrap();
        assert_eq!(
            t.sidecar_disk_path(),
            PathBuf::from("content/writing/foo.meta.json")
        );
    }

    #[test]
    fn stage_paths_include_generated_sidecar() {
        let t = parse_promote_path("writing/foo.md").unwrap();
        let paths = stage_paths(&t, false);
        assert!(paths.contains(&PathBuf::from("content/writing/foo.md")));
        assert!(paths.contains(&PathBuf::from("content/writing/foo.meta.json")));
        assert!(paths.contains(&PathBuf::from("content/.websh/ledger.json")));
        assert!(paths.contains(&PathBuf::from("content/manifest.json")));
        assert!(!paths.contains(&PathBuf::from(ATTESTATIONS_PATH)));

        let paths = stage_paths(&t, true);
        assert!(paths.contains(&PathBuf::from(ATTESTATIONS_PATH)));
    }

    #[test]
    fn parse_promote_path_rejects_non_md_extension() {
        assert_eq!(
            promote_path_error("writing/foo.txt"),
            MempoolEntryPathError::Extension
        );
    }

    #[test]
    fn parse_promote_path_rejects_unknown_category() {
        assert_eq!(
            promote_path_error("fiction/foo.md"),
            MempoolEntryPathError::UnknownCategory("fiction".to_string())
        );
    }

    #[test]
    fn parse_promote_path_rejects_nested_slug() {
        assert_eq!(
            promote_path_error("writing/series/foo.md"),
            MempoolEntryPathError::Shape
        );
    }

    #[test]
    fn parse_promote_path_rejects_missing_slug() {
        assert_eq!(
            promote_path_error("writing/.md"),
            MempoolEntryPathError::Slug
        );
    }

    #[test]
    fn preflight_rejects_dirty_command_owned_paths() {
        let root = temp_repo("dirty-owned");
        write(&root, "content/manifest.json", "{}\n");
        write(&root, "content/other.md", "original\n");
        commit_all(&root);

        write(&root, "content/manifest.json", "{\"changed\":true}\n");
        let target = parse_promote_path("writing/foo.md").unwrap();
        let err = ensure_promote_worktree_ready(&root, &target).unwrap_err();
        assert!(err.to_string().contains("command-owned"));
        assert!(err.to_string().contains("content/manifest.json"));
    }

    #[test]
    fn preflight_rejects_unrelated_content_changes() {
        let root = temp_repo("dirty-unrelated");
        write(&root, "content/manifest.json", "{}\n");
        write(&root, "content/other.md", "original\n");
        commit_all(&root);

        write(&root, "content/other.md", "user edit\n");
        let target = parse_promote_path("writing/foo.md").unwrap();
        let err = ensure_promote_worktree_ready(&root, &target).unwrap_err();
        assert!(err.to_string().contains("Stage/stash"));
        assert!(err.to_string().contains("content/other.md"));
    }

    #[test]
    fn branch_mismatch_noninteractive_fails_fast() {
        let root = temp_repo("branch-noninteractive");
        write(&root, "content/manifest.json", "{}\n");
        commit_all(&root);
        run_git(&root, ["checkout", "-b", "not-deploy-branch"]);

        let err =
            confirm_on_bundle_branch(&root, InteractionMode::NonInteractive, false).unwrap_err();
        assert!(err.to_string().contains("--allow-branch-mismatch"));
    }

    #[test]
    fn branch_mismatch_can_be_explicitly_allowed() {
        let root = temp_repo("branch-allowed");
        write(&root, "content/manifest.json", "{}\n");
        commit_all(&root);
        run_git(&root, ["checkout", "-b", "not-deploy-branch"]);

        confirm_on_bundle_branch(&root, InteractionMode::NonInteractive, true).unwrap();
    }

    #[test]
    fn write_set_validation_rejects_new_unexpected_paths() {
        let root = temp_repo("write-set");
        write(&root, "content/.websh/ledger.json", "ledger old\n");
        write(&root, "content/manifest.json", "manifest old\n");
        commit_all(&root);

        let target = parse_promote_path("writing/foo.md").unwrap();
        let baseline = git_status_snapshot(&root).unwrap();
        write(&root, "content/writing/foo.md", "body\n");
        write(&root, "content/writing/foo.meta.json", "sidecar\n");
        write(&root, "content/.websh/ledger.json", "ledger new\n");
        write(&root, "content/manifest.json", "manifest new\n");
        write(&root, "unexpected/generated.txt", "bad\n");

        let err = validate_promote_write_set(&root, &target, false, &baseline).unwrap_err();
        assert!(err.to_string().contains("unexpected/generated.txt"));
    }

    #[test]
    fn write_set_validation_ignores_preexisting_unrelated_dirty_paths() {
        let root = temp_repo("write-set-baseline");
        write(&root, "content/.websh/ledger.json", "ledger old\n");
        write(&root, "content/manifest.json", "manifest old\n");
        write(&root, "scratch.txt", "user old\n");
        commit_all(&root);

        write(&root, "scratch.txt", "user edit\n");
        let target = parse_promote_path("writing/foo.md").unwrap();
        let baseline = git_status_snapshot(&root).unwrap();
        write(&root, "content/writing/foo.md", "body\n");
        write(&root, "content/writing/foo.meta.json", "sidecar\n");
        write(&root, "content/.websh/ledger.json", "ledger new\n");
        write(&root, "content/manifest.json", "manifest new\n");

        validate_promote_write_set(&root, &target, false, &baseline).unwrap();
    }

    #[test]
    fn rollback_restores_only_command_owned_paths() {
        let root = temp_repo("rollback");
        write(&root, "content/.websh/ledger.json", "ledger old\n");
        write(&root, "content/manifest.json", "manifest old\n");
        write(&root, ATTESTATIONS_PATH, "attest old\n");
        write(&root, "content/other.md", "other old\n");
        commit_all(&root);

        write(&root, "content/other.md", "other staged edit\n");
        run_git(&root, ["add", "--", "content/other.md"]);
        write(&root, "content/.websh/ledger.json", "ledger new\n");
        write(&root, "content/manifest.json", "manifest new\n");
        write(&root, ATTESTATIONS_PATH, "attest new\n");
        write(&root, "content/writing/foo.md", "body\n");
        write(&root, "content/writing/foo.meta.json", "sidecar\n");
        run_git(
            &root,
            [
                "add",
                "--",
                "content/.websh/ledger.json",
                "content/manifest.json",
                ATTESTATIONS_PATH,
                "content/writing/foo.md",
                "content/writing/foo.meta.json",
            ],
        );

        let target = parse_promote_path("writing/foo.md").unwrap();
        rollback(
            &root,
            &target,
            &PromoteCleanup {
                body_written: true,
                sidecar_written: true,
                ledger_written: true,
                manifest_written: true,
                attest_written: true,
            },
        );

        assert!(!root.join("content/writing/foo.md").exists());
        assert!(!root.join("content/writing/foo.meta.json").exists());
        assert_eq!(
            fs::read_to_string(root.join("content/.websh/ledger.json")).unwrap(),
            "ledger old\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("content/manifest.json")).unwrap(),
            "manifest old\n"
        );
        assert_eq!(
            fs::read_to_string(root.join(ATTESTATIONS_PATH)).unwrap(),
            "attest old\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("content/other.md")).unwrap(),
            "other staged edit\n"
        );
        let other_status =
            git_status_for_paths(&root, &[PathBuf::from("content/other.md")]).unwrap();
        assert!(
            other_status.starts_with("M  content/other.md"),
            "{other_status:?}"
        );
    }
}
