//! Transport-independent application operations. HTTP and MCP adapt these
//! replies/errors; application code must never call an HTTP handler.
use std::fmt::{self, Display};

pub mod nodes;
pub mod rules;
pub mod service;
pub mod settings;
pub mod subs;
#[cfg(test)]
mod tests;
#[cfg(not(windows))]
pub mod vps;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandErrorKind {
    InvalidInput,
    Conflict,
    NotFound,
    Internal,
    // Remote provisioning is not compiled on Windows.
    #[cfg(not(windows))]
    Upstream,
}

#[derive(Debug)]
pub struct CommandError {
    pub kind: CommandErrorKind,
    pub message: String,
}

impl Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}
impl std::error::Error for CommandError {}

#[derive(Debug)]
pub struct CommandReply<T = ()> {
    pub message: String,
    pub data: Option<T>,
}

pub type CommandResult<T = ()> = Result<CommandReply<T>, CommandError>;

fn ensure_initialized(state: &crate::state::AppState) -> Result<(), CommandError> {
    if state
        .initializing
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        Err(command_error(
            CommandErrorKind::Conflict,
            "Initialization is still in progress",
        ))
    } else {
        Ok(())
    }
}

fn success<T>(message: impl Display, data: T) -> CommandReply<T> {
    CommandReply {
        message: message.to_string(),
        data: Some(data),
    }
}
fn success_no_data<T>(message: impl Display) -> CommandReply<T> {
    CommandReply {
        message: message.to_string(),
        data: None,
    }
}
fn command_error(kind: CommandErrorKind, message: impl Display) -> CommandError {
    CommandError {
        kind,
        message: message.to_string(),
    }
}
