//! Enhanced request extraction with security logging and rate limiting
//!
//! This module provides middleware and utilities to integrate validation failure
//! logging and rate limiting into the request/response pipeline.

use axum::{
    extract::{connect_info::ConnectInfo, MatchedPath},
    http::Request,
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

use crate::security_log;

/// Middleware that tracks validation failures for rate limiting
///
/// Add this middleware to your router to enable validation failure rate limiting:
///
/// ```ignore
/// let app = Router::new()
///     // ... routes ...
///     .layer(middleware::from_fn(request_tracing::tracing_middleware))
///     .layer(middleware::from_fn(validation_failure_tracking_middleware))
/// ```
pub async fn validation_failure_tracking_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    matched_path: Option<MatchedPath>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let correlation_id = crate::request_tracing::get_or_create_request_id(&req);

    let response = next.run(req).await;

    // Log validation failures (400 responses)
    if response.status() == axum::http::StatusCode::BAD_REQUEST {
        let client_ip = addr.ip();
        let path = matched_path
            .as_ref()
            .map(|p| p.as_str())
            .unwrap_or("unknown");

        security_log::log_validation_failure(
            client_ip,
            "request",
            "Request validation failed",
            path,
            "POST",
            &correlation_id,
            1,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_correlation_id_generation() {
        let id = crate::request_tracing::generate_request_id();
        assert!(!id.is_empty());
        assert_eq!(id.len(), 36); // UUID v4 format
    }
}
