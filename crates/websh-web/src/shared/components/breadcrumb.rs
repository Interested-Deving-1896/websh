//! Shared breadcrumb navigation component.
//!
//! Used by Reader and other surfaces to display current path with clickable segments.
//! Supports mobile-responsive collapsed mode.

use leptos::prelude::*;

use crate::shared::icons as ic;
use websh_core::domain::VirtualPath;
use websh_core::filesystem::{
    RouteFrame, RouteRequest, RouteSurface, request_path_for_canonical_path,
};

stylance::import_crate_style!(css, "src/shared/components/breadcrumb.module.css");

/// Segment data for breadcrumb rendering.
#[derive(Clone)]
struct BreadcrumbSegment {
    /// Display label
    label: String,
    /// Icon to show
    icon: ic::UiIcon,
    /// Target route for navigation (None = current/disabled)
    target: Option<RouteRequest>,
}

/// Shared breadcrumb navigation component.
///
/// Displays the current path as clickable segments for navigation.
/// Automatically handles Root, Browse, and Read routes.
#[component]
pub fn Breadcrumb(
    route: Memo<RouteFrame>,
    on_navigate: Callback<RouteRequest>,
    /// Show root "/" segment
    #[prop(default = false)]
    show_root: bool,
) -> impl IntoView {
    view! {
        <nav class=css::breadcrumb>
            {move || {
                let route = route.get();
                let display = route.display_path();

                // Handle Root specially
                if route.is_root() {
                    return view! {
                        <SegmentCurrent icon=ic::SERVER label="/".to_string() />
                    }.into_any();
                }

                let segments: Vec<&str> = display.split('/').filter(|s| !s.is_empty()).collect();

                // Build segment data
                let mut segment_data: Vec<BreadcrumbSegment> = Vec::new();

                // Root segment (optional)
                if show_root {
                    segment_data.push(BreadcrumbSegment {
                        label: "/".to_string(),
                        icon: ic::SERVER,
                        target: Some(RouteRequest::new("/")),
                    });
                }

                // Path segments
                for (idx, segment) in segments.iter().enumerate() {
                    let is_last = idx == segments.len() - 1;
                    let is_home_segment = *segment == "~";

                    // Determine icon
                    let icon = if is_home_segment {
                        ic::HOME
                    } else if is_last && route.is_file() {
                        ic::FILE
                    } else {
                        ic::FOLDER
                    };

                    // Build target route for navigation
                    // Use absolute path construction, not relative join
                    let target = if is_last {
                        None // Current segment is not clickable
                    } else if is_home_segment {
                        Some(RouteRequest::new("/"))
                    } else {
                        let path = canonical_segment_path(&segments, idx);
                        Some(RouteRequest::new(request_path_for_canonical_path(
                            &path,
                            RouteSurface::Content,
                        )))
                    };

                    segment_data.push(BreadcrumbSegment {
                        label: segment.to_string(),
                        icon,
                        target,
                    });
                }

                // Render segments
                let views: Vec<_> = segment_data
                    .into_iter()
                    .enumerate()
                    .map(|(idx, seg)| {
                        let show_separator = idx > 0;

                        view! {
                            <>
                                {show_separator.then(|| view! {
                                    <span class=css::separator>
                                        <ic::SvgIcon icon=ic::CHEVRON_RIGHT />
                                    </span>
                                })}
                                {if let Some(target) = seg.target.clone() {
                                    view! {
                                        <SegmentLink
                                            icon=seg.icon
                                            label=seg.label.clone()
                                            on_click=move || on_navigate.run(target.clone())
                                        />
                                    }.into_any()
                                } else {
                                    view! {
                                        <SegmentCurrent icon=seg.icon label=seg.label.clone() />
                                    }.into_any()
                                }}
                            </>
                        }
                    })
                    .collect();

                views.collect_view().into_any()
            }}
        </nav>
    }
}

fn canonical_segment_path(segments: &[&str], idx: usize) -> VirtualPath {
    if segments.first() == Some(&"~") {
        let rel = build_segment_path(segments, idx);
        if rel.is_empty() {
            return VirtualPath::root();
        }
        return VirtualPath::from_absolute(format!("/{rel}")).expect("constant path");
    }

    let abs = format!("/{}", segments[..=idx].join("/"));
    VirtualPath::from_absolute(abs).expect("constant path")
}

/// Clickable breadcrumb segment.
#[component]
fn SegmentLink<F>(icon: ic::UiIcon, label: String, on_click: F) -> impl IntoView
where
    F: Fn() + 'static,
{
    view! {
        <button
            class=css::segment
            on:click=move |_| on_click()
        >
            <span class=css::icon><ic::SvgIcon icon=icon /></span>
            <span class=css::label>{label}</span>
        </button>
    }
}

/// Current (disabled) breadcrumb segment.
#[component]
fn SegmentCurrent(icon: ic::UiIcon, label: String) -> impl IntoView {
    view! {
        <button class=format!("{} {}", css::segment, css::segmentCurrent) disabled=true>
            <span class=css::icon><ic::SvgIcon icon=icon /></span>
            <span class=css::label>{label}</span>
        </button>
    }
}

/// Build the absolute path for a breadcrumb segment click.
///
/// `segments`: full breadcrumb segments from the current route, including
/// any leading "~" mount alias.
/// `idx`: the clicked segment's index into `segments`.
///
/// If segments starts with "~", the home mount alias is skipped when joining.
fn build_segment_path(segments: &[&str], idx: usize) -> String {
    let start_idx = if segments.first() == Some(&"~") { 1 } else { 0 };
    if idx < start_idx {
        return String::new();
    }
    segments[start_idx..=idx].join("/")
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::build_segment_path;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn build_segment_path_cases() {
        let cases = [
            (vec!["~", "blog", "posts"], 1, "blog"),
            (vec!["~", "blog", "posts"], 2, "blog/posts"),
            (vec!["work", "notes"], 0, "work"),
            (vec!["work", "notes"], 1, "work/notes"),
            (vec!["~", "blog"], 0, ""),
            (vec!["~", "a", "b", "c", "d"], 4, "a/b/c/d"),
        ];

        for (segments, idx, expected) in cases {
            assert_eq!(build_segment_path(&segments, idx), expected);
        }
    }
}
