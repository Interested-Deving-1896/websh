//! Native build-time CLI: clap dispatchers + engine modules.

pub mod cli;
pub(crate) mod commands;
pub(crate) mod infra;
pub(crate) mod workflows;

pub(crate) type CliResult<T = ()> = anyhow::Result<T>;

pub use cli::run;
