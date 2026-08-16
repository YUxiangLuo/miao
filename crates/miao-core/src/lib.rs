mod error;
mod handlers;
mod models;
mod paths;
mod platform;
mod privilege;
mod responses;
mod router;
mod runtime;
mod services;
mod state;
#[cfg(test)]
mod test_support;
mod validation;

pub use error::{AppError, AppResult};
pub use privilege::{is_elevated, require_privileges};
pub use runtime::{run, spawn_server, RuntimeOptions, ServerHandle};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
