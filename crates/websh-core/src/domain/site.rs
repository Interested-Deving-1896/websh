//! Filesystem-first site metadata models.
//!
//! Mount declarations and the derived route index. The unified node-level
//! metadata model lives in [`super::metadata`].

use serde::{Deserialize, Serialize};

use super::metadata::{NodeKind, RendererKind};

/// Filesystem-declared mount definition loaded after bootstrap.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountDeclaration {
    pub backend: String,
    pub mount_at: String,
    pub repo: Option<String>,
    pub branch: Option<String>,
    pub root: Option<String>,
    pub gateway: Option<String>,
    pub name: Option<String>,
    pub writable: bool,
}

/// One route entry in the derived index.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteIndexEntry {
    pub route: String,
    pub node_path: String,
    pub kind: Option<NodeKind>,
    pub renderer: Option<RendererKind>,
}

/// Derived route/search index generated from the canonical tree plus sidecars.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedIndex {
    pub routes: Vec<RouteIndexEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_declaration_parses_expected_shape() {
        let decl: MountDeclaration = serde_json::from_str(
            r#"{
                "backend": "github",
                "mount_at": "/db",
                "repo": "0xwonj/db",
                "branch": "main",
                "writable": true
            }"#,
        )
        .unwrap();

        assert_eq!(decl.backend, "github");
        assert_eq!(decl.mount_at, "/db");
        assert_eq!(decl.repo.as_deref(), Some("0xwonj/db"));
        assert_eq!(decl.branch.as_deref(), Some("main"));
        assert!(decl.writable);
    }

    #[test]
    fn derived_index_parses_explicit_empty_routes() {
        let index: DerivedIndex = serde_json::from_str(r#"{"routes":[]}"#).unwrap();
        assert!(index.routes.is_empty());
    }

    #[test]
    fn derived_index_requires_routes_field() {
        let parsed = serde_json::from_str::<DerivedIndex>("{}");
        assert!(parsed.is_err());
    }
}
