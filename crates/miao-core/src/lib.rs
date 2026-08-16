mod error;
mod handlers;
mod instance;
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
pub use instance::{
    acquire_single_instance, focus_existing_window, peek_single_instance, InstanceAcquire,
    InstanceGuard, InstancePeek,
};
pub use paths::default_log_path;
pub use privilege::{is_elevated, require_privileges, show_user_error};
pub use runtime::{run, spawn_server, RuntimeOptions, ServerHandle};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
