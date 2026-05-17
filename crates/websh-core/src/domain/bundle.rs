use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub type BundleValidationResult<T = ()> = Result<T, BundleValidationError>;

/// Top-level metadata for a renderable directory bundle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleMetadata {
    pub default_variant: String,
    pub variants: Vec<BundleVariant>,
}

/// One declared rendition inside a bundle directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleVariant {
    pub id: String,
    pub path: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BundleValidationError {
    #[error("bundle {bundle_path} has an empty default_variant")]
    EmptyDefaultVariant { bundle_path: String },
    #[error("bundle {bundle_path} declares duplicate variant id `{variant_id}`")]
    DuplicateVariantId {
        bundle_path: String,
        variant_id: String,
    },
    #[error("bundle {bundle_path} declares duplicate variant path `{variant_path}`")]
    DuplicateVariantPath {
        bundle_path: String,
        variant_path: String,
    },
    #[error("bundle {bundle_path} default_variant `{default_variant}` is not declared")]
    DefaultVariantMissing {
        bundle_path: String,
        default_variant: String,
    },
    #[error(
        "bundle {bundle_path} variant id `{variant_id}` must use only ASCII letters, numbers, `_`, or `-`"
    )]
    InvalidVariantId {
        bundle_path: String,
        variant_id: String,
    },
    #[error("bundle {bundle_path} variant `{variant_id}` has an empty label")]
    EmptyVariantLabel {
        bundle_path: String,
        variant_id: String,
    },
    #[error("bundle {bundle_path} variant `{variant_id}` has an empty path")]
    EmptyVariantPath {
        bundle_path: String,
        variant_id: String,
    },
    #[error("bundle {bundle_path} variant `{variant_id}` path `{path}` is not portable")]
    NonPortableVariantPath {
        bundle_path: String,
        variant_id: String,
        path: String,
    },
    #[error(
        "bundle {bundle_path} variant `{variant_id}` path `{path}` must stay inside the bundle"
    )]
    VariantPathEscapesBundle {
        bundle_path: String,
        variant_id: String,
        path: String,
    },
    #[error("bundle {bundle_path} variant `{variant_id}` points to sidecar `{path}`")]
    VariantPointsToSidecar {
        bundle_path: String,
        variant_id: String,
        path: String,
    },
    #[error("bundle {bundle_path} route `{route}` collides with file `{file_path}`")]
    RootRouteCollision {
        bundle_path: String,
        route: String,
        file_path: String,
    },
    #[error(
        "bundle {bundle_path} variant `{variant_id}` route `{route}` collides with file `{file_path}`"
    )]
    VariantRouteCollision {
        bundle_path: String,
        variant_id: String,
        route: String,
        file_path: String,
    },
}

pub fn validate_bundle_metadata(
    bundle_path: &str,
    bundle: &BundleMetadata,
) -> BundleValidationResult {
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let bundle_path = display_bundle_path(bundle_path).to_string();

    if bundle.default_variant.trim().is_empty() {
        return Err(BundleValidationError::EmptyDefaultVariant { bundle_path });
    }

    for variant in &bundle.variants {
        validate_bundle_variant(&bundle_path, variant)?;
        if !ids.insert(variant.id.clone()) {
            return Err(BundleValidationError::DuplicateVariantId {
                bundle_path,
                variant_id: variant.id.clone(),
            });
        }
        if !paths.insert(variant.path.clone()) {
            return Err(BundleValidationError::DuplicateVariantPath {
                bundle_path,
                variant_path: variant.path.clone(),
            });
        }
    }

    if !ids.contains(&bundle.default_variant) {
        return Err(BundleValidationError::DefaultVariantMissing {
            bundle_path,
            default_variant: bundle.default_variant.clone(),
        });
    }

    Ok(())
}

pub fn validate_bundle_metadata_with_targets<E>(
    bundle_path: &str,
    bundle: &BundleMetadata,
    mut validate_variant_target: impl FnMut(&BundleVariant) -> Result<(), E>,
) -> Result<(), E>
where
    E: From<BundleValidationError>,
{
    validate_bundle_metadata(bundle_path, bundle).map_err(E::from)?;
    for variant in &bundle.variants {
        validate_variant_target(variant)?;
    }
    Ok(())
}

pub fn validate_bundle_route_collisions<'a>(
    bundle_path: &str,
    bundle: &BundleMetadata,
    file_paths: impl IntoIterator<Item = &'a str>,
    route_for_path: impl Fn(&str) -> String,
) -> BundleValidationResult {
    let display_path = display_bundle_path(bundle_path).to_string();
    let bundle_route = route_for_path(bundle_path);
    let declared_variant_paths = bundle
        .variants
        .iter()
        .map(|variant| join_bundle_path(bundle_path, &variant.path))
        .collect::<BTreeSet<_>>();

    for file_path in file_paths {
        if declared_variant_paths.contains(file_path) {
            continue;
        }

        let file_route = route_for_path(file_path);
        if file_route == bundle_route {
            return Err(BundleValidationError::RootRouteCollision {
                bundle_path: display_path,
                route: bundle_route,
                file_path: file_path.to_string(),
            });
        }
        for variant in &bundle.variants {
            let variant_route = join_route(&bundle_route, &variant.id);
            if file_route == variant_route {
                return Err(BundleValidationError::VariantRouteCollision {
                    bundle_path: display_path,
                    variant_id: variant.id.clone(),
                    route: variant_route,
                    file_path: file_path.to_string(),
                });
            }
        }
    }

    Ok(())
}

pub fn validate_bundle_variant(
    bundle_path: &str,
    variant: &BundleVariant,
) -> BundleValidationResult {
    validate_bundle_variant_id(bundle_path, &variant.id)?;
    if variant.label.trim().is_empty() {
        return Err(BundleValidationError::EmptyVariantLabel {
            bundle_path: display_bundle_path(bundle_path).to_string(),
            variant_id: variant.id.clone(),
        });
    }
    validate_relative_bundle_path(bundle_path, &variant.id, &variant.path)
}

pub fn validate_bundle_variant_id(bundle_path: &str, variant_id: &str) -> BundleValidationResult {
    if variant_id.is_empty()
        || !variant_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(BundleValidationError::InvalidVariantId {
            bundle_path: display_bundle_path(bundle_path).to_string(),
            variant_id: variant_id.to_string(),
        });
    }
    Ok(())
}

pub fn validate_relative_bundle_path(
    bundle_path: &str,
    variant_id: &str,
    rel_path: &str,
) -> BundleValidationResult {
    let bundle_path = display_bundle_path(bundle_path).to_string();
    let variant_id = variant_id.to_string();
    if rel_path.trim().is_empty() {
        return Err(BundleValidationError::EmptyVariantPath {
            bundle_path,
            variant_id,
        });
    }
    if rel_path.starts_with('/')
        || rel_path.contains('\\')
        || rel_path.chars().any(char::is_control)
    {
        return Err(BundleValidationError::NonPortableVariantPath {
            bundle_path,
            variant_id,
            path: rel_path.to_string(),
        });
    }
    for segment in rel_path.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") {
            return Err(BundleValidationError::VariantPathEscapesBundle {
                bundle_path,
                variant_id,
                path: rel_path.to_string(),
            });
        }
    }
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    if name == "_index.dir.json" || name.ends_with(".meta.json") {
        return Err(BundleValidationError::VariantPointsToSidecar {
            bundle_path,
            variant_id,
            path: rel_path.to_string(),
        });
    }
    Ok(())
}

fn display_bundle_path(path: &str) -> &str {
    if path.is_empty() { "/" } else { path }
}

fn join_bundle_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn join_route(base: &str, child: &str) -> String {
    if base == "/" {
        format!("/{child}")
    } else {
        format!("{base}/{child}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(id: &str, path: &str) -> BundleVariant {
        BundleVariant {
            id: id.to_string(),
            path: path.to_string(),
            label: id.to_string(),
            locale: None,
            media_type: None,
        }
    }

    #[test]
    fn rejects_variant_ids_with_dots() {
        let bundle = BundleMetadata {
            default_variant: "ko.md".to_string(),
            variants: vec![variant("ko.md", "ko.md")],
        };

        let err = validate_bundle_metadata("writing/foo", &bundle).unwrap_err();
        assert!(matches!(
            err,
            BundleValidationError::InvalidVariantId { variant_id, .. }
                if variant_id == "ko.md"
        ));
    }

    #[test]
    fn accepts_slug_like_non_language_variant_ids() {
        let bundle = BundleMetadata {
            default_variant: "print_pdf".to_string(),
            variants: vec![variant("print_pdf", "print.pdf")],
        };

        validate_bundle_metadata("writing/foo", &bundle).unwrap();
    }

    #[test]
    fn rejects_sidecar_variant_paths() {
        let bundle = BundleMetadata {
            default_variant: "en".to_string(),
            variants: vec![variant("en", "en.meta.json")],
        };

        let err = validate_bundle_metadata("writing/foo", &bundle).unwrap_err();
        assert!(matches!(
            err,
            BundleValidationError::VariantPointsToSidecar { path, .. }
                if path == "en.meta.json"
        ));
    }

    #[test]
    fn rejects_root_route_collisions() {
        let bundle = BundleMetadata {
            default_variant: "en".to_string(),
            variants: vec![variant("en", "en.md")],
        };

        let err = validate_bundle_route_collisions(
            "writing/foo",
            &bundle,
            ["writing/foo/en.md", "writing/foo.md"],
            test_route_for_path,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            BundleValidationError::RootRouteCollision { file_path, .. }
                if file_path == "writing/foo.md"
        ));
    }

    #[test]
    fn rejects_variant_route_collisions_except_declared_variant_path() {
        let bundle = BundleMetadata {
            default_variant: "print".to_string(),
            variants: vec![variant("print", "print.pdf")],
        };

        let err = validate_bundle_route_collisions(
            "writing/foo",
            &bundle,
            ["writing/foo/print.pdf", "writing/foo/print.md"],
            test_route_for_path,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            BundleValidationError::VariantRouteCollision {
                variant_id,
                file_path,
                ..
            } if variant_id == "print" && file_path == "writing/foo/print.md"
        ));
    }

    fn test_route_for_path(path: &str) -> String {
        let normalized = path.trim_matches('/');
        if normalized.is_empty() {
            return "/".to_string();
        }
        for suffix in [".page.html", ".page.md", ".html", ".md", ".link", ".app"] {
            if let Some(route) = normalized.strip_suffix(suffix) {
                return format!("/{route}");
            }
        }
        format!("/{normalized}")
    }
}
