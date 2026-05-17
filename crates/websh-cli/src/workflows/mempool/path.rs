use websh_core::mempool::LEDGER_CATEGORIES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MempoolEntryPath(String);

impl MempoolEntryPath {
    pub(crate) fn parse(raw: &str) -> Result<Self, MempoolEntryPathError> {
        if raw.is_empty() {
            return Err(MempoolEntryPathError::Empty);
        }
        if raw.starts_with('/') {
            return Err(MempoolEntryPathError::Absolute);
        }
        if raw == "manifest.json" || raw.ends_with("/manifest.json") {
            return Err(MempoolEntryPathError::Reserved);
        }

        let parts: Vec<&str> = raw.split('/').collect();
        if parts.len() != 2 {
            return Err(MempoolEntryPathError::Shape);
        }
        if parts.iter().any(|part| part.is_empty()) {
            return Err(MempoolEntryPathError::EmptySegment);
        }
        if parts.iter().any(|part| matches!(*part, "." | "..")) {
            return Err(MempoolEntryPathError::Traversal);
        }
        if !LEDGER_CATEGORIES.contains(&parts[0]) {
            return Err(MempoolEntryPathError::UnknownCategory(parts[0].to_string()));
        }
        let Some(slug) = parts[1].strip_suffix(".md") else {
            return Err(MempoolEntryPathError::Extension);
        };
        if !slug_is_valid(slug) {
            return Err(MempoolEntryPathError::Slug);
        }

        Ok(Self(raw.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for MempoolEntryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum MempoolEntryPathError {
    #[error("mempool entry path is empty")]
    Empty,
    #[error("mempool entry path must be repo-relative")]
    Absolute,
    #[error("mempool entry path targets a reserved file")]
    Reserved,
    #[error("mempool entry path must be <category>/<slug>.md")]
    Shape,
    #[error("mempool entry path contains an empty segment")]
    EmptySegment,
    #[error("mempool entry path cannot contain . or ..")]
    Traversal,
    #[error("unknown mempool category `{0}`")]
    UnknownCategory(String),
    #[error("mempool entry path must end in .md")]
    Extension,
    #[error("mempool entry slug must be lowercase ASCII letters, digits, and hyphens")]
    Slug,
}

fn slug_is_valid(slug: &str) -> bool {
    if slug.is_empty() {
        return false;
    }
    let bytes = slug.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_entry_path() {
        assert_eq!(
            MempoolEntryPath::parse("writing/hello-world.md")
                .unwrap()
                .as_str(),
            "writing/hello-world.md"
        );
    }

    #[test]
    fn rejects_reserved_or_escaping_paths() {
        for raw in [
            "",
            "/writing/a.md",
            "manifest.json",
            "writing/../manifest.json",
            "writing//a.md",
            "writing/a.txt",
            "unknown/a.md",
            "writing/-bad.md",
        ] {
            assert!(MempoolEntryPath::parse(raw).is_err(), "{raw} should fail");
        }
    }
}
