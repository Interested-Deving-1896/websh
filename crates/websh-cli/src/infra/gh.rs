//! Shared helpers for invoking the `gh` CLI as a subprocess.
//!
//! Native GitHub workflows shell out to `gh` for API access. Centralizing
//! the boilerplate keeps their auth model identical and avoids re-inventing
//! the require/check/capture patterns.

use std::ffi::OsStr;
use std::process::{Command as Process, Stdio};

use anyhow::{Context, bail};

use crate::CliResult;

/// Verify that the `gh` CLI is installed and on `PATH`. Does not check
/// authentication — that's enforced by the actual `gh api` calls.
pub(crate) fn require_gh() -> CliResult {
    let probe = Process::new("gh").arg("--version").output();
    match probe {
        Ok(out) if out.status.success() => Ok(()),
        _ => bail!(
            "the `gh` CLI is required (https://cli.github.com); \
             ensure `gh auth status` reports an authenticated account before re-running"
        ),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GhResourceStatus {
    Exists,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum GhApiOutput {
    Success(String),
    Missing,
}

/// Run `gh` with the given args and classify only explicit GitHub 404/not-found
/// responses as `Missing`. Auth, permission, rate-limit, network, malformed
/// response, and other command failures are surfaced as errors.
pub(crate) fn gh_capture_status<I, S>(args: I) -> CliResult<GhApiOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Process::new("gh").args(args).output().context("run gh")?;
    if out.status.success() {
        return Ok(GhApiOutput::Success(
            String::from_utf8_lossy(&out.stdout).into_owned(),
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if is_github_not_found(&stdout) || is_github_not_found(&stderr) {
        return Ok(GhApiOutput::Missing);
    }
    bail!(
        "gh failed (exit {}): {}",
        out.status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "?".to_string()),
        stderr.trim()
    )
}

pub(crate) fn gh_resource_status<I, S>(args: I) -> CliResult<GhResourceStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    match gh_capture_status(args)? {
        GhApiOutput::Success(_) => Ok(GhResourceStatus::Exists),
        GhApiOutput::Missing => Ok(GhResourceStatus::Missing),
    }
}

/// Run `gh` with the given args, capture stdout as a `String`. Errors when
/// the process exits non-zero — stderr is included in the error message
/// so the caller can surface it without re-running.
pub(crate) fn gh_capture<I, S>(args: I) -> CliResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Process::new("gh").args(args).output().context("run gh")?;
    if !out.status.success() {
        bail!(
            "gh failed (exit {}): {}",
            out.status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".to_string()),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn gh_status<I, S>(args: I) -> CliResult<bool>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(Process::new("gh")
        .args(args)
        .stdout(Stdio::null())
        .status()
        .context("run gh")?
        .success())
}

fn is_github_not_found(output: &str) -> bool {
    output.contains("HTTP 404")
        || output.contains("HTTP 404 Not Found")
        || output.contains("Not Found (HTTP 404)")
        || output.contains("\"status\":\"404\"")
        || output.contains("\"message\":\"Not Found\"")
}
