//! Canonical public crypto artifact paths and bundled deployed artifacts.

use websh_core::attestation::artifact::AttestationArtifact;
use websh_core::crypto::ack::AckArtifact;

pub const ATTESTATIONS_PATH: &str = "assets/crypto/attestations.json";
pub const ACK_ARTIFACT_PATH: &str = "assets/crypto/ack.commitment.json";

pub const ATTESTATIONS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/crypto/attestations.json"
));

pub const ACK_COMMITMENT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/crypto/ack.commitment.json"
));

pub type SiteArtifactResult<T> = Result<T, SiteArtifactError>;

#[derive(Debug, thiserror::Error)]
pub enum SiteArtifactError {
    #[error("parse bundled attestation artifact: {source}")]
    Attestations {
        #[source]
        source: serde_json::Error,
    },
    #[error("parse bundled ACK artifact: {source}")]
    Ack {
        #[source]
        source: serde_json::Error,
    },
}

pub fn attestation_artifact() -> SiteArtifactResult<AttestationArtifact> {
    AttestationArtifact::from_json_str(ATTESTATIONS_JSON)
        .map_err(|source| SiteArtifactError::Attestations { source })
}

pub fn ack_artifact() -> SiteArtifactResult<AckArtifact> {
    AckArtifact::from_json_str(ACK_COMMITMENT_JSON)
        .map_err(|source| SiteArtifactError::Ack { source })
}
