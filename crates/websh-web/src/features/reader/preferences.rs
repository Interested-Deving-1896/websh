//! Reader-local preferences.
//!
//! These settings are viewer preferences, not authoring state. They live in
//! browser storage and only affect the reader body surface.

use super::intent::ReaderIntent;
use websh_core::domain::FileType;

pub const TEXT_SCALE_STORAGE_KEY: &str = "reader.TEXT_SCALE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderTextScale {
    Small,
    Normal,
    Large,
    XLarge,
}

impl ReaderTextScale {
    pub fn attr(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Normal => "normal",
            Self::Large => "large",
            Self::XLarge => "xlarge",
        }
    }

    pub fn smaller(self) -> Option<Self> {
        match self {
            Self::Small => None,
            Self::Normal => Some(Self::Small),
            Self::Large => Some(Self::Normal),
            Self::XLarge => Some(Self::Large),
        }
    }

    pub fn larger(self) -> Option<Self> {
        match self {
            Self::Small => Some(Self::Normal),
            Self::Normal => Some(Self::Large),
            Self::Large => Some(Self::XLarge),
            Self::XLarge => None,
        }
    }
}

pub fn parse_text_scale(raw: &str) -> Option<ReaderTextScale> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "small" => Some(ReaderTextScale::Small),
        "normal" => Some(ReaderTextScale::Normal),
        "large" => Some(ReaderTextScale::Large),
        "xlarge" | "extra-large" | "extra_large" => Some(ReaderTextScale::XLarge),
        _ => None,
    }
}

pub fn initial_text_scale() -> ReaderTextScale {
    stored_text_scale().unwrap_or(ReaderTextScale::Normal)
}

pub fn persist_text_scale(scale: ReaderTextScale) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        {
            let _ = storage.set_item(TEXT_SCALE_STORAGE_KEY, scale.attr());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = scale;
    }
}

pub fn intent_supports_text_scale(intent: &ReaderIntent) -> bool {
    match intent {
        ReaderIntent::Html { .. } | ReaderIntent::Markdown { .. } | ReaderIntent::Plain { .. } => {
            true
        }
        ReaderIntent::BundleVariant { variant_path, .. } => {
            matches!(
                FileType::from_path(variant_path.as_str()),
                FileType::Html | FileType::Markdown
            )
        }
        ReaderIntent::Asset { .. } | ReaderIntent::Redirect { .. } => false,
    }
}

fn stored_text_scale() -> Option<ReaderTextScale> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(TEXT_SCALE_STORAGE_KEY).ok().flatten())
            .and_then(|value| parse_text_scale(&value))
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;
    use websh_core::domain::VirtualPath;

    wasm_bindgen_test_configure!(run_in_browser);

    fn vp(path: &str) -> VirtualPath {
        VirtualPath::from_absolute(path).expect("test path")
    }

    #[wasm_bindgen_test]
    fn parses_text_scale_values() {
        assert_eq!(parse_text_scale("small"), Some(ReaderTextScale::Small));
        assert_eq!(parse_text_scale("NORMAL"), Some(ReaderTextScale::Normal));
        assert_eq!(parse_text_scale("large"), Some(ReaderTextScale::Large));
        assert_eq!(
            parse_text_scale("extra-large"),
            Some(ReaderTextScale::XLarge)
        );
        assert_eq!(parse_text_scale("unknown"), None);
    }

    #[wasm_bindgen_test]
    fn text_scale_steps_are_bounded() {
        assert_eq!(ReaderTextScale::Small.smaller(), None);
        assert_eq!(
            ReaderTextScale::Small.larger(),
            Some(ReaderTextScale::Normal)
        );
        assert_eq!(
            ReaderTextScale::Normal.smaller(),
            Some(ReaderTextScale::Small)
        );
        assert_eq!(
            ReaderTextScale::Normal.larger(),
            Some(ReaderTextScale::Large)
        );
        assert_eq!(
            ReaderTextScale::XLarge.smaller(),
            Some(ReaderTextScale::Large)
        );
        assert_eq!(ReaderTextScale::XLarge.larger(), None);
    }

    #[wasm_bindgen_test]
    fn text_scale_is_limited_to_text_readers() {
        assert!(intent_supports_text_scale(&ReaderIntent::Markdown {
            node_path: vp("/note.md")
        }));
        assert!(intent_supports_text_scale(&ReaderIntent::Html {
            node_path: vp("/index.html")
        }));
        assert!(intent_supports_text_scale(&ReaderIntent::Plain {
            node_path: vp("/note.txt")
        }));
        assert!(intent_supports_text_scale(&ReaderIntent::BundleVariant {
            bundle_path: vp("/writing/foo"),
            variant_id: "en".to_string(),
            variant_path: vp("/writing/foo/en.md")
        }));
        assert!(!intent_supports_text_scale(&ReaderIntent::Asset {
            node_path: vp("/paper.pdf"),
            media_type: "application/pdf".to_string()
        }));
    }
}
