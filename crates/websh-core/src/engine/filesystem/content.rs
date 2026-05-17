use std::collections::BTreeMap;

use thiserror::Error;

use crate::domain::VirtualPath;
use crate::ports::{StorageBackendRef, StorageError};

use super::GlobalFs;

pub type BackendRegistry = BTreeMap<VirtualPath, StorageBackendRef>;

/// Target-neutral content read errors for backend-backed file reads.
#[derive(Debug, Clone, Error)]
pub enum ContentReadError {
    #[error("no backend for {path}")]
    NoBackend { path: VirtualPath },
    #[error("path {path} is outside backend root {root}")]
    PathOutsideBackendRoot {
        path: VirtualPath,
        root: VirtualPath,
    },
    #[error(transparent)]
    Storage(#[from] StorageError),
}

pub async fn read_text(
    fs: &GlobalFs,
    backends: &BackendRegistry,
    path: &VirtualPath,
) -> Result<String, ContentReadError> {
    if let Some(text) = fs.read_pending_text(path) {
        return Ok(text);
    }

    let (root, backend) = backend_for_path(backends, path)
        .ok_or_else(|| ContentReadError::NoBackend { path: path.clone() })?;
    let rel_path = relative_backend_path(path, &root).ok_or_else(|| {
        ContentReadError::PathOutsideBackendRoot {
            path: path.clone(),
            root: root.clone(),
        }
    })?;

    backend.read_text(&rel_path).await.map_err(Into::into)
}

pub async fn read_bytes(
    fs: &GlobalFs,
    backends: &BackendRegistry,
    path: &VirtualPath,
) -> Result<Vec<u8>, ContentReadError> {
    if let Some(text) = fs.read_pending_text(path) {
        return Ok(text.into_bytes());
    }

    let (root, backend) = backend_for_path(backends, path)
        .ok_or_else(|| ContentReadError::NoBackend { path: path.clone() })?;
    let rel_path = relative_backend_path(path, &root).ok_or_else(|| {
        ContentReadError::PathOutsideBackendRoot {
            path: path.clone(),
            root: root.clone(),
        }
    })?;

    backend.read_bytes(&rel_path).await.map_err(Into::into)
}

pub fn public_read_url(
    fs: &GlobalFs,
    backends: &BackendRegistry,
    path: &VirtualPath,
) -> Result<Option<String>, ContentReadError> {
    if fs.read_pending_text(path).is_some() {
        return Ok(None);
    }

    let (root, backend) = backend_for_path(backends, path)
        .ok_or_else(|| ContentReadError::NoBackend { path: path.clone() })?;
    let rel_path = relative_backend_path(path, &root).ok_or_else(|| {
        ContentReadError::PathOutsideBackendRoot {
            path: path.clone(),
            root: root.clone(),
        }
    })?;

    backend.public_read_url(&rel_path).map_err(Into::into)
}

fn backend_for_path(
    backends: &BackendRegistry,
    path: &VirtualPath,
) -> Option<(VirtualPath, StorageBackendRef)> {
    backends
        .iter()
        .filter(|(root, _)| path.starts_with(root))
        .max_by_key(|(root, _)| root.as_str().len())
        .map(|(root, backend)| (root.clone(), backend.clone()))
}

fn relative_backend_path(path: &VirtualPath, root: &VirtualPath) -> Option<String> {
    let rel = path.strip_prefix(root)?;
    Some(rel.to_string())
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;
    use std::sync::Mutex;

    use crate::domain::{
        EntryExtensions, Fields, NodeKind, NodeMetadata, SCHEMA_VERSION, VirtualPath,
    };
    use crate::ports::StorageBackend;

    use super::*;

    struct StubBackend {
        reads: Mutex<Vec<String>>,
        public_url_reads: Mutex<Vec<String>>,
        text: String,
        public_url: Option<String>,
    }

    impl StorageBackend for StubBackend {
        fn backend_type(&self) -> &'static str {
            "stub"
        }

        fn scan(
            &self,
        ) -> crate::ports::LocalBoxFuture<
            '_,
            crate::ports::StorageResult<crate::ports::ScannedSubtree>,
        > {
            Box::pin(async { Ok(crate::ports::ScannedSubtree::default()) })
        }

        fn read_text<'a>(
            &'a self,
            rel_path: &'a str,
        ) -> crate::ports::LocalBoxFuture<'a, crate::ports::StorageResult<String>> {
            self.reads.lock().unwrap().push(rel_path.to_string());
            let text = self.text.clone();
            Box::pin(async move { Ok(text) })
        }

        fn read_bytes<'a>(
            &'a self,
            rel_path: &'a str,
        ) -> crate::ports::LocalBoxFuture<'a, crate::ports::StorageResult<Vec<u8>>> {
            self.reads.lock().unwrap().push(rel_path.to_string());
            let text = self.text.clone();
            Box::pin(async move { Ok(text.into_bytes()) })
        }

        fn public_read_url(&self, rel_path: &str) -> crate::ports::StorageResult<Option<String>> {
            self.public_url_reads
                .lock()
                .unwrap()
                .push(rel_path.to_string());
            Ok(self.public_url.clone())
        }

        fn commit<'a>(
            &'a self,
            _request: &'a crate::ports::CommitRequest,
        ) -> crate::ports::LocalBoxFuture<
            'a,
            crate::ports::StorageResult<crate::ports::CommitOutcome>,
        > {
            Box::pin(async {
                Err(crate::ports::StorageError::InvalidRequest {
                    message: "commit unused".to_string(),
                })
            })
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_content_wins_over_backend_reads() {
        let mut fs = GlobalFs::empty();
        let path = VirtualPath::from_absolute("/.websh/state/env/EDITOR").unwrap();
        fs.upsert_file(
            path.clone(),
            "vim".to_string(),
            NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Data,
                bundle: None,
                authored: Fields::default(),
                derived: Fields::default(),
            },
            EntryExtensions::default(),
        );

        let mut backends = BackendRegistry::new();
        backends.insert(
            VirtualPath::from_absolute("/.websh/state").unwrap(),
            Rc::new(StubBackend {
                reads: Mutex::new(Vec::new()),
                public_url_reads: Mutex::new(Vec::new()),
                text: "nano".to_string(),
                public_url: Some("/content/.websh/state/env/EDITOR".to_string()),
            }),
        );

        let text = read_text(&fs, &backends, &path).await.expect("text");
        assert_eq!(text, "vim");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn backend_reads_relative_path_under_mount_root() {
        let mut fs = GlobalFs::empty();
        fs.upsert_binary_placeholder(
            VirtualPath::from_absolute("/blog/post.md").unwrap(),
            NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Page,
                bundle: None,
                authored: Fields::default(),
                derived: Fields::default(),
            },
            EntryExtensions::default(),
        );

        let backend = Rc::new(StubBackend {
            reads: Mutex::new(Vec::new()),
            public_url_reads: Mutex::new(Vec::new()),
            text: "hello".to_string(),
            public_url: None,
        });
        let mut backends = BackendRegistry::new();
        backends.insert(VirtualPath::root(), backend.clone());

        let text = read_text(
            &fs,
            &backends,
            &VirtualPath::from_absolute("/blog/post.md").unwrap(),
        )
        .await
        .expect("text");

        assert_eq!(text, "hello");
        assert_eq!(backend.reads.lock().unwrap().as_slice(), ["blog/post.md"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn public_read_url_uses_relative_path_under_mount_root() {
        let fs = GlobalFs::empty();
        let backend = Rc::new(StubBackend {
            reads: Mutex::new(Vec::new()),
            public_url_reads: Mutex::new(Vec::new()),
            text: "unused".to_string(),
            public_url: Some("/content/blog/post.pdf".to_string()),
        });
        let mut backends = BackendRegistry::new();
        backends.insert(VirtualPath::root(), backend.clone());

        let url = public_read_url(
            &fs,
            &backends,
            &VirtualPath::from_absolute("/blog/post.pdf").unwrap(),
        )
        .expect("url");

        assert_eq!(url.as_deref(), Some("/content/blog/post.pdf"));
        assert_eq!(
            backend.public_url_reads.lock().unwrap().as_slice(),
            ["blog/post.pdf"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pending_content_disables_public_read_url() {
        let mut fs = GlobalFs::empty();
        let path = VirtualPath::from_absolute("/draft.md").unwrap();
        fs.upsert_file(
            path.clone(),
            "draft".to_string(),
            NodeMetadata {
                schema: SCHEMA_VERSION,
                kind: NodeKind::Page,
                bundle: None,
                authored: Fields::default(),
                derived: Fields::default(),
            },
            EntryExtensions::default(),
        );

        let backend = Rc::new(StubBackend {
            reads: Mutex::new(Vec::new()),
            public_url_reads: Mutex::new(Vec::new()),
            text: "remote".to_string(),
            public_url: Some("/content/draft.md".to_string()),
        });
        let mut backends = BackendRegistry::new();
        backends.insert(VirtualPath::root(), backend.clone());

        let url = public_read_url(&fs, &backends, &path).expect("url");

        assert_eq!(url, None);
        assert!(backend.public_url_reads.lock().unwrap().is_empty());
    }
}
