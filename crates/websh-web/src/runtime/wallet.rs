//! Browser wallet runtime adapter.

use js_sys::{Array, Function, Object, Promise, Reflect};
use serde::Deserialize;
use thiserror::Error;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::Closure;
use wasm_bindgen_futures::JsFuture;

use crate::config::WALLET_TIMEOUT_MS;
use crate::platform::fetch::{RaceResult, fetch_json, race_with_timeout};
use crate::platform::js_value_message;

use super::state::EnvironmentError;

#[derive(Debug, Clone, Error)]
pub enum WalletError {
    #[error("browser window not available")]
    NoWindow,
    #[error("no wallet provider detected; install a browser wallet extension")]
    NotInstalled,
    #[error("failed to create wallet request")]
    RequestCreationFailed,
    #[error("wallet request rejected: {0}")]
    RequestRejected(String),
    #[error("no account returned from wallet")]
    NoAccount,
}

fn get_ethereum() -> Result<Object, WalletError> {
    let window = web_sys::window().ok_or(WalletError::NoWindow)?;
    Reflect::get(&window, &"ethereum".into())
        .ok()
        .and_then(|v| v.dyn_into::<Object>().ok())
        .ok_or(WalletError::NotInstalled)
}

async fn ethereum_request(method: &str) -> Result<JsValue, WalletError> {
    let ethereum = get_ethereum()?;

    let args = Object::new();
    Reflect::set(&args, &"method".into(), &method.into())
        .map_err(|_| WalletError::RequestCreationFailed)?;

    let request = Reflect::get(&ethereum, &"request".into())
        .map_err(|_| WalletError::RequestCreationFailed)?
        .dyn_into::<Function>()
        .map_err(|_| WalletError::RequestCreationFailed)?;

    let promise: Promise = request
        .call1(&ethereum, &args)
        .map_err(|_| WalletError::RequestCreationFailed)?
        .into();

    JsFuture::from(promise)
        .await
        .map_err(|error| WalletError::RequestRejected(js_value_message(&error)))
}

pub fn is_available() -> bool {
    get_ethereum().is_ok()
}

pub async fn get_chain_id() -> Option<u64> {
    let result = ethereum_request("eth_chainId").await.ok()?;
    let hex_str = result.as_string()?;
    u64::from_str_radix(hex_str.trim_start_matches("0x"), 16).ok()
}

pub async fn connect() -> Result<String, WalletError> {
    let result = ethereum_request("eth_requestAccounts").await?;
    let accounts = Array::from(&result);

    accounts.get(0).as_string().ok_or(WalletError::NoAccount)
}

pub async fn get_account() -> Option<String> {
    let ethereum = get_ethereum().ok()?;

    let args = Object::new();
    Reflect::set(&args, &"method".into(), &"eth_accounts".into()).ok()?;

    let request_fn = Reflect::get(&ethereum, &"request".into())
        .ok()?
        .dyn_into::<Function>()
        .ok()?;

    let request_promise: Promise = request_fn.call1(&ethereum, &args).ok()?.into();

    match race_with_timeout(request_promise, WALLET_TIMEOUT_MS).await {
        RaceResult::Completed(result) => Array::from(&result).get(0).as_string(),
        RaceResult::TimedOut | RaceResult::Error(_) => None,
    }
}

#[derive(Deserialize)]
struct EnsResponse {
    name: Option<String>,
}

pub async fn resolve_ens(address: &str) -> Option<String> {
    let url = format!("https://api.ensideas.com/ens/resolve/{address}");

    match fetch_json::<EnsResponse>(&url).await {
        Ok(response) => response.name,
        Err(_) => None,
    }
}

#[derive(Debug, Clone)]
pub struct ConnectOutcome {
    pub address: String,
    pub chain_id: Option<u64>,
    pub ens_name: Option<String>,
    pub session_persist_error: Option<EnvironmentError>,
}

pub struct WalletEventListeners {
    _accounts: WalletEventListener,
    _chain: WalletEventListener,
}

impl WalletEventListeners {
    pub fn new(accounts: WalletEventListener, chain: WalletEventListener) -> Self {
        Self {
            _accounts: accounts,
            _chain: chain,
        }
    }
}

pub struct WalletEventListener {
    ethereum: Object,
    event: &'static str,
    closure: Closure<dyn Fn(JsValue)>,
}

impl Drop for WalletEventListener {
    fn drop(&mut self) {
        remove_wallet_listener(&self.ethereum, self.event, self.closure.as_ref());
    }
}

pub fn on_accounts_changed(
    callback: impl Fn(Option<String>) + 'static,
) -> Result<WalletEventListener, WalletError> {
    let ethereum = get_ethereum()?;

    let closure = Closure::wrap(Box::new(move |accounts: JsValue| {
        let account = Array::from(&accounts).get(0).as_string();
        callback(account);
    }) as Box<dyn Fn(JsValue)>);

    let on_fn = Reflect::get(&ethereum, &"on".into())
        .map_err(|_| WalletError::RequestCreationFailed)?
        .dyn_into::<Function>()
        .map_err(|_| WalletError::RequestCreationFailed)?;

    on_fn
        .call2(&ethereum, &"accountsChanged".into(), closure.as_ref())
        .map_err(|_| WalletError::RequestCreationFailed)?;

    Ok(WalletEventListener {
        ethereum,
        event: "accountsChanged",
        closure,
    })
}

pub fn on_chain_changed(
    callback: impl Fn(String) + 'static,
) -> Result<WalletEventListener, WalletError> {
    let ethereum = get_ethereum()?;

    let closure = Closure::wrap(Box::new(move |chain_id: JsValue| {
        if let Some(id) = chain_id.as_string() {
            callback(id);
        }
    }) as Box<dyn Fn(JsValue)>);

    let on_fn = Reflect::get(&ethereum, &"on".into())
        .map_err(|_| WalletError::RequestCreationFailed)?
        .dyn_into::<Function>()
        .map_err(|_| WalletError::RequestCreationFailed)?;

    on_fn
        .call2(&ethereum, &"chainChanged".into(), closure.as_ref())
        .map_err(|_| WalletError::RequestCreationFailed)?;

    Ok(WalletEventListener {
        ethereum,
        event: "chainChanged",
        closure,
    })
}

fn remove_wallet_listener(ethereum: &Object, event: &'static str, closure: &JsValue) {
    for method in ["removeListener", "off"] {
        let Ok(value) = Reflect::get(ethereum, &method.into()) else {
            continue;
        };
        let Ok(function) = value.dyn_into::<Function>() else {
            continue;
        };
        if function.call2(ethereum, &event.into(), closure).is_ok() {
            return;
        }
    }
}
