use std::sync::Arc;

use crate::app::AppContext;
use crate::platform::redirect::{UrlValidation, validate_redirect_url};
use crate::platform::{BrowserAssetUrl, object_url_for_bytes};
use crate::render::{RenderedMarkdown, render_markdown, rendered_from_html, sanitize_html};
use websh_core::domain::{FileType, VirtualPath};
use websh_core::support::asset::data_url_for_bytes;
use websh_core::support::media_type_for_path;

use super::{ReaderIntent, ReaderLoadError};

#[derive(Clone)]
pub(super) enum RendererContent {
    Markdown(RenderedMarkdown),
    Html(RenderedMarkdown),
    Text(String),
    Pdf { url: BrowserAssetUrl },
    Image { url: String },
    Redirecting,
}

#[derive(Clone)]
pub(super) struct ReaderDocument {
    pub(super) content: RendererContent,
    pub(super) raw_source: Option<String>,
}

pub(super) async fn load_reader_document(
    ctx: AppContext,
    intent: ReaderIntent,
) -> Result<ReaderDocument, ReaderLoadError> {
    let path = content_path_for_intent(&intent);
    let content = match intent {
        ReaderIntent::Markdown { .. } => {
            let markdown = ctx
                .read_text(&path)
                .await
                .map_err(|source| ReaderLoadError::Read {
                    path: path.clone(),
                    source,
                })?;
            return Ok(ReaderDocument {
                content: RendererContent::Markdown(render_markdown(&markdown)),
                raw_source: Some(markdown),
            });
        }
        ReaderIntent::Html { .. } => ctx
            .read_text(&path)
            .await
            .map(|html| RendererContent::Html(rendered_from_html(sanitize_html(&html))))
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            })?,
        ReaderIntent::Plain { .. } => ctx
            .read_text(&path)
            .await
            .map(RendererContent::Text)
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            })?,
        ReaderIntent::Asset { media_type, .. } => load_asset(ctx, &path, media_type).await?,
        ReaderIntent::Redirect { .. } => load_redirect(ctx, &path).await?,
        ReaderIntent::BundleVariant { .. } => load_bundle_variant(ctx, &path).await?,
    };

    Ok(ReaderDocument {
        content,
        raw_source: None,
    })
}

fn content_path_for_intent(intent: &ReaderIntent) -> VirtualPath {
    match intent {
        ReaderIntent::Markdown { node_path }
        | ReaderIntent::Html { node_path }
        | ReaderIntent::Plain { node_path }
        | ReaderIntent::Asset { node_path, .. }
        | ReaderIntent::Redirect { node_path } => node_path.clone(),
        ReaderIntent::BundleVariant { variant_path, .. } => variant_path.clone(),
    }
}

async fn load_bundle_variant(
    ctx: AppContext,
    path: &VirtualPath,
) -> Result<RendererContent, ReaderLoadError> {
    match FileType::from_path(path.as_str()) {
        FileType::Markdown => {
            let markdown = ctx
                .read_text(path)
                .await
                .map_err(|source| ReaderLoadError::Read {
                    path: path.clone(),
                    source,
                })?;
            Ok(RendererContent::Markdown(render_markdown(&markdown)))
        }
        FileType::Html => ctx
            .read_text(path)
            .await
            .map(|html| RendererContent::Html(rendered_from_html(sanitize_html(&html))))
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            }),
        FileType::Pdf | FileType::Image => {
            load_asset(ctx, path, media_type_for_path(path.as_str()).to_string()).await
        }
        FileType::Link => load_redirect(ctx, path).await,
        FileType::Unknown => ctx
            .read_text(path)
            .await
            .map(RendererContent::Text)
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            }),
    }
}

async fn load_asset(
    ctx: AppContext,
    path: &VirtualPath,
    media_type: String,
) -> Result<RendererContent, ReaderLoadError> {
    let public_url = ctx
        .public_read_url(path)
        .map_err(|source| ReaderLoadError::Read {
            path: path.clone(),
            source,
        })?;

    if media_type == "application/pdf" {
        if let Some(url) = public_url
            .as_deref()
            .filter(|url| can_embed_pdf_url(url))
            .map(|url| BrowserAssetUrl::public(url.to_owned()))
        {
            return Ok(RendererContent::Pdf { url });
        }
        let bytes = ctx
            .read_bytes(path)
            .await
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            })?;
        let url =
            object_url_for_bytes(&bytes, &media_type).map_err(|source| ReaderLoadError::Asset {
                path: path.clone(),
                source,
            })?;
        Ok(RendererContent::Pdf { url })
    } else {
        if let Some(url) = public_url.filter(|url| can_render_image_url(url)) {
            return Ok(RendererContent::Image { url });
        }
        let bytes = ctx
            .read_bytes(path)
            .await
            .map_err(|source| ReaderLoadError::Read {
                path: path.clone(),
                source,
            })?;
        let url = data_url_for_bytes(&bytes, &media_type);
        Ok(RendererContent::Image { url })
    }
}

fn can_embed_pdf_url(url: &str) -> bool {
    is_relative_public_url(url) || is_githubusercontent_url(url)
}

fn can_render_image_url(url: &str) -> bool {
    is_relative_public_url(url) || url.trim_start().starts_with("https://")
}

fn is_relative_public_url(url: &str) -> bool {
    let trimmed = url.trim_start();
    !trimmed.starts_with("//") && !has_url_scheme(trimmed)
}

fn has_url_scheme(url: &str) -> bool {
    let head = url.split(['/', '?', '#']).next().unwrap_or_default();
    head.contains(':')
}

fn is_githubusercontent_url(url: &str) -> bool {
    let Some(rest) = url.trim_start().strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    host == "raw.githubusercontent.com" || host.ends_with(".githubusercontent.com")
}

async fn load_redirect(
    ctx: AppContext,
    path: &VirtualPath,
) -> Result<RendererContent, ReaderLoadError> {
    let target = ctx
        .read_text(path)
        .await
        .map_err(|source| ReaderLoadError::Read {
            path: path.clone(),
            source,
        })?;
    match validate_redirect_url(target.trim()) {
        UrlValidation::Valid(safe_url) => {
            if let Some(window) = web_sys::window()
                && window.location().set_href(&safe_url).is_err()
            {
                return Err(ReaderLoadError::RedirectFailed);
            }
            Ok(RendererContent::Redirecting)
        }
        UrlValidation::Invalid(source) => Err(ReaderLoadError::RedirectBlocked {
            source: Arc::new(source),
        }),
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn pdf_direct_url_allows_relative_and_githubusercontent_sources() {
        assert!(can_embed_pdf_url("./content/docs/file.pdf"));
        assert!(can_embed_pdf_url("/content/docs/file.pdf"));
        assert!(can_embed_pdf_url(
            "https://raw.githubusercontent.com/owner/repo/main/content/file.pdf"
        ));
    }

    #[wasm_bindgen_test]
    fn pdf_direct_url_rejects_non_csp_sources() {
        assert!(!can_embed_pdf_url(
            "https://gateway.pinata.cloud/ipfs/cid/file.pdf"
        ));
        assert!(!can_embed_pdf_url(
            "//gateway.pinata.cloud/ipfs/cid/file.pdf"
        ));
        assert!(!can_embed_pdf_url("javascript:alert(1)"));
    }

    #[wasm_bindgen_test]
    fn image_direct_url_allows_https_sources() {
        assert!(can_render_image_url("./content/images/file.png"));
        assert!(can_render_image_url(
            "https://gateway.pinata.cloud/ipfs/cid/file.png"
        ));
        assert!(!can_render_image_url("http://example.com/file.png"));
    }
}
