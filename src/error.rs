use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

#[derive(Debug)]
pub enum AppError {
    Message(String),
    AlreadyRunning,
    Io(std::io::Error),
    Json(serde_json::Error),
    Yaml(serde_yaml::Error),
    Http(reqwest::Error),
    Context {
        context: String,
        source: Box<AppError>,
    },
}

impl AppError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn context(context: impl Into<String>, source: impl Into<AppError>) -> Self {
        Self::Context {
            context: context.into(),
            source: Box::new(source.into()),
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => write!(f, "{message}"),
            Self::AlreadyRunning => write!(f, "sing-box is already running"),
            Self::Io(err) => write!(f, "{err}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Yaml(err) => write!(f, "{err}"),
            Self::Http(err) => write!(f, "{err}"),
            Self::Context { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Message(_) => None,
            Self::AlreadyRunning => None,
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::Yaml(err) => Some(err),
            Self::Http(err) => Some(err),
            Self::Context { source, .. } => Some(source.as_ref()),
        }
    }
}

impl From<&str> for AppError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<String> for AppError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::Yaml(value)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::AppError;

    #[test]
    fn message_error_displays_plain_text_and_has_no_source() {
        let err = AppError::message("plain failure");

        assert_eq!(err.to_string(), "plain failure");
        assert!(err.source().is_none());
    }

    #[test]
    fn context_error_displays_full_chain_and_exposes_source_chain() {
        let err = AppError::context(
            "while saving config",
            AppError::context("while writing file", std::io::Error::other("disk full")),
        );

        assert_eq!(
            err.to_string(),
            "while saving config: while writing file: disk full"
        );

        let source = err.source().unwrap();
        assert_eq!(source.to_string(), "while writing file: disk full");
        let nested = source.source().unwrap();
        assert_eq!(nested.to_string(), "disk full");
    }

    #[test]
    fn io_error_conversion_preserves_original_message() {
        let err: AppError = std::io::Error::other("permission denied").into();

        assert_eq!(err.to_string(), "permission denied");
        assert!(err.source().is_some());
    }
}
