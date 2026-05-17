use std::path::Path;
use std::process::Command;

use anyhow::Context;

use crate::CliResult;

pub(crate) struct GitOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) success: bool,
}

pub(crate) fn git_output<I, S>(root: &Path, args: I) -> CliResult<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .with_context(|| format!("run git in {}", root.display()))?;
    Ok(GitOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        success: output.status.success(),
    })
}

pub(crate) fn git_status<I, S>(root: &Path, args: I) -> CliResult<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Ok(Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .with_context(|| format!("run git in {}", root.display()))?
        .success())
}

pub(crate) fn run_git_best_effort<I, S>(root: &Path, args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = Command::new("git");
    cmd.current_dir(root).args(args);
    cmd.output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
