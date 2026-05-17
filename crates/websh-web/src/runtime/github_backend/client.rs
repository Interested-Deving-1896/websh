//! GitHub backend — GraphQL createCommitOnBranch + manifest fetch.
//! See spec §4.2 / §4.3.

use serde::{Deserialize, Serialize};

use websh_core::domain::VirtualPath;
use websh_core::ports::{
    CommitBase, CommitOutcome, CommitRequest, LocalBoxFuture, ScannedSubtree, StorageBackend,
    StorageError, StorageResult, parse_manifest_snapshot, serialize_manifest_snapshot,
};

use super::graphql::{
    BranchRef, CommitMessage, CreateCommitInput, GraphQLOperationBuildError, build_file_changes,
};
use super::path::{
    RepoPathError, encoded_repo_relative_path, normalize_repo_prefix, prefixed_repo_path,
};

pub struct GitHubBackend {
    repo_with_owner: String,
    branch: String,
    mount_root: VirtualPath,
    content_prefix: String,
    gateway: String,
    allow_missing_manifest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GitHubBackendConfigError {
    #[error("invalid repo_with_owner `{value}`")]
    InvalidRepo { value: String },
    #[error("invalid content prefix: {source}")]
    InvalidContentPrefix {
        #[from]
        source: RepoPathError,
    },
}

impl GitHubBackend {
    pub fn new(
        repo_with_owner: impl Into<String>,
        branch: impl Into<String>,
        mount_root: VirtualPath,
        content_prefix: impl Into<String>,
        gateway: impl Into<String>,
    ) -> Result<Self, GitHubBackendConfigError> {
        Self::new_with_manifest_policy(
            repo_with_owner,
            branch,
            mount_root,
            content_prefix,
            gateway,
            true,
        )
    }

    pub fn new_with_manifest_policy(
        repo_with_owner: impl Into<String>,
        branch: impl Into<String>,
        mount_root: VirtualPath,
        content_prefix: impl Into<String>,
        gateway: impl Into<String>,
        allow_missing_manifest: bool,
    ) -> Result<Self, GitHubBackendConfigError> {
        let repo_with_owner = repo_with_owner.into();
        validate_repo_with_owner(&repo_with_owner)?;
        Ok(Self {
            repo_with_owner,
            branch: branch.into(),
            mount_root,
            content_prefix: normalize_repo_prefix(&content_prefix.into())?,
            gateway: gateway.into().trim_end_matches('/').to_string(),
            allow_missing_manifest,
        })
    }

    fn base_url(&self) -> String {
        if self.gateway == "self" {
            return if self.content_prefix.is_empty() {
                ".".to_string()
            } else {
                encoded_repo_relative_path(&self.content_prefix, false)
                    .expect("normalized content prefix must be URL-encodable")
            };
        }

        if self.content_prefix.is_empty() {
            format!("{}/{}/{}", self.gateway, self.repo_with_owner, self.branch)
        } else {
            let encoded_prefix = encoded_repo_relative_path(&self.content_prefix, false)
                .expect("normalized content prefix must be URL-encodable");
            format!(
                "{}/{}/{}/{}",
                self.gateway, self.repo_with_owner, self.branch, encoded_prefix
            )
        }
    }

    fn manifest_url(&self) -> String {
        format!("{}/manifest.json", self.base_url())
    }

    fn content_url(&self, rel_path: &str) -> Result<String, RepoPathError> {
        let base_url = self.base_url();
        let rel_path = encoded_repo_relative_path(rel_path.trim_start_matches('/'), true)?;
        if rel_path.is_empty() {
            Ok(base_url)
        } else {
            Ok(format!("{base_url}/{rel_path}"))
        }
    }

    /// Fetch the current HEAD OID of the configured branch via the GitHub
    /// GraphQL API. Returns `None` if the branch exists but has no commits
    /// (very rare — typically only just-initialized branches), and an error
    /// for network / auth / not-found conditions.
    async fn fetch_branch_head_oid(&self, token: &str) -> StorageResult<Option<String>> {
        let (owner, name) =
            self.repo_with_owner
                .split_once('/')
                .ok_or_else(|| StorageError::InvalidRequest {
                    message: "invalid repo_with_owner".into(),
                })?;
        let body = HeadQueryRequest {
            query: HEAD_QUERY,
            variables: HeadQueryVariables {
                owner,
                name,
                qualified_name: format!("refs/heads/{}", self.branch),
            },
        };
        let body_json = serde_json::to_string(&body).map_err(|e| StorageError::InvalidRequest {
            message: e.to_string(),
        })?;
        let resp = gloo_net::http::Request::post(GRAPHQL_ENDPOINT)
            .header("Authorization", &format!("bearer {}", token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "websh/0.1")
            .body(body_json)
            .map_err(|e| StorageError::InvalidRequest {
                message: e.to_string(),
            })?
            .send()
            .await
            .map_err(|e| StorageError::Network {
                message: e.to_string(),
            })?;
        let status = resp.status();
        if !(200..300).contains(&status) {
            return Err(map_http_status(status, retry_after_header(&resp)));
        }
        let parsed: HeadQueryResponse = resp.json().await.map_err(|e| StorageError::Network {
            message: e.to_string(),
        })?;
        if !parsed.errors.is_empty() {
            return Err(map_graphql_error(&parsed.errors));
        }
        Ok(parsed
            .data
            .and_then(|d| d.repository)
            .and_then(|r| r.ref_)
            .and_then(|r| r.target)
            .map(|t| t.oid))
    }

    async fn load_manifest_snapshot(&self) -> StorageResult<ScannedSubtree> {
        // Read the manifest through `raw.githubusercontent.com`. The CDN
        // edge cache (Cache-Control: max-age=300) means a post-commit
        // reload may see a 5-minute-stale tree, but in exchange the read
        // is not subject to the api.github.com Contents API rate limit
        // (60/hr unauthenticated). Local commits already update the
        // in-memory GlobalFs synchronously, so the staleness window only
        // affects multi-tab/multi-user re-scans.
        let resp = gloo_net::http::Request::get(&self.manifest_url())
            .cache(web_sys::RequestCache::NoCache)
            .send()
            .await
            .map_err(|e| StorageError::Network {
                message: e.to_string(),
            })?;
        // A missing manifest is the canonical signal of a fresh / empty
        // external mount. The bootstrap root is stricter because Home's
        // root mount status needs to distinguish a failed root manifest
        // from a genuinely empty external mount.
        if resp.status() == 404 {
            return if self.allow_missing_manifest {
                Ok(ScannedSubtree::default())
            } else {
                Err(StorageError::NotFound {
                    path: self.manifest_url(),
                })
            };
        }
        if !(200..300).contains(&resp.status()) {
            return Err(map_http_status(resp.status(), None));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| StorageError::RemoteRejected {
                message: e.to_string(),
            })?;
        parse_manifest_snapshot(&body).map_err(Into::into)
    }

    async fn load_manifest_snapshot_at_head(
        &self,
        token: &str,
        head_oid: &str,
    ) -> StorageResult<ScannedSubtree> {
        let (owner, name) =
            self.repo_with_owner
                .split_once('/')
                .ok_or_else(|| StorageError::InvalidRequest {
                    message: "invalid repo_with_owner".into(),
                })?;
        let manifest_path =
            prefixed_repo_path(&self.content_prefix, "manifest.json").map_err(|source| {
                StorageError::InvalidRequest {
                    message: source.to_string(),
                }
            })?;
        let body = ManifestObjectQueryRequest {
            query: MANIFEST_OBJECT_QUERY,
            variables: ManifestObjectQueryVariables {
                owner,
                name,
                manifest_expression: format!("{head_oid}:{manifest_path}"),
            },
        };
        let body_json = serde_json::to_string(&body).map_err(|e| StorageError::InvalidRequest {
            message: e.to_string(),
        })?;
        let resp = gloo_net::http::Request::post(GRAPHQL_ENDPOINT)
            .header("Authorization", &format!("bearer {}", token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "websh/0.1")
            .body(body_json)
            .map_err(|e| StorageError::InvalidRequest {
                message: e.to_string(),
            })?
            .send()
            .await
            .map_err(|e| StorageError::Network {
                message: e.to_string(),
            })?;
        let status = resp.status();
        if !(200..300).contains(&status) {
            return Err(map_http_status(status, retry_after_header(&resp)));
        }
        let parsed: ManifestObjectQueryResponse =
            resp.json().await.map_err(|e| StorageError::Network {
                message: e.to_string(),
            })?;
        if !parsed.errors.is_empty() {
            return Err(map_graphql_error(&parsed.errors));
        }

        let Some(repository) = parsed.data.and_then(|data| data.repository) else {
            return Err(StorageError::NotFound {
                path: self.repo_with_owner.clone(),
            });
        };
        let Some(object) = repository.object else {
            return if self.allow_missing_manifest {
                Ok(ScannedSubtree::default())
            } else {
                Err(StorageError::NotFound {
                    path: manifest_path,
                })
            };
        };
        match object {
            ManifestObject::Blob { text } => {
                parse_manifest_snapshot(&text.unwrap_or_default()).map_err(Into::into)
            }
            ManifestObject::Other => Err(StorageError::RemoteRejected {
                message: "manifest object is not a Blob".to_string(),
            }),
        }
    }
}

fn validate_repo_with_owner(value: &str) -> Result<(), GitHubBackendConfigError> {
    let Some((owner, name)) = value.split_once('/') else {
        return Err(GitHubBackendConfigError::InvalidRepo {
            value: value.to_string(),
        });
    };
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(GitHubBackendConfigError::InvalidRepo {
            value: value.to_string(),
        });
    }
    Ok(())
}

#[derive(Serialize)]
struct GraphQLRequest<'a> {
    query: &'static str,
    variables: GraphQLVariables<'a>,
}

#[derive(Serialize)]
struct GraphQLVariables<'a> {
    input: &'a CreateCommitInput,
}

#[derive(Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
    // GitHub omits `errors` on successful GraphQL responses.
    #[serde(default)]
    errors: Vec<GraphQLErrorItem>,
}

#[derive(Deserialize)]
struct GraphQLData {
    #[serde(rename = "createCommitOnBranch")]
    create_commit_on_branch: Option<CreateCommitResult>,
}

#[derive(Deserialize)]
struct CreateCommitResult {
    commit: CommitOid,
}

#[derive(Deserialize)]
struct CommitOid {
    oid: String,
}

#[derive(Deserialize)]
struct GraphQLErrorItem {
    message: String,
}

const MUTATION: &str = "\
mutation ($input: CreateCommitOnBranchInput!) {
  createCommitOnBranch(input: $input) {
    commit { oid }
  }
}
";

const HEAD_QUERY: &str = "\
query ($owner: String!, $name: String!, $qualifiedName: String!) {
  repository(owner: $owner, name: $name) {
    ref(qualifiedName: $qualifiedName) {
      target { oid }
    }
  }
}
";

const MANIFEST_OBJECT_QUERY: &str = "\
query ($owner: String!, $name: String!, $manifestExpression: String!) {
  repository(owner: $owner, name: $name) {
    object(expression: $manifestExpression) {
      __typename
      ... on Blob { text }
    }
  }
}
";

#[derive(Serialize)]
struct HeadQueryVariables<'a> {
    owner: &'a str,
    name: &'a str,
    #[serde(rename = "qualifiedName")]
    qualified_name: String,
}

#[derive(Serialize)]
struct HeadQueryRequest<'a> {
    query: &'static str,
    variables: HeadQueryVariables<'a>,
}

#[derive(Deserialize)]
struct HeadQueryResponse {
    data: Option<HeadQueryData>,
    // GitHub omits `errors` on successful GraphQL responses.
    #[serde(default)]
    errors: Vec<GraphQLErrorItem>,
}

#[derive(Serialize)]
struct ManifestObjectQueryVariables<'a> {
    owner: &'a str,
    name: &'a str,
    #[serde(rename = "manifestExpression")]
    manifest_expression: String,
}

#[derive(Serialize)]
struct ManifestObjectQueryRequest<'a> {
    query: &'static str,
    variables: ManifestObjectQueryVariables<'a>,
}

#[derive(Deserialize)]
struct ManifestObjectQueryResponse {
    data: Option<ManifestObjectQueryData>,
    // GitHub omits `errors` on successful GraphQL responses.
    #[serde(default)]
    errors: Vec<GraphQLErrorItem>,
}

#[derive(Deserialize)]
struct ManifestObjectQueryData {
    repository: Option<ManifestObjectRepository>,
}

#[derive(Deserialize)]
struct ManifestObjectRepository {
    object: Option<ManifestObject>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum ManifestObject {
    Blob {
        text: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct HeadQueryData {
    repository: Option<HeadQueryRepository>,
}

#[derive(Deserialize)]
struct HeadQueryRepository {
    #[serde(rename = "ref")]
    ref_: Option<HeadQueryRef>,
}

#[derive(Deserialize)]
struct HeadQueryRef {
    target: Option<HeadQueryTarget>,
}

#[derive(Deserialize)]
struct HeadQueryTarget {
    oid: String,
}

const GRAPHQL_ENDPOINT: &str = "https://api.github.com/graphql";

fn map_graphql_error(errors: &[GraphQLErrorItem]) -> StorageError {
    for e in errors {
        let msg = e.message.to_lowercase();
        if msg.contains("expected") && msg.contains("head") {
            return StorageError::Conflict {
                remote_head: extract_sha(&e.message),
            };
        }
        if msg.contains("not authorized") || msg.contains("must have push access") {
            return StorageError::AuthFailed;
        }
        if msg.contains("could not resolve") || msg.contains("not found") {
            return StorageError::NotFound {
                path: e.message.clone(),
            };
        }
    }
    StorageError::RemoteRejected {
        message: errors
            .first()
            .map(|e| e.message.clone())
            .unwrap_or_else(|| "unknown error".into()),
    }
}

fn map_http_status(status: u16, retry_after: Option<u64>) -> StorageError {
    match status {
        401 | 403 => StorageError::AuthFailed,
        404 => StorageError::NotFound {
            path: String::new(),
        },
        409 => StorageError::Conflict { remote_head: None },
        422 => StorageError::RemoteRejected {
            message: String::new(),
        },
        429 => StorageError::RateLimited { retry_after },
        500..=599 => StorageError::Server { status },
        _ => StorageError::Server { status },
    }
}

fn retry_after_header(resp: &gloo_net::http::Response) -> Option<u64> {
    resp.headers()
        .get("Retry-After")
        .and_then(|value| value.parse::<u64>().ok())
}

fn extract_sha(msg: &str) -> Option<String> {
    msg.split_whitespace()
        .find(|w| w.len() == 40 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(String::from)
}

impl StorageBackend for GitHubBackend {
    fn backend_type(&self) -> &'static str {
        "github"
    }

    fn commit_base(
        &self,
        expected_head: Option<String>,
        auth_token: Option<String>,
    ) -> LocalBoxFuture<'_, StorageResult<CommitBase>> {
        Box::pin(async move {
            let token = auth_token.as_deref().ok_or(StorageError::MissingToken)?;
            let expected_head = match expected_head {
                Some(head) => Some(head),
                None => self.fetch_branch_head_oid(token).await?,
            };
            let snapshot = match expected_head.as_deref() {
                Some(head) => self.load_manifest_snapshot_at_head(token, head).await?,
                None => ScannedSubtree::default(),
            };
            Ok(CommitBase {
                snapshot,
                expected_head,
            })
        })
    }

    fn scan(&self) -> LocalBoxFuture<'_, StorageResult<ScannedSubtree>> {
        Box::pin(async move { self.load_manifest_snapshot().await })
    }

    fn read_text<'a>(&'a self, rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<String>> {
        Box::pin(async move {
            let url =
                self.content_url(rel_path)
                    .map_err(|source| StorageError::InvalidRequest {
                        message: source.to_string(),
                    })?;
            let resp = gloo_net::http::Request::get(&url)
                .send()
                .await
                .map_err(|e| StorageError::Network {
                    message: e.to_string(),
                })?;
            if !(200..300).contains(&resp.status()) {
                return Err(map_http_status(resp.status(), None));
            }
            resp.text().await.map_err(|e| StorageError::RemoteRejected {
                message: e.to_string(),
            })
        })
    }

    fn read_bytes<'a>(&'a self, rel_path: &'a str) -> LocalBoxFuture<'a, StorageResult<Vec<u8>>> {
        Box::pin(async move {
            let url =
                self.content_url(rel_path)
                    .map_err(|source| StorageError::InvalidRequest {
                        message: source.to_string(),
                    })?;
            let resp = gloo_net::http::Request::get(&url)
                .send()
                .await
                .map_err(|e| StorageError::Network {
                    message: e.to_string(),
                })?;
            if !(200..300).contains(&resp.status()) {
                return Err(map_http_status(resp.status(), None));
            }
            resp.binary()
                .await
                .map_err(|e| StorageError::RemoteRejected {
                    message: e.to_string(),
                })
        })
    }

    fn public_read_url(&self, rel_path: &str) -> StorageResult<Option<String>> {
        self.content_url(rel_path)
            .map(Some)
            .map_err(|source| StorageError::InvalidRequest {
                message: source.to_string(),
            })
    }

    fn commit<'a>(
        &'a self,
        request: &'a CommitRequest,
    ) -> LocalBoxFuture<'a, StorageResult<CommitOutcome>> {
        Box::pin(async move {
            let token = request
                .auth_token
                .as_deref()
                .ok_or(StorageError::MissingToken)?;
            let manifest_body = serialize_manifest_snapshot(&request.merged_snapshot)?;
            let manifest_repo_path = prefixed_repo_path(&self.content_prefix, "manifest.json")
                .map_err(|source| StorageError::InvalidRequest {
                    message: source.to_string(),
                })?;
            let file_changes = build_file_changes(
                &request.delta,
                &self.mount_root,
                &self.content_prefix,
                Some((manifest_repo_path.as_str(), &manifest_body)),
            )
            .map_err(|source: GraphQLOperationBuildError| {
                StorageError::InvalidRequest {
                    message: source.to_string(),
                }
            })?;

            // GitHub's `createCommitOnBranch` mutation requires
            // `expectedHeadOid`. On the first UI-driven commit to a mount
            // there is nothing in IndexedDB to seed `remote_heads`, so the
            // runtime passes `None` here. Fetch the current branch HEAD
            // before committing to avoid the chicken-and-egg "remote
            // changed (now ). run 'sync refresh'" failure.
            let expected_head_oid = match request.expected_head.clone() {
                Some(head) => Some(head),
                None => self.fetch_branch_head_oid(token).await?,
            };

            let input = CreateCommitInput {
                branch: BranchRef {
                    repo_with_owner: self.repo_with_owner.clone(),
                    branch_name: self.branch.clone(),
                },
                message: CommitMessage {
                    headline: request.message.clone(),
                },
                expected_head_oid,
                file_changes,
            };

            let body = GraphQLRequest {
                query: MUTATION,
                variables: GraphQLVariables { input: &input },
            };
            let body_json =
                serde_json::to_string(&body).map_err(|e| StorageError::InvalidRequest {
                    message: e.to_string(),
                })?;

            let resp = gloo_net::http::Request::post(GRAPHQL_ENDPOINT)
                .header("Authorization", &format!("bearer {}", token))
                .header("Content-Type", "application/json")
                .header("User-Agent", "websh/0.1")
                .body(body_json)
                .map_err(|e| StorageError::InvalidRequest {
                    message: e.to_string(),
                })?
                .send()
                .await
                .map_err(|e| StorageError::Network {
                    message: e.to_string(),
                })?;

            let status = resp.status();
            if !(200..300).contains(&status) {
                return Err(map_http_status(status, retry_after_header(&resp)));
            }

            let gql: GraphQLResponse = resp.json().await.map_err(|e| StorageError::Network {
                message: e.to_string(),
            })?;

            if !gql.errors.is_empty() {
                return Err(map_graphql_error(&gql.errors));
            }

            let new_head = gql
                .data
                .and_then(|d| d.create_commit_on_branch)
                .map(|c| c.commit.oid)
                .ok_or_else(|| StorageError::RemoteRejected {
                    message: "empty data".into(),
                })?;

            Ok(CommitOutcome {
                new_head,
                committed_paths: request.cleanup_paths.clone(),
            })
        })
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn http_401_maps_auth_failed() {
        assert_eq!(map_http_status(401, None), StorageError::AuthFailed);
        assert_eq!(map_http_status(403, None), StorageError::AuthFailed);
    }

    #[wasm_bindgen_test]
    fn http_429_preserves_retry_after() {
        assert_eq!(
            map_http_status(429, Some(30)),
            StorageError::RateLimited {
                retry_after: Some(30)
            }
        );
    }

    #[wasm_bindgen_test]
    fn graphql_error_conflict_detected() {
        let e = vec![GraphQLErrorItem {
            message: "expected head oid abc123def456abc123def456abc123def4567890 was not current"
                .into(),
        }];
        let mapped = map_graphql_error(&e);
        assert!(matches!(mapped, StorageError::Conflict { .. }));
    }

    #[wasm_bindgen_test]
    fn graphql_error_auth_detected() {
        let e = vec![GraphQLErrorItem {
            message: "must have push access".into(),
        }];
        assert_eq!(map_graphql_error(&e), StorageError::AuthFailed);
    }

    #[wasm_bindgen_test]
    fn content_url_uses_manifest_directory_as_base() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();

        assert_eq!(
            backend.content_url(".websh/site.json").unwrap(),
            "https://raw.githubusercontent.com/owner/repo/main/~/.websh/site.json"
        );
    }

    #[wasm_bindgen_test]
    fn content_url_encodes_path_segments() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();

        assert_eq!(
            backend.content_url("docs/file #1.md").unwrap(),
            "https://raw.githubusercontent.com/owner/repo/main/~/docs/file%20%231.md"
        );
    }

    #[wasm_bindgen_test]
    fn content_url_rejects_traversal_segments() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();

        assert!(backend.content_url("../secret.md").is_err());
    }

    #[wasm_bindgen_test]
    fn public_read_url_reuses_encoded_content_url() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();

        assert_eq!(
            backend.public_read_url("docs/file #1.pdf").unwrap(),
            Some("https://raw.githubusercontent.com/owner/repo/main/~/docs/file%20%231.pdf".into())
        );
    }

    #[wasm_bindgen_test]
    fn public_read_url_rejects_traversal_segments() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();

        assert!(backend.public_read_url("../secret.md").is_err());
    }

    #[wasm_bindgen_test]
    fn constructor_rejects_traversal_content_prefix() {
        let err = match GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "content/../other",
            "https://raw.githubusercontent.com",
        ) {
            Ok(_) => panic!("constructor should reject traversal content prefix"),
            Err(err) => err,
        };
        assert!(matches!(
            err,
            GitHubBackendConfigError::InvalidContentPrefix {
                source: RepoPathError::Traversal { path },
            } if path == "content/../other"
        ));
    }

    #[wasm_bindgen_test]
    async fn commit_requires_token_from_request() {
        let backend = GitHubBackend::new(
            "owner/repo",
            "main",
            VirtualPath::root(),
            "~",
            "https://raw.githubusercontent.com",
        )
        .unwrap();
        let request = CommitRequest {
            delta: websh_core::ports::CommitDelta::default(),
            cleanup_paths: vec![],
            merged_snapshot: ScannedSubtree::default(),
            message: "msg".to_string(),
            expected_head: None,
            auth_token: None,
        };

        let err = backend.commit(&request).await.unwrap_err();
        assert_eq!(err, StorageError::MissingToken);
    }
}
