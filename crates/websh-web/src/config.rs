//! Browser application configuration.

/// ASCII banner displayed after boot sequence.
pub const ASCII_BANNER: &str = websh_site::ASCII_BANNER;

/// Application name displayed in terminal and chrome.
pub const APP_NAME: &str = websh_site::APP_NAME;

/// Application version.
pub const APP_VERSION: &str = "0.1.0";

/// User tagline displayed after boot.
pub const APP_TAGLINE: &str = websh_site::APP_TAGLINE;

/// Fetch request timeout in milliseconds.
pub const FETCH_TIMEOUT_MS: i32 = 10000;

/// localStorage key for wallet session persistence.
pub const WALLET_SESSION_KEY: &str = "websh.wallet_session";

/// Wallet connection timeout in milliseconds.
pub const WALLET_TIMEOUT_MS: i32 = 2000;

/// Prefix for user environment variables in localStorage.
pub const USER_VAR_PREFIX: &str = "user.";

/// User environment variable used as content language preference.
pub const LANG_ENV_KEY: &str = "LANG";

/// Fallback language when browser preference is unavailable or invalid.
pub const DEFAULT_LANG: &str = "en";

/// Default user variables initialized on first visit.
/// `LANG` is initialized from the browser language separately. `THEME` is
/// omitted: the theme system writes `user.THEME` directly.
pub const DEFAULT_USER_VARS: &[(&str, &str)] = &[("EDITOR", "vim")];

/// Maximum number of terminal output lines to keep in history.
pub const MAX_TERMINAL_HISTORY: usize = 1000;

/// Maximum number of command history entries to keep.
pub const MAX_COMMAND_HISTORY: usize = 100;

/// Milliseconds per second for time formatting.
pub const MS_PER_SECOND: f64 = 1000.0;

/// Boot sequence animation delay constants (milliseconds).
pub mod boot_delays {
    /// Delay after kernel init message.
    pub const KERNEL_INIT: i32 = 30;
    /// Delay after WASM runtime message.
    pub const WASM_RUNTIME: i32 = 20;
    /// Delay after boot complete message.
    pub const BOOT_COMPLETE: i32 = 40;
}
