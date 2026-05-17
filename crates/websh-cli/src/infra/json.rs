use std::fs;
use std::path::Path;

use anyhow::Context;

use crate::CliResult;

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> CliResult<T> {
    let body = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&body).with_context(|| format!("parse json {}", path.display()))
}

pub(crate) fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> CliResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(value)
            .with_context(|| format!("serialize json {}", path.display()))?
    );
    // Skip the write entirely when the on-disk content already matches.
    // This keeps `cargo run -- content manifest` truly idempotent so it can
    // be invoked from a Trunk pre_build hook without the resulting mtime
    // bump triggering another rebuild and looping forever.
    if let Ok(existing) = fs::read(path)
        && existing == body.as_bytes()
    {
        return Ok(());
    }
    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
