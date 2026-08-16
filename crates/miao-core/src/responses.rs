use std::fmt::Display;

use axum::{http::StatusCode, response::Json};
use serde::Serialize;

use crate::models::ApiResponse;

pub type HandlerResult<T = ()> = Result<Json<ApiResponse<T>>, (StatusCode, Json<ApiResponse<T>>)>;

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
    fn status_error_preserves_http_status_and_response_body() {
        let (status, Json(response)) =
            status_error::<()>(StatusCode::BAD_REQUEST, TestMessage("bad request"));

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!response.success);
        assert_eq!(response.message, "bad request");
        assert!(response.data.is_none());
    }
}
