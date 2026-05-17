//! Typed attestation subjects.
//!
//! `Subject` is a tagged enum where each variant owns the fields its
//! signature actually binds. A homepage subject carries `ack_combined_root`;
//! a ledger subject carries `chain_head`; documents and pages bind only
//! their content. Stored fields are exactly the irreducible facts —
//! `id`, `content_sha256`, and the canonical signed message are all
//! derived from the variant via methods on `Subject`.

use serde::{Deserialize, Serialize};

use crate::engine::attestation::artifact::{Attestation, SUBJECT_MESSAGE_SCHEME, sha256_hex};

/// One file contributing to a subject's content fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

/// Fields common to every subject variant.
///
/// Flattened into each variant via `#[serde(flatten)]` so the JSON shape is
/// `{ "kind": "...", "route": "...", "issued_at": "...", "content_files": [...], "attestations": [...], <variant fields> }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub route: String,
    pub issued_at: String,
    pub content_files: Vec<ContentFile>,
    pub attestations: Vec<Attestation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomepageSubject {
    #[serde(flatten)]
    pub env: Envelope,
    pub ack_combined_root: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerSubject {
    #[serde(flatten)]
    pub env: Envelope,
    pub chain_head: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSubject {
    #[serde(flatten)]
    pub env: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageSubject {
    #[serde(flatten)]
    pub env: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleSubject {
    #[serde(flatten)]
    pub env: Envelope,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Subject {
    Homepage(HomepageSubject),
    Ledger(LedgerSubject),
    Document(DocumentSubject),
    Page(PageSubject),
    Bundle(BundleSubject),
}

impl Subject {
    pub fn envelope(&self) -> &Envelope {
        match self {
            Subject::Homepage(s) => &s.env,
            Subject::Ledger(s) => &s.env,
            Subject::Document(s) => &s.env,
            Subject::Page(s) => &s.env,
            Subject::Bundle(s) => &s.env,
        }
    }

    pub fn envelope_mut(&mut self) -> &mut Envelope {
        match self {
            Subject::Homepage(s) => &mut s.env,
            Subject::Ledger(s) => &mut s.env,
            Subject::Document(s) => &mut s.env,
            Subject::Page(s) => &mut s.env,
            Subject::Bundle(s) => &mut s.env,
        }
    }

    pub fn route(&self) -> &str {
        &self.envelope().route
    }

    pub fn issued_at(&self) -> &str {
        &self.envelope().issued_at
    }

    pub fn content_files(&self) -> &[ContentFile] {
        &self.envelope().content_files
    }

    pub fn attestations(&self) -> &[Attestation] {
        &self.envelope().attestations
    }

    pub fn attestations_mut(&mut self) -> &mut Vec<Attestation> {
        &mut self.envelope_mut().attestations
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            Subject::Homepage(_) => "homepage",
            Subject::Ledger(_) => "ledger",
            Subject::Document(_) => "document",
            Subject::Page(_) => "page",
            Subject::Bundle(_) => "bundle",
        }
    }

    pub fn id(&self) -> String {
        subject_id_for_route(self.route())
    }

    pub fn content_sha256(&self) -> Result<String, SubjectCanonicalError> {
        compute_content_sha256(self.content_files())
    }

    /// The canonical text bound by attestation signatures.
    ///
    /// Field order is part of the contract — changes invalidate every existing signature.
    pub fn canonical_message(&self) -> Result<String, SubjectCanonicalError> {
        let id = self.id();
        let content_sha256 = self.content_sha256()?;
        let env = self.envelope();
        let body = match self {
            Subject::Homepage(s) => format!(
                "id={id}\nroute={route}\nkind=homepage\ncontent_sha256={content_sha256}\nack_combined_root={ack}\nissued_at={issued_at}",
                route = env.route,
                ack = s.ack_combined_root,
                issued_at = env.issued_at,
            ),
            Subject::Ledger(s) => format!(
                "id={id}\nroute={route}\nkind=ledger\ncontent_sha256={content_sha256}\nchain_head={head}\nissued_at={issued_at}",
                route = env.route,
                head = s.chain_head,
                issued_at = env.issued_at,
            ),
            Subject::Document(_) => format!(
                "id={id}\nroute={route}\nkind=document\ncontent_sha256={content_sha256}\nissued_at={issued_at}",
                route = env.route,
                issued_at = env.issued_at,
            ),
            Subject::Page(_) => format!(
                "id={id}\nroute={route}\nkind=page\ncontent_sha256={content_sha256}\nissued_at={issued_at}",
                route = env.route,
                issued_at = env.issued_at,
            ),
            Subject::Bundle(_) => format!(
                "id={id}\nroute={route}\nkind=bundle\ncontent_sha256={content_sha256}\nissued_at={issued_at}",
                route = env.route,
                issued_at = env.issued_at,
            ),
        };
        Ok(format!("{SUBJECT_MESSAGE_SCHEME}\n{body}"))
    }

    /// Static structural checks that don't require disk or network access.
    ///
    /// - `content_files` must be strictly sorted by path with no duplicates
    /// - `canonical_message` must serialize without error
    pub fn validate(&self) -> Result<(), SubjectValidationError> {
        let env = self.envelope();
        let mut last: Option<&str> = None;
        for file in &env.content_files {
            if let Some(prev) = last {
                if file.path.as_str() == prev {
                    return Err(SubjectValidationError::DuplicateContentPath {
                        path: file.path.clone(),
                    });
                }
                if file.path.as_str() < prev {
                    return Err(SubjectValidationError::UnsortedContentFiles {
                        previous: prev.to_string(),
                        current: file.path.clone(),
                    });
                }
            }
            last = Some(file.path.as_str());
        }
        self.canonical_message()?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SubjectCanonicalError {
    #[error("serialize subject content files: {source}")]
    ContentFiles {
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum SubjectValidationError {
    #[error("duplicate content path: {path}")]
    DuplicateContentPath { path: String },
    #[error("content_files not strictly sorted: {previous} > {current}")]
    UnsortedContentFiles { previous: String, current: String },
    #[error("canonical_message failed: {source}")]
    Canonical {
        #[from]
        source: SubjectCanonicalError,
    },
}

pub fn subject_id_for_route(route: &str) -> String {
    format!("route:{route}")
}

pub fn compute_content_sha256(files: &[ContentFile]) -> Result<String, SubjectCanonicalError> {
    serde_json::to_vec(files)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|source| SubjectCanonicalError::ContentFiles { source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_files() -> Vec<ContentFile> {
        vec![
            ContentFile {
                path: "a.txt".to_string(),
                sha256: "0xaaa".to_string(),
                bytes: 3,
            },
            ContentFile {
                path: "b.txt".to_string(),
                sha256: "0xbbb".to_string(),
                bytes: 4,
            },
        ]
    }

    fn homepage() -> Subject {
        Subject::Homepage(HomepageSubject {
            env: Envelope {
                route: "/".to_string(),
                issued_at: "2026-04-30".to_string(),
                content_files: sample_files(),
                attestations: Vec::new(),
            },
            ack_combined_root: "0xack".to_string(),
        })
    }

    fn ledger() -> Subject {
        Subject::Ledger(LedgerSubject {
            env: Envelope {
                route: "/ledger".to_string(),
                issued_at: "2026-04-30".to_string(),
                content_files: sample_files(),
                attestations: Vec::new(),
            },
            chain_head: "0xhead".to_string(),
        })
    }

    fn document() -> Subject {
        Subject::Document(DocumentSubject {
            env: Envelope {
                route: "/keys/wonjae.asc".to_string(),
                issued_at: "2026-04-30".to_string(),
                content_files: sample_files(),
                attestations: Vec::new(),
            },
        })
    }

    fn page() -> Subject {
        Subject::Page(PageSubject {
            env: Envelope {
                route: "/papers/tabula".to_string(),
                issued_at: "2026-04-30".to_string(),
                content_files: sample_files(),
                attestations: Vec::new(),
            },
        })
    }

    fn bundle() -> Subject {
        Subject::Bundle(BundleSubject {
            env: Envelope {
                route: "/writing/foo".to_string(),
                issued_at: "2026-04-30".to_string(),
                content_files: sample_files(),
                attestations: Vec::new(),
            },
        })
    }

    #[test]
    fn id_is_route_prefixed() {
        assert_eq!(homepage().id(), "route:/");
        assert_eq!(ledger().id(), "route:/ledger");
        assert_eq!(document().id(), "route:/keys/wonjae.asc");
        assert_eq!(bundle().id(), "route:/writing/foo");
    }

    #[test]
    fn kind_str_matches_variant() {
        assert_eq!(homepage().kind_str(), "homepage");
        assert_eq!(ledger().kind_str(), "ledger");
        assert_eq!(document().kind_str(), "document");
        assert_eq!(page().kind_str(), "page");
        assert_eq!(bundle().kind_str(), "bundle");
    }

    #[test]
    fn canonical_message_homepage_is_exact() {
        let subject = homepage();
        let content_sha = subject.content_sha256().unwrap();
        let expected = format!(
            "websh.subject.v1\nid=route:/\nroute=/\nkind=homepage\ncontent_sha256={content_sha}\nack_combined_root=0xack\nissued_at=2026-04-30"
        );
        assert_eq!(subject.canonical_message().unwrap(), expected);
    }

    #[test]
    fn canonical_message_ledger_is_exact() {
        let subject = ledger();
        let content_sha = subject.content_sha256().unwrap();
        let expected = format!(
            "websh.subject.v1\nid=route:/ledger\nroute=/ledger\nkind=ledger\ncontent_sha256={content_sha}\nchain_head=0xhead\nissued_at=2026-04-30"
        );
        assert_eq!(subject.canonical_message().unwrap(), expected);
    }

    #[test]
    fn canonical_message_document_is_exact() {
        let subject = document();
        let content_sha = subject.content_sha256().unwrap();
        let expected = format!(
            "websh.subject.v1\nid=route:/keys/wonjae.asc\nroute=/keys/wonjae.asc\nkind=document\ncontent_sha256={content_sha}\nissued_at=2026-04-30"
        );
        assert_eq!(subject.canonical_message().unwrap(), expected);
    }

    #[test]
    fn canonical_message_page_is_exact() {
        let subject = page();
        let content_sha = subject.content_sha256().unwrap();
        let expected = format!(
            "websh.subject.v1\nid=route:/papers/tabula\nroute=/papers/tabula\nkind=page\ncontent_sha256={content_sha}\nissued_at=2026-04-30"
        );
        assert_eq!(subject.canonical_message().unwrap(), expected);
    }

    #[test]
    fn canonical_message_bundle_is_exact() {
        let subject = bundle();
        let content_sha = subject.content_sha256().unwrap();
        let expected = format!(
            "websh.subject.v1\nid=route:/writing/foo\nroute=/writing/foo\nkind=bundle\ncontent_sha256={content_sha}\nissued_at=2026-04-30"
        );
        assert_eq!(subject.canonical_message().unwrap(), expected);
    }

    #[test]
    fn canonical_message_is_deterministic() {
        let subject = homepage();
        assert_eq!(
            subject.canonical_message().unwrap(),
            subject.canonical_message().unwrap()
        );
    }

    #[test]
    fn content_sha256_is_stable_for_same_files() {
        let files = sample_files();
        assert_eq!(
            compute_content_sha256(&files).unwrap(),
            compute_content_sha256(&files).unwrap()
        );
    }

    #[test]
    fn content_sha256_differs_when_files_differ() {
        let mut files = sample_files();
        let baseline = compute_content_sha256(&files).unwrap();
        files[0].bytes += 1;
        assert_ne!(compute_content_sha256(&files).unwrap(), baseline);
    }

    #[test]
    fn validate_accepts_well_formed_subject() {
        assert!(homepage().validate().is_ok());
        assert!(ledger().validate().is_ok());
        assert!(document().validate().is_ok());
        assert!(page().validate().is_ok());
        assert!(bundle().validate().is_ok());
    }

    #[test]
    fn validate_rejects_unsorted_content_files() {
        let mut subject = homepage();
        subject.envelope_mut().content_files = vec![
            ContentFile {
                path: "b.txt".to_string(),
                sha256: "0xbbb".to_string(),
                bytes: 4,
            },
            ContentFile {
                path: "a.txt".to_string(),
                sha256: "0xaaa".to_string(),
                bytes: 3,
            },
        ];
        assert!(subject.validate().is_err());
    }

    #[test]
    fn validate_rejects_duplicate_content_paths() {
        let mut subject = homepage();
        subject.envelope_mut().content_files = vec![
            ContentFile {
                path: "a.txt".to_string(),
                sha256: "0xaaa".to_string(),
                bytes: 3,
            },
            ContentFile {
                path: "a.txt".to_string(),
                sha256: "0xbbb".to_string(),
                bytes: 4,
            },
        ];
        assert!(subject.validate().is_err());
    }

    #[test]
    fn serde_roundtrip_homepage() {
        let subject = homepage();
        let json = serde_json::to_string(&subject).unwrap();
        let back: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
        assert!(json.contains("\"kind\":\"homepage\""));
        assert!(json.contains("\"ack_combined_root\""));
        assert!(!json.contains("\"chain_head\""));
    }

    #[test]
    fn serde_roundtrip_ledger() {
        let subject = ledger();
        let json = serde_json::to_string(&subject).unwrap();
        let back: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
        assert!(json.contains("\"kind\":\"ledger\""));
        assert!(json.contains("\"chain_head\""));
        assert!(!json.contains("\"ack_combined_root\""));
    }

    #[test]
    fn serde_roundtrip_document() {
        let subject = document();
        let json = serde_json::to_string(&subject).unwrap();
        let back: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
        assert!(json.contains("\"kind\":\"document\""));
        assert!(!json.contains("\"chain_head\""));
        assert!(!json.contains("\"ack_combined_root\""));
    }

    #[test]
    fn serde_roundtrip_page() {
        let subject = page();
        let json = serde_json::to_string(&subject).unwrap();
        let back: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
        assert!(json.contains("\"kind\":\"page\""));
    }

    #[test]
    fn serde_roundtrip_bundle() {
        let subject = bundle();
        let json = serde_json::to_string(&subject).unwrap();
        let back: Subject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
        assert!(json.contains("\"kind\":\"bundle\""));
    }

    #[test]
    fn serde_requires_attestations_field() {
        let json = r#"{
            "kind": "page",
            "route": "/papers/tabula",
            "issued_at": "2026-04-30",
            "content_files": []
        }"#;

        let parsed = serde_json::from_str::<Subject>(json);
        assert!(parsed.is_err());
    }

    #[test]
    fn json_does_not_contain_derived_fields() {
        let subject = homepage();
        let json = serde_json::to_string(&subject).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(!json.contains("\"content_sha256\""));
        assert!(!json.contains("\"message\""));
    }
}
