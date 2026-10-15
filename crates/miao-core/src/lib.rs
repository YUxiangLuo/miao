#[cfg(not(target_os = "linux"))]
compile_error!("Miao supports Linux/OpenWrt only");

mod cli;
mod error;
mod handlers;
mod models;
mod paths;
mod privilege;
mod profile;
mod responses;
mod router;
mod runtime;
mod services;
mod state;
#[cfg(test)]
mod test_support;
mod validation;

pub use cli::config_path_from_args;
pub use error::{AppError, AppResult};
pub use runtime::{run, spawn_server, RuntimeOptions, ServerHandle};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
