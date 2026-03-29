//! Structured request tracing middleware.
//!
//! Every incoming HTTP request gets a unique UUID (`X-Request-ID`), and a
//! JSON-structured log line is emitted after the response is sent.
//!
//! Health-check endpoints are intentionally skipped so they don't pollute
//! the log stream.
//!
//! Log fields:
//!   timestamp, request_id, method, path, status, duration_ms, user_ip

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, HeaderName, HeaderValue, Request},
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

/// Paths that should never be logged (health checks, readiness probes, etc.)
const SKIP_LOG_PATHS: &[&str] = &["/health", "/healthz", "/ready", "/ping", "/metrics"];

/// The response header name carrying the request ID back to the caller.
pub static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
pub static X_CORRELATION_ID: HeaderName = HeaderName::from_static("x-correlation-id");

tokio::task_local! {
    static CURRENT_REQUEST_ID: String;
}

/// Axum middleware: attach a request ID, log the completed request as JSON,
/// and add the `X-Request-ID` header to the response.
pub async fn tracing_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let request_id = request_id_from_headers(req.headers()).unwrap_or_else(generate_request_id);
    let method = req.method().to_string();
    let path = req.uri().path().to_owned();
    let user_ip = addr.ip().to_string();

    // Inject the request ID into extensions so handlers / DB layers can read it
    req.extensions_mut().insert(RequestId(request_id.clone()));

    let start = Instant::now();
    let span = tracing::info_span!(
        "http_request",
        request_id = %request_id,
        method = %method,
        path = %path,
        user_ip = %user_ip
    );
    let mut response = CURRENT_REQUEST_ID
        .scope(request_id.clone(), next.run(req).instrument(span.clone()))
        .await;
    let duration_ms = start.elapsed().as_millis() as u64;

    attach_request_id_headers(response.headers_mut(), &request_id);

    // Skip noisy health-check paths
    if SKIP_LOG_PATHS.iter().any(|p| path.starts_with(p)) {
        return response;
    }

    let status = response.status().as_u16();

    // Emit a single structured JSON log line per request
    tracing::info!(
        request_id = %request_id,
        method     = %method,
        path       = %path,
        status     = status,
        duration_ms = duration_ms,
        user_ip    = %user_ip,
        "request"
    );

    response
}

pub fn generate_request_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn current_request_id() -> Option<String> {
    CURRENT_REQUEST_ID
        .try_with(|request_id| request_id.clone())
        .ok()
}

pub fn request_id_from_headers(headers: &HeaderMap) -> Option<String> {
    [X_REQUEST_ID.as_str(), X_CORRELATION_ID.as_str()]
        .iter()
        .find_map(|header_name| {
            headers
                .get(*header_name)
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

pub fn request_id_from_request<B>(request: &Request<B>) -> Option<String> {
    request
        .extensions()
        .get::<RequestId>()
        .map(|request_id| request_id.0.clone())
        .or_else(|| request_id_from_headers(request.headers()))
}

pub fn get_or_create_request_id<B>(request: &Request<B>) -> String {
    request_id_from_request(request)
        .or_else(current_request_id)
        .unwrap_or_else(generate_request_id)
}

pub fn attach_request_id_headers(headers: &mut HeaderMap, request_id: &str) {
    if let Ok(value) = HeaderValue::from_str(request_id) {
        headers.insert(X_REQUEST_ID.clone(), value.clone());
        headers.insert(X_CORRELATION_ID.clone(), value);
    }
}

// ── Request ID extractor ──────────────────────────────────────────────────────

/// A newtype wrapper stored in request extensions so downstream code can
/// cheaply retrieve the current request ID without re-parsing headers.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

impl RequestId {
    /// Retrieve the request ID from Axum request extensions, if present.
    pub fn from_request(req: &Request<Body>) -> Option<&str> {
        req.extensions().get::<RequestId>().map(|r| r.0.as_str())
    }
}

// ── JSON tracing subscriber initialiser ──────────────────────────────────────

/// Initialise `tracing-subscriber` with a JSON formatter suitable for
/// ELK / Splunk / Datadog ingestion.
///
/// Call this **once** at application startup, replacing the plain-text
/// subscriber currently set up in `main.rs`.
///
/// Log rotation (daily, 7-day retention) is handled by the deployment
/// environment (e.g. logrotate, Docker log driver, or a dedicated log
/// shipper). The subscriber itself writes to stdout so the runtime can
/// redirect / rotate as needed.
pub fn init_json_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "api=info,tower_http=info".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_current_span(true),
        )
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        response::IntoResponse,
        routing::get,
        Json, Router,
    };
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    async fn ok_handler(req: Request<Body>) -> impl IntoResponse {
        let request_id = request_id_from_request(&req).unwrap_or_default();
        Json(serde_json::json!({ "request_id": request_id }))
    }

    async fn error_handler() -> crate::error::ApiResult<Json<serde_json::Value>> {
        Err(crate::error::ApiError::bad_request(
            "BadRequest",
            "broken request",
        ))
    }

    fn app() -> Router {
        Router::new()
            .route("/ok", get(ok_handler))
            .route("/error", get(error_handler))
            .layer(middleware::from_fn(tracing_middleware))
    }

    async fn call(app: Router, request: Request<Body>) -> axum::response::Response {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 4000);
        let mut request = request;
        request.extensions_mut().insert(ConnectInfo(addr));
        app.oneshot(request).await.unwrap()
    }

    #[tokio::test]
    async fn preserves_incoming_request_id_header() {
        let response = call(
            app(),
            Request::builder()
                .uri("/ok")
                .method("GET")
                .header(X_REQUEST_ID.as_str(), "req-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(X_REQUEST_ID.as_str())
                .and_then(|v| v.to_str().ok()),
            Some("req-123")
        );
    }

    #[tokio::test]
    async fn generates_request_id_when_missing() {
        let response = call(
            app(),
            Request::builder()
                .uri("/ok")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await;

        let request_id = response
            .headers()
            .get(X_REQUEST_ID.as_str())
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();

        assert_eq!(request_id.len(), 36);
        assert_eq!(
            response
                .headers()
                .get(X_CORRELATION_ID.as_str())
                .and_then(|v| v.to_str().ok()),
            Some(request_id.as_str())
        );
    }

    #[tokio::test]
    async fn error_responses_reuse_request_id() {
        let response = call(
            app(),
            Request::builder()
                .uri("/error")
                .method("GET")
                .header(X_REQUEST_ID.as_str(), "req-error-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response
                .headers()
                .get(X_REQUEST_ID.as_str())
                .and_then(|v| v.to_str().ok()),
            Some("req-error-1")
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should read");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(json["correlation_id"], "req-error-1");
    }
}
