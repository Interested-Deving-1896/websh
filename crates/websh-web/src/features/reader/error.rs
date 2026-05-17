use std::sync::Arc;

use websh_core::domain::VirtualPath;
use websh_core::filesystem::ContentReadError;

use crate::platform::BrowserAssetError;
use crate::platform::redirect::UrlValidationError;

#[derive(Clone, Debug, thiserror::Error)]
pub(super) enum ReaderLoadError {
    #[error("read {path}: {source}")]
    Read {
        path: VirtualPath,
        #[source]
        source: ContentReadError,
    },
    #[error("load asset {path}: {source}")]
    Asset {
        path: VirtualPath,
        #[source]
        source: BrowserAssetError,
    },
    #[error("redirect blocked: {source}")]
    RedirectBlocked {
        #[source]
        source: Arc<UrlValidationError>,
    },
    #[error("redirect failed")]
    RedirectFailed,
}
