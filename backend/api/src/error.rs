use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    UnprocessableEntity,
    PayloadTooLarge,
    RateLimited,
    InternalError,
}

impl ErrorCode {
    fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => Self::BadRequest,
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            StatusCode::FORBIDDEN => Self::Forbidden,
            StatusCode::NOT_FOUND => Self::NotFound,
            StatusCode::CONFLICT => Self::Conflict,
            StatusCode::UNPROCESSABLE_ENTITY => Self::UnprocessableEntity,
            StatusCode::PAYLOAD_TOO_LARGE => Self::PayloadTooLarge,
            StatusCode::TOO_MANY_REQUESTS => Self::RateLimited,
            _ => Self::InternalError,
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error_code: ErrorCode,
    message: String,
    details: Option<Value>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.error_code, self.message)
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error_code: ErrorCode,
    message: String,
    details: Value,
    timestamp: String,
    correlation_id: String,
}

impl ApiError {
    pub fn new(status: StatusCode, error: impl Into<String>, message: impl Into<String>) -> Self {
        let reason = error.into();
        Self {
            status,
            error_code: ErrorCode::from_status(status),
            message: message.into(),
            details: if reason.is_empty() {
                None
            } else {
                Some(json!({ "reason": reason }))
            },
        }
    }

    pub fn with_details(mut self, details: impl Into<Value>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn bad_request(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, error, message)
    }

    pub fn not_found(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, error, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }

    pub fn forbidden_with_error(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, error, message)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", message)
    }

    pub fn unprocessable(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, error, message)
    }

    pub fn conflict(error: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, error, message)
    }

    pub fn db_error(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let correlation_id = crate::request_tracing::current_request_id()
            .unwrap_or_else(crate::request_tracing::generate_request_id);
        let payload = ErrorResponse {
            error_code: self.error_code,
            message: self.message,
            details: self.details.unwrap_or_else(|| json!({})),
            timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            correlation_id: correlation_id.clone(),
        };

        let mut response = (self.status, Json(payload)).into_response();
        crate::request_tracing::attach_request_id_headers(response.headers_mut(), &correlation_id);
        response
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
pub type AppError = ApiError;

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(err = %e, "database error");
        ApiError::internal("Database error")
    }
}

impl From<StatusCode> for ApiError {
    fn from(status: StatusCode) -> Self {
        Self::new(status, format!("{}", ErrorCode::from_status(status)), status.canonical_reason().unwrap_or("Unknown Error"))
    }
}

impl From<String> for ApiError {
    fn from(message: String) -> Self {
        Self::internal(message)
    }
}

impl From<&str> for ApiError {
    fn from(message: &str) -> Self {
        Self::internal(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_error_uses_standard_response_shape() {
        let response = ApiError::bad_request("INVALID_INPUT", "Invalid request payload")
            .with_details(json!({ "field": "name" }))
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be valid json");

        assert_eq!(value["error_code"], "BAD_REQUEST");
        assert_eq!(value["message"], "Invalid request payload");
        assert_eq!(value["details"]["field"], "name");
        assert!(value["timestamp"].is_string());
    }

    #[tokio::test]
    async fn rate_limited_errors_use_rate_limited_code() {
        let response = ApiError::rate_limited("Too many requests").into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("response body should be valid json");

        assert_eq!(value["error_code"], "RATE_LIMITED");
    }
}
