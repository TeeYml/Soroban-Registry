use axum::{
    routing::{get, post, put},
    Router,
};

use crate::{handlers, state::AppState};

pub fn contract_routes() -> Router<AppState> {
    Router::new()
        .route("/api/contracts", get(handlers::list_contracts))
        .route("/api/contracts", post(handlers::publish_contract))
        .route("/api/contracts/:id", get(handlers::get_contract))
        .route("/api/contracts/:id/abi", get(handlers::get_contract_abi))
        .route("/api/contracts/:id/versions", get(handlers::get_contract_versions))
        .route(
            "/api/contracts/:id/versions",
            get(handlers::get_contract_versions),
        )
        .route(
            "/api/contracts/:id/analytics",
            get(handlers::get_contract_analytics),
        )
        .route(
            "/api/contracts/:id/dependencies",
            get(crate::dependency_handlers::get_contract_dependencies),
        )
        .route("/api/contracts/verify", post(handlers::verify_contract))
        .route("/api/contracts/:id/deployments/status", get(handlers::get_deployment_status))
        .route("/api/deployments/green", post(handlers::deploy_green))
        .route("/api/deployments/switch", post(handlers::switch_deployment))
        .route("/api/deployments/:contract_id/rollback", post(handlers::rollback_deployment))
        .route("/api/deployments/health", post(handlers::report_health_check))
        .route("/api/contracts/:id/state/:key", get(handlers::get_contract_state).post(handlers::update_contract_state))
        .route("/api/contracts/:id/performance", get(handlers::get_contract_performance))
}

/// Publisher-related routes
pub fn publisher_routes() -> Router<AppState> {
    Router::new()
        .route("/api/publishers", post(handlers::create_publisher))
        .route("/api/publishers/:id", get(handlers::get_publisher))
        .route(
            "/api/publishers/:id/contracts",
            get(handlers::get_publisher_contracts),
        )
}

/// Health check routes
pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(handlers::health_check))
        .route("/api/stats", get(handlers::get_stats))
        .route("/api/cache/stats", get(handlers::get_cache_stats))
}

/// Migration-related routes
pub fn migration_routes() -> Router<AppState> {
    Router::new()
        .route("/api/migrations", post(handlers::migrations::create_migration).get(handlers::migrations::get_migrations))
        .route("/api/migrations/:id", put(handlers::migrations::update_migration).get(handlers::migrations::get_migration))
}

pub fn canary_routes() -> Router<AppState> {
    Router::new()
        .route("/api/canaries", post(handlers::create_canary))
        .route("/api/canaries/:id", get(handlers::get_canary_status))
        .route("/api/canaries/:id/advance", post(handlers::advance_canary))
        .route("/api/canaries/:id/rollback", post(handlers::rollback_canary))
        .route("/api/canaries/:id/metrics", post(handlers::record_canary_metric))
        .route("/api/canaries/:id/users", post(handlers::assign_canary_users))
}

pub fn ab_test_routes() -> Router<AppState> {
    Router::new()
        .route("/api/ab-tests", post(handlers::create_ab_test))
        .route("/api/ab-tests/:id/start", post(handlers::start_ab_test))
        .route("/api/ab-tests/variant", post(handlers::get_variant))
        .route("/api/ab-tests/metrics", post(handlers::record_ab_test_metric))
        .route("/api/ab-tests/:id/results", get(handlers::get_ab_test_results))
        .route("/api/ab-tests/:id/rollout", post(handlers::rollout_winning_variant))
}

pub fn performance_routes() -> Router<AppState> {
    Router::new()
        .route("/api/performance/metrics", post(handlers::record_performance_metric))
        .route("/api/performance/alerts/config", post(handlers::create_alert_config))
        .route("/api/performance/anomalies/:contract_id", get(handlers::get_performance_anomalies))
        .route("/api/performance/alerts/:id/acknowledge", post(handlers::acknowledge_alert))
}
