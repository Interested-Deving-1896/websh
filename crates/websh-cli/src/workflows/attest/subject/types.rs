use std::path::PathBuf;

use anyhow::bail;

use crate::CliResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::workflows::attest) enum SubjectKind {
    Homepage,
    Ledger,
    Document,
    Page,
    Bundle,
}

impl SubjectKind {
    pub(super) fn parse(value: &str) -> CliResult<Self> {
        match value {
            "homepage" => Ok(Self::Homepage),
            "ledger" => Ok(Self::Ledger),
            "document" => Ok(Self::Document),
            "page" => Ok(Self::Page),
            "bundle" => Ok(Self::Bundle),
            other => bail!("unsupported subject kind: {other}"),
        }
    }
}

#[derive(Clone)]
pub(in crate::workflows::attest) struct SubjectSpec {
    pub(in crate::workflows::attest) route: String,
    pub(in crate::workflows::attest) kind: SubjectKind,
    pub(in crate::workflows::attest) content_paths: Vec<PathBuf>,
}
