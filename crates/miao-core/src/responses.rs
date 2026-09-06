use std::fmt::Display;

use axum::{http::StatusCode, response::Json};
use serde::Serialize;

use crate::models::ApiResponse;

pub type HandlerResult<T = ()> = Result<Json<ApiResponse<T>>, (StatusCode, Json<ApiResponse<T>>)>;

/// The only mapping from application errors to HTTP status codes.
pub fn command_result<T: Serialize>(
    result: crate::services::commands::CommandResult<T>,
) -> HandlerResult<T> {
    use crate::services::commands::CommandErrorKind;
    result.map(command_reply).map_err(|error| {
        let status = match error.kind {
            CommandErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
            CommandErrorKind::Conflict => StatusCode::CONFLICT,
            CommandErrorKind::NotFound => StatusCode::NOT_FOUND,
            CommandErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            #[cfg(not(windows))]
            CommandErrorKind::Upstream => StatusCode::BAD_GATEWAY,
        };
        status_error(status, error)
    })
}

pub fn command_reply<T: Serialize>(
    reply: crate::services::commands::CommandReply<T>,
) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        message: reply.message,
        data: reply.data,
    })
}

pub fn success<T: Serialize>(message: impl Display, data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse::success(message.to_string(), data))
}

pub fn success_no_data<T: Serialize>(message: impl Display) -> Json<ApiResponse<T>> {
    Json(ApiResponse::success_no_data(message.to_string()))
}

pub fn error<T: Serialize>(message: impl Display) -> Json<ApiResponse<T>> {
    Json(ApiResponse::error(message.to_string()))
}

pub fn status_error<T: Serialize>(
    status: StatusCode,
    message: impl Display,
) -> (StatusCode, Json<ApiResponse<T>>) {
    (status, error(message))
}

#[cfg(test)]
mod tests {
    use std::fmt::{self, Display, Formatter};

    use axum::{http::StatusCode, response::Json};

    use super::{error, status_error, success, success_no_data};

    struct TestMessage(&'static str);

    impl Display for TestMessage {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    #[test]
    fn success_wraps_message_and_payload() {
        let Json(response) = success(TestMessage("created"), 42_u32);

        assert!(response.success);
        assert_eq!(response.message, "created");
        assert_eq!(response.data, Some(42));
    }

    #[test]
    fn success_no_data_sets_empty_payload() {
        let Json(response) = success_no_data::<()>(TestMessage("done"));

        assert!(response.success);
        assert_eq!(response.message, "done");
        assert!(response.data.is_none());
    }

    #[test]
    fn error_wraps_message_without_payload() {
        let Json(response) = error::<()>(TestMessage("boom"));

        assert!(!response.success);
        assert_eq!(response.message, "boom");
        assert!(response.data.is_none());
    }

    #[test]
    fn application_errors_keep_the_http_status_and_envelope() {
        use crate::services::commands::{CommandError, CommandErrorKind::*};
        for (kind, expected) in [
            (InvalidInput, StatusCode::BAD_REQUEST),
            (Conflict, StatusCode::CONFLICT),
            (NotFound, StatusCode::NOT_FOUND),
            (Internal, StatusCode::INTERNAL_SERVER_ERROR),
            #[cfg(not(windows))]
            (Upstream, StatusCode::BAD_GATEWAY),
        ] {
            let (status, Json(response)) = super::command_result::<()>(Err(CommandError {
                kind,
                message: "unchanged message".into(),
            }))
            .err()
            .unwrap();
            assert_eq!(status, expected);
            assert!(!response.success);
            assert_eq!(response.message, "unchanged message");
            assert!(response.data.is_none());
        }
    }

    #[test]
    fn status_error_preserves_http_status_and_response_body() {
        let (status, Json(response)) =
            status_error::<()>(StatusCode::BAD_REQUEST, TestMessage("bad request"));

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.success);
        assert_eq!(response.message, "bad request");
        assert!(response.data.is_none());
    }
}
