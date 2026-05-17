//! Boot sequence logic
//!
//! Handles the initial terminal animation and applies the pure runtime loader.

use wasm_bindgen_futures::spawn_local;

use crate::app::AppContext;
use crate::app::RuntimeServices;
use crate::config::{APP_NAME, APP_TAGLINE, APP_VERSION, ASCII_BANNER, boot_delays};
use websh_core::shell::OutputLine;
use websh_core::support::format::{format_elapsed, format_eth_address};

/// Delay helper using setTimeout
async fn delay(window: &web_sys::Window, ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Run the boot sequence
///
/// Initializes the application by:
/// 1. Booting the kernel and WASM runtime
/// 2. Fetching and mounting the runtime filesystem
/// 3. Restoring wallet session if available
/// 4. Displaying the welcome banner
/// 5. Displaying the initial terminal prompt
pub fn run(ctx: AppContext) {
    spawn_local(async move {
        let window = web_sys::window().expect("Boot sequence requires browser environment");
        let start = js_sys::Date::now();
        let elapsed = || js_sys::Date::now() - start;
        let services = RuntimeServices::new(ctx);

        services.init_default_env();

        ctx.terminal.push_output(OutputLine::info(format!(
            "{} Booting websh kernel v{}",
            format_elapsed(elapsed()),
            APP_VERSION
        )));
        delay(&window, boot_delays::KERNEL_INIT).await;

        ctx.terminal.push_output(OutputLine::success(format!(
            "{} WASM runtime initialized",
            format_elapsed(elapsed())
        )));
        delay(&window, boot_delays::WASM_RUNTIME).await;

        ctx.terminal.push_output(OutputLine::text(format!(
            "{} Mounting filesystems...",
            format_elapsed(elapsed())
        )));

        services.mark_root_mount_loading();
        match services.load_runtime().await {
            Ok(load) => {
                let total_files = load.total_files;
                let failed_mounts = load.mounts.failed_entries();
                let scan_jobs = load.mounts.scan_jobs.clone();
                let generation = services.apply_successful_root_mount_load(load);
                services.start_mount_scans(generation, scan_jobs);
                ctx.terminal.push_output(OutputLine::success(format!(
                    "{} Total: {} files mounted",
                    format_elapsed(elapsed()),
                    total_files
                )));
                for failure in failed_mounts {
                    let error = failure.error().unwrap_or("unavailable");
                    ctx.terminal.push_output(OutputLine::error(format!(
                        "{} mount {} unavailable: {}",
                        format_elapsed(elapsed()),
                        failure.declared.label,
                        error
                    )));
                }
            }
            Err(error) => {
                services.apply_failed_root_mount_load(error.to_string());
                ctx.terminal.push_output(OutputLine::error(format!(
                    "{} Failed to mount filesystems: {}",
                    format_elapsed(elapsed()),
                    error
                )));
            }
        }

        if services.wallet_available() && services.has_wallet_session() {
            ctx.terminal.push_output(OutputLine::text(format!(
                "{} Restoring wallet session...",
                format_elapsed(elapsed())
            )));

            match services.wallet_account().await {
                Some(address) => {
                    let short_addr = format_eth_address(&address);
                    ctx.terminal.push_output(OutputLine::success(format!(
                        "{} Connected: {}",
                        format_elapsed(elapsed()),
                        short_addr
                    )));

                    let chain_id = services.wallet_chain_id().await;
                    if let Some(id) = chain_id {
                        ctx.terminal.push_output(OutputLine::info(format!(
                            "{} Network: {} (chain_id={})",
                            format_elapsed(elapsed()),
                            websh_core::domain::chain_name(id),
                            id
                        )));
                    }

                    let ens_name = services.resolve_wallet_ens(&address).await;
                    if let Some(ref name) = ens_name {
                        ctx.terminal.push_output(OutputLine::success(format!(
                            "{} ENS resolved: {}",
                            format_elapsed(elapsed()),
                            name
                        )));
                    }

                    match services.restore_wallet_session(address, chain_id, ens_name) {
                        Ok(()) => {}
                        Err(error) => ctx.terminal.push_output(OutputLine::error(format!(
                            "wallet: failed to persist session: {error}"
                        ))),
                    }
                }
                None => {
                    match services.disconnect_wallet() {
                        Ok(()) => {}
                        Err(error) => ctx.terminal.push_output(OutputLine::error(format!(
                            "wallet: failed to clear session: {error}"
                        ))),
                    }
                    ctx.terminal.push_output(OutputLine::text(format!(
                        "{} Wallet session expired",
                        format_elapsed(elapsed())
                    )));
                }
            }
        }

        ctx.terminal.push_output(OutputLine::info(format!(
            "{} Initializing Terminal mode",
            format_elapsed(elapsed())
        )));
        delay(&window, boot_delays::BOOT_COMPLETE).await;

        ctx.terminal.push_output(OutputLine::success(format!(
            "{} Boot complete. Welcome to {}",
            format_elapsed(elapsed()),
            APP_NAME
        )));

        ctx.terminal.push_output(OutputLine::empty());
        ctx.terminal.push_output(OutputLine::ascii(ASCII_BANNER));
        ctx.terminal.push_output(OutputLine::empty());
        ctx.terminal.push_output(OutputLine::info(APP_TAGLINE));
        ctx.terminal.push_output(OutputLine::empty());
        ctx.terminal.push_output(OutputLine::text("Tips:"));
        ctx.terminal
            .push_output(OutputLine::text("  - Type 'help' for available commands"));
        ctx.terminal.push_output(OutputLine::text(
            "  - Use the archive bar to jump between home, ledger, and websh",
        ));
        ctx.terminal.push_output(OutputLine::empty());
    });
}
