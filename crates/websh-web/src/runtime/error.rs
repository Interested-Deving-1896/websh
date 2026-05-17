use websh_core::domain::VirtualPath;
use websh_core::filesystem::MountError;
use websh_core::ports::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeLoadError {
    #[error("mount {label}: {source}")]
    BootstrapMount {
        label: String,
        #[source]
        source: StorageError,
    },
    #[error("assemble global filesystem: {source}")]
    AssembleGlobalFs {
        #[source]
        source: MountError,
    },
    #[error("read {path}: {source}")]
    Read {
        path: VirtualPath,
        #[source]
        source: StorageError,
    },
    #[error("{path} outside {mount_root}")]
    PathOutsideMount {
        path: VirtualPath,
        mount_root: VirtualPath,
    },
    #[error("parse {path}: {source}")]
    ParseJson {
        path: VirtualPath,
        #[source]
        source: serde_json::Error,
    },
}
