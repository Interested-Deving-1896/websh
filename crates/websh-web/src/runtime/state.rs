//! Browser runtime-state owner.

use std::cell::RefCell;
use std::collections::BTreeMap;

use thiserror::Error;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use crate::config::{
    DEFAULT_LANG, DEFAULT_USER_VARS, LANG_ENV_KEY, USER_VAR_PREFIX, WALLET_SESSION_KEY,
};
#[cfg(target_arch = "wasm32")]
use websh_core::support::normalize_locale_tag;

pub use websh_core::runtime::RuntimeStateSnapshot;

const GITHUB_TOKEN_KEY: &str = "websh.gh_token";

#[derive(Debug, Clone, Error)]
pub enum EnvironmentError {
    #[error("localStorage not available")]
    StorageUnavailable,
    #[error("invalid variable name (use letters, numbers, underscores)")]
    InvalidVariableName,
    #[error("failed to save to localStorage")]
    SaveFailed,
    #[error("failed to remove from localStorage")]
    RemoveFailed,
}

#[derive(Clone, Default)]
struct BrowserRuntimeStateLoad {
    pub env: BTreeMap<String, String>,
    pub github_token: Option<String>,
    pub wallet_session: bool,
}

#[derive(Clone, Default)]
struct RuntimeState {
    env: BTreeMap<String, String>,
    github_token: Option<String>,
    wallet_session: bool,
}

impl RuntimeState {
    fn snapshot(&self) -> RuntimeStateSnapshot {
        RuntimeStateSnapshot {
            env: self.env.clone(),
            github_token_present: self.github_token.is_some(),
            wallet_session: self.wallet_session,
        }
    }
}

impl From<BrowserRuntimeStateLoad> for RuntimeState {
    fn from(value: BrowserRuntimeStateLoad) -> Self {
        Self {
            env: value.env,
            github_token: value.github_token,
            wallet_session: value.wallet_session,
        }
    }
}

thread_local! {
    static RUNTIME_STATE: RefCell<Option<RuntimeState>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut RuntimeState) -> R) -> R {
    RUNTIME_STATE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.get_or_insert_with(|| load_from_browser_storage().into());
        f(state)
    })
}

pub fn install_browser_persistence() {
    RUNTIME_STATE.with(|slot| *slot.borrow_mut() = None);
}

pub fn snapshot() -> RuntimeStateSnapshot {
    with_state(|state| state.snapshot())
}

pub fn get_env_var(key: &str) -> Option<String> {
    with_state(|state| state.env.get(key).cloned())
}

pub fn set_env_var(key: &str, value: &str) -> Result<RuntimeStateSnapshot, EnvironmentError> {
    if !is_valid_var_name(key) {
        return Err(EnvironmentError::InvalidVariableName);
    }

    persist_env_var(key, value)?;
    with_state(|state| {
        state.env.insert(key.to_string(), value.to_string());
    });
    Ok(snapshot())
}

pub fn unset_env_var(key: &str) -> Result<RuntimeStateSnapshot, EnvironmentError> {
    if !is_valid_var_name(key) {
        return Err(EnvironmentError::InvalidVariableName);
    }

    remove_env_var(key)?;
    with_state(|state| {
        state.env.remove(key);
    });
    Ok(snapshot())
}

pub fn init_default_env() {
    if get_env_var(LANG_ENV_KEY).is_none() {
        let _ = set_env_var(LANG_ENV_KEY, &browser_default_lang());
    }

    for (key, value) in DEFAULT_USER_VARS {
        if get_env_var(key).is_none() {
            let _ = set_env_var(key, value);
        }
    }
}

pub fn github_token_for_commit() -> Option<String> {
    with_state(|state| state.github_token.clone())
}

pub fn set_github_token(token: &str) -> Result<RuntimeStateSnapshot, EnvironmentError> {
    persist_github_token(token)?;
    with_state(|state| {
        state.github_token = Some(token.to_string());
    });
    Ok(snapshot())
}

pub fn clear_github_token() -> Result<RuntimeStateSnapshot, EnvironmentError> {
    remove_github_token()?;
    with_state(|state| {
        state.github_token = None;
    });
    Ok(snapshot())
}

pub fn has_wallet_session() -> bool {
    with_state(|state| state.wallet_session)
}

pub fn set_wallet_session(active: bool) -> Result<RuntimeStateSnapshot, EnvironmentError> {
    persist_wallet_session(active)?;
    with_state(|state| {
        state.wallet_session = active;
    });
    Ok(snapshot())
}

pub fn is_valid_var_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();
    let first = chars.next().unwrap();

    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }

    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn load_from_browser_storage() -> BrowserRuntimeStateLoad {
    let mut env = BTreeMap::new();
    let mut wallet_session = false;

    if let Some(storage) = local_storage() {
        let len = storage.length().unwrap_or(0);
        for idx in 0..len {
            if let Ok(Some(key)) = storage.key(idx) {
                if let Some(env_key) = key.strip_prefix(USER_VAR_PREFIX) {
                    if let Ok(Some(value)) = storage.get_item(&key) {
                        env.insert(env_key.to_string(), value);
                    }
                    continue;
                }

                if key == WALLET_SESSION_KEY {
                    wallet_session = storage
                        .get_item(WALLET_SESSION_KEY)
                        .ok()
                        .flatten()
                        .is_some();
                }
            }
        }
    }

    let github_token =
        session_storage().and_then(|storage| storage.get_item(GITHUB_TOKEN_KEY).ok().flatten());

    BrowserRuntimeStateLoad {
        env,
        github_token,
        wallet_session,
    }
}

fn persist_env_var(key: &str, value: &str) -> Result<(), EnvironmentError> {
    let storage = local_storage().ok_or(EnvironmentError::StorageUnavailable)?;
    storage
        .set_item(&format!("{USER_VAR_PREFIX}{key}"), value)
        .map_err(|_| EnvironmentError::SaveFailed)
}

fn remove_env_var(key: &str) -> Result<(), EnvironmentError> {
    let storage = local_storage().ok_or(EnvironmentError::StorageUnavailable)?;
    storage
        .remove_item(&format!("{USER_VAR_PREFIX}{key}"))
        .map_err(|_| EnvironmentError::RemoveFailed)
}

fn persist_github_token(token: &str) -> Result<(), EnvironmentError> {
    let storage = session_storage().ok_or(EnvironmentError::StorageUnavailable)?;
    storage
        .set_item(GITHUB_TOKEN_KEY, token)
        .map_err(|_| EnvironmentError::SaveFailed)
}

fn remove_github_token() -> Result<(), EnvironmentError> {
    let storage = session_storage().ok_or(EnvironmentError::StorageUnavailable)?;
    storage
        .remove_item(GITHUB_TOKEN_KEY)
        .map_err(|_| EnvironmentError::RemoveFailed)
}

fn persist_wallet_session(active: bool) -> Result<(), EnvironmentError> {
    let storage = local_storage().ok_or(EnvironmentError::StorageUnavailable)?;
    if active {
        storage
            .set_item(WALLET_SESSION_KEY, "1")
            .map_err(|_| EnvironmentError::SaveFailed)
    } else {
        storage
            .remove_item(WALLET_SESSION_KEY)
            .map_err(|_| EnvironmentError::RemoveFailed)
    }
}

fn browser_default_lang() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        for candidate in browser_language_candidates() {
            if let Some(locale) = normalize_locale_tag(&candidate) {
                return locale;
            }
        }
    }

    DEFAULT_LANG.to_string()
}

#[cfg(target_arch = "wasm32")]
fn browser_language_candidates() -> Vec<String> {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return Vec::new();
    };
    let navigator = JsValue::from(navigator);
    let mut values = Vec::new();

    if let Ok(languages) = js_sys::Reflect::get(&navigator, &JsValue::from_str("languages"))
        && js_sys::Array::is_array(&languages)
    {
        let languages = js_sys::Array::from(&languages);
        for language in languages.iter() {
            if let Some(language) = language.as_string() {
                values.push(language);
            }
        }
    }

    if let Ok(language) = js_sys::Reflect::get(&navigator, &JsValue::from_str("language"))
        && let Some(language) = language.as_string()
    {
        values.push(language);
    }

    values
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.session_storage().ok()?
}
