//! Attestation artifact (collection of typed `Subject`s with their signatures).
//!
//! The data shape of subjects lives in `crate::engine::attestation::subject`; this module
//! holds the on-disk artifact wrapper, the `Attestation` enum (the actual
//! signature payloads), and the constants/hash helpers shared across the
//! crypto modules.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use crate::engine::attestation::subject::{
    BundleSubject, ContentFile, DocumentSubject, Envelope, HomepageSubject, LedgerSubject,
    PageSubject, Subject, SubjectCanonicalError, SubjectValidationError, compute_content_sha256,
    subject_id_for_route,
};

pub const ATTESTATIONS_SCHEME: &str = "websh.attestations.v1";
pub const SUBJECT_MESSAGE_SCHEME: &str = "websh.subject.v1";
pub const CONTENT_HASH: &str = "sha256";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationArtifact {
    pub version: u32,
    pub scheme: String,
    pub subjects: Vec<Subject>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Attestation {
    Pgp {
        #[serde(skip_serializing_if = "Option::is_none")]
        signer: Option<String>,
        fingerprint: String,
        key_path: String,
        signature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature_path: Option<String>,
        message_sha256: String,
        verified: bool,
    },
    Ethereum {
        scheme: String,
        signer: String,
        address: String,
        signature: String,
        recovered_address: String,
        message_sha256: String,
        verified: bool,
    },
}

impl Default for AttestationArtifact {
    fn default() -> Self {
        Self {
            version: 1,
            scheme: ATTESTATIONS_SCHEME.to_string(),
            subjects: Vec::new(),
        }
    }
}

impl AttestationArtifact {
    pub fn from_json_str(body: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(body)
    }

    pub fn subject_for_route(&self, route: &str) -> Option<&Subject> {
        self.subjects
            .iter()
            .find(|subject| subject.route() == route)
    }

    pub fn subject_for_route_mut(&mut self, route: &str) -> Option<&mut Subject> {
        self.subjects
            .iter_mut()
            .find(|subject| subject.route() == route)
    }

    pub fn validate_header(&self) -> Result<(), AttestationArtifactError> {
        if self.version != 1 {
            return Err(AttestationArtifactError::UnsupportedVersion {
                version: self.version,
            });
        }
        if self.scheme != ATTESTATIONS_SCHEME {
            return Err(AttestationArtifactError::UnsupportedScheme {
                scheme: self.scheme.clone(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AttestationArtifactError {
    #[error("unsupported attestations version {version}")]
    UnsupportedVersion { version: u32 },
    #[error("unsupported attestations scheme {scheme}")]
    UnsupportedScheme { scheme: String },
}

impl Attestation {
    pub fn message_sha256(&self) -> &str {
        match self {
            Self::Pgp { message_sha256, .. } | Self::Ethereum { message_sha256, .. } => {
                message_sha256
            }
        }
    }

    pub fn verified(&self) -> bool {
        match self {
            Self::Pgp { verified, .. } | Self::Ethereum { verified, .. } => *verified,
        }
    }
}

pub fn message_sha256(message: &str) -> String {
    sha256_hex(message.as_bytes())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("0x{}", hex::encode(hasher.finalize()))
}
