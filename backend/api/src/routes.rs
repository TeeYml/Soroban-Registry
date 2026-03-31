#[cfg(feature = "openapi")]
use crate::openapi;
use crate::{
    ab_test_handlers, analytics_handlers, auth, auth_handlers, batch_verify_handlers,
    breaking_changes, canary_handlers, category_handlers, clone_federation_handlers,
    compatibility_testing_handlers, contract_events, custom_metrics_handlers,
    deprecation_handlers, handlers, interoperability_handlers, metrics_handler,
    migration_handlers, org_handlers, performance_handlers, resource_handlers,
    security_scan_handlers, similarity_handlers, simulation_handlers, state::AppState,
    subscription_handlers, websocket,
};


use axum::{
    middleware,
    routing::{delete, get, patch, post, put},
    Router,
};
#[cfg(feature = "openapi")]
use utoipa::OpenApi;
#[cfg(feature = "openapi")]
use utoipa_swagger_ui::SwaggerUi;

pub fn observability_routes() -> Router<AppState> {
    Router::new().route("/metrics", get(metrics_handler::metrics_endpoint))
}

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/challenge", get(auth_handlers::get_challenge))
        .route("/api/auth/verify", post(auth_handlers::verify_challenge))
}

pub fn contract_routes() -> Router<AppState> {
    Router::new()
        .route("/ws/contracts", get(contract_events::contracts_websocket))
        .route(
            "/api/contracts",
            get(handlers::list_contracts).post(handlers::publish_contract),
        )
        .route(
            "/api/contracts/tags",
            get(handlers::list_tags),
        )
        .route(
            "/api/contracts/export",
            post(handlers::export_contract_metadata),
        )
        .route(
            "/contracts/export",
            post(handlers::export_contract_metadata),
        )
        .route(
            "/api/contracts/export/:job_id",
            get(handlers::get_contract_export_status),
        )
        .route(
            "/contracts/export/:job_id",
            get(handlers::get_contract_export_status),
        )
        .route(
            "/api/contracts/export/:job_id/download",
            get(handlers::download_contract_export),
        )
        .route(
            "/contracts/export/:job_id/download",
            get(handlers::download_contract_export),
        )
        .route(
            "/api/contracts/suggestions",
            get(handlers::get_contract_search_suggestions),
        )
        .route(
            "/api/contracts/trending",
            get(handlers::get_trending_contracts),
        )
        .route("/api/contracts/batch", post(handlers::get_contracts_batch))
        .route("/contracts/batch", post(handlers::get_contracts_batch))
        .route("/api/contracts/graph", get(handlers::get_contract_graph))
        .route("/api/contracts/:id", get(handlers::get_contract))
        .route(
            "/api/contracts/:id/metadata",
            patch(handlers::update_contract_metadata),
        )
        .route(
            "/api/contracts/:id/publisher",
            patch(handlers::change_contract_publisher),
        )
        .route(
            "/api/contracts/:id/status",
            patch(handlers::update_contract_status),
        )
        .route(
            "/api/contracts/:id/audit-log",
            get(handlers::get_contract_audit_log),
        )
        .route("/api/contracts/:id/abi", get(handlers::get_contract_abi))
        .route(
            "/api/contracts/:id/openapi.yaml",
            get(handlers::get_contract_openapi_yaml),
        )
        .route(
            "/api/contracts/:id/openapi.json",
            get(handlers::get_contract_openapi_json),
        )
        .route(
            "/api/contracts/:id/versions",
            get(handlers::get_contract_versions).post(handlers::create_contract_version),
        )
        // Static segment "compare" must be registered before the dynamic ":version" route
        // so Axum resolves it correctly.
        .route(
            "/api/contracts/:id/versions/compare",
            get(handlers::compare_contract_versions),
        )
        .route(
            "/api/contracts/:id/versions/:version",
            get(handlers::get_specific_contract_version),
        )
        .route(
            "/api/contracts/:id/changelog",
            get(handlers::get_contract_changelog),
        )
        // Differential update pipeline (Issue #501)
        .route(
            "/api/contracts/:id/patches",
            get(patch_handlers::list_contract_patches),
        )
        .route(
            "/api/contracts/:id/patches/:from_version/:to_version",
            get(patch_handlers::get_patch_between_versions),
        )
        .route(
            "/api/contracts/:id/patches/reconstruct",
            post(patch_handlers::reconstruct_contract_version),
        )
        .route(
            "/api/contracts/patches/bulk-apply",
            post(patch_handlers::bulk_apply_patches),
        )
        .route(
            "/api/contracts/:id/versions/:version/source",
            get(handlers::get_contract_source).post(handlers::upload_contract_source),
        )
        .route(
            "/api/contracts/:id/versions/:version/source/diff",
            get(handlers::get_contract_source_diff),
        )
        .route(
            "/contracts/:id/changelog",
            get(handlers::get_contract_changelog),
        )
        .route(
            "/api/contracts/breaking-changes",
            get(breaking_changes::get_breaking_changes),
        )
        .route(
            "/api/contracts/:id/interactions",
            get(handlers::get_contract_interactions).post(handlers::post_contract_interaction),
        )
        .route(
            "/api/contracts/:id/interactions/batch",
            post(handlers::post_contract_interactions_batch),
        )
        .route(
            "/api/contracts/:id/deprecation-info",
            get(deprecation_handlers::get_deprecation_info),
        )
        .route(
            "/api/contracts/:id/deprecate",
            post(deprecation_handlers::deprecate_contract),
        )
        .route(
            "/api/contracts/:id/state/:key",
            get(handlers::get_contract_state)
                .put(handlers::update_contract_state)
                .post(handlers::update_contract_state),
        )
        .route(
            "/api/contracts/:id/analytics",
            get(analytics_handlers::get_contract_analytics),
        )
        .route(
            "/api/analytics/dashboard",
            get(analytics_handlers::get_analytics_summary),
        )

        .route(
            "/api/contracts/:id/dependencies",
            get(crate::dependency_handlers::get_contract_dependencies),
        )
        .route(
            "/api/contracts/:id/graph",
            get(handlers::get_contract_local_graph),
        )

        .route(
            "/api/contracts/:id/trust-score",
            get(handlers::get_trust_score),
        )
        .route(
            "/api/contracts/:id/dependents",
            get(handlers::get_contract_dependents),
        )
        .route(
            "/api/contracts/:id/impact",
            get(handlers::get_impact_analysis),
        )
        .route(
            "/api/contracts/:id/similar",
            get(similarity_handlers::get_similar_contracts),
        )
        .route(
            "/api/contracts/:id/recommendations",
            get(recommendation_handlers::get_contract_recommendations),
        )
        .route(
            "/contracts/:id/recommendations",
            get(recommendation_handlers::get_contract_recommendations),
        )
        .route(
            "/contracts/:id/similar",
            get(similarity_handlers::get_similar_contracts),
        )
        .route("/api/contracts/verify", post(handlers::verify_contract))
        .route(
            "/api/contracts/batch-verify",
            post(batch_verify_handlers::batch_verify_contracts),
        )
        .route(
            "/api/contracts/similarity/analyze",
            post(similarity_handlers::analyze_contract_similarity_batch),
        )
        .route(
            "/api/contracts/status/bulk",
            post(handlers::bulk_update_contract_status),
        )
        .route(
            "/api/contracts/:id/performance",
            get(performance_handlers::get_contract_performance_overview),
        )
        .route(
            "/api/contracts/:id/metrics",
            get(custom_metrics_handlers::get_contract_metrics)
                .post(custom_metrics_handlers::record_contract_metric),
        )
        .route(
            "/api/contracts/:id/resources",
            get(resource_handlers::get_contract_resources),
        )
        .route(
            "/api/contracts/:id/metrics/batch",
            post(custom_metrics_handlers::record_metrics_batch),
        )
        .route(
            "/api/contracts/:id/metrics/catalog",
            get(custom_metrics_handlers::get_metric_catalog),
        )
        .route(
            "/api/contracts/:id/compatibility",
            get(interoperability_handlers::get_contract_interoperability),
        )
        .route(
            "/api/contracts/:id/compatibility-matrix",
            get(compatibility_testing_handlers::get_compatibility_matrix),
        )
        .route(
            "/api/contracts/:id/compatibility-matrix/test",
            post(compatibility_testing_handlers::run_compatibility_test),
        )
        .route(
            "/api/contracts/:id/compatibility-matrix/history",
            get(compatibility_testing_handlers::get_compatibility_history),
        )
        .route(
            "/api/contracts/:id/compatibility-matrix/notifications",
            get(compatibility_testing_handlers::get_compatibility_notifications),
        )
        .route(
            "/api/contracts/:id/compatibility-matrix/notifications/read",
            post(compatibility_testing_handlers::mark_notifications_read),
        )
        .route(
            "/api/contracts/:id/deployments",
            get(handlers::get_contract_deployments),
        )
        .route(
            "/api/contracts/:id/deployments/status",
            get(handlers::get_deployment_status),
        )
        .route(
            "/api/contracts/:id/deployment-status",
            get(handlers::get_deployment_status),
        )
        .route("/api/deployments/green", post(handlers::deploy_green))
        .route(
            "/api/contracts/:id/deploy-green",
            post(handlers::deploy_green),
        )
        .route(
            "/api/contracts/simulate-deploy",
            post(simulation_handlers::simulate_deploy),
        )
        // Gas usage estimation (Issue #496)
        // Static segment "gas-estimate/batch" registered before dynamic ":method"
        .route(
            "/api/contracts/:id/methods/gas-estimate/batch",
            post(gas_estimation_handlers::batch_gas_estimate),
        )
        .route(
            "/api/contracts/:id/methods/:method/gas-estimate",
            get(gas_estimation_handlers::get_method_gas_estimate),
        )
        // Review system endpoints
        .route(
            "/api/contracts/:id/reviews",
            get(handlers::reviews::get_reviews).post(handlers::reviews::create_review),
        )
        .route(
            "/api/contracts/:id/reviews/:review_id/vote",
            post(handlers::reviews::vote_review),
        )
        .route(
            "/api/contracts/:id/reviews/:review_id/flag",
            post(handlers::reviews::flag_review),
        )
        .route(
            "/api/contracts/:id/reviews/:review_id/moderate",
            post(handlers::reviews::moderate_review),
        )
        .route(
            "/api/contracts/:id/rating-stats",
            get(handlers::reviews::get_rating_stats),
        )
        // Contract clone endpoints (#487)
        .route(
            "/api/contracts/:id/clone",
            post(clone_federation_handlers::clone_contract),
        )
        .route(
            "/api/contracts/:id/clones",
            get(clone_federation_handlers::get_contract_clones),
        )
        .merge(favorite_routes())
}

pub fn organization_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/organizations",
            post(org_handlers::create_organization),
        )
        .route(
            "/api/organizations/:id",
            get(org_handlers::get_organization).patch(org_handlers::update_organization),
        )
        .route(
            "/api/organizations/:id/members",
            get(org_handlers::list_org_members),
        )
        .route(
            "/api/organizations/:id/invitations",
            post(org_handlers::invite_member),
        )
        .route(
            "/api/organizations/invitations/:token/accept",
            post(org_handlers::accept_invitation),
        )
}

#[cfg(not(feature = "openapi"))]
pub fn openapi_routes() -> Router<AppState> {
    Router::new()
}

#[cfg(feature = "openapi")]
pub fn openapi_routes() -> Router<AppState> {
    Router::new().merge(SwaggerUi::new("/docs").url("/openapi.json", openapi::ApiDoc::openapi()))
}

pub fn publisher_routes() -> Router<AppState> {
    Router::new()
        .route("/api/publishers", post(handlers::create_publisher))
        .route("/api/publishers/:id", get(handlers::get_publisher))
        .route(
            "/api/publishers/:id/contracts",
            get(handlers::get_publisher_contracts),
        )
}

pub fn contributor_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/contributors",
            get(contributor_handlers::list_contributors)
                .post(contributor_handlers::create_contributor),
        )
        .route(
            "/api/contributors/:id",
            get(contributor_handlers::get_contributor)
                .put(contributor_handlers::update_contributor),
        )
        .route(
            "/api/contributors/:id/contracts",
            get(contributor_handlers::get_contributor_contracts),
        )
}

pub fn favorite_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/favorites/search",
            get(handlers::list_favorite_searches).post(handlers::save_favorite_search),
        )
        .route(
            "/api/favorites/search/:id",
            delete(handlers::delete_favorite_search),
        )
}

pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(handlers::health_check))
        .route("/health/live", get(handlers::health_check_live))
        .route("/health/ready", get(handlers::health_check_ready))
        .route("/health/detailed", get(handlers::health_check_detailed))
        .route("/api/stats", get(handlers::get_stats))
        // Registry-wide analytics summary (issue #415)
        .route(
            "/api/analytics/summary",
            get(analytics_handlers::get_analytics_summary),
        )
}

pub fn category_routes() -> Router<AppState> {
    Router::new()
        .route("/api/categories", get(category_handlers::list_categories))
        .route("/api/categories/:id", get(category_handlers::get_category))
}

pub fn network_routes() -> Router<AppState> {
    Router::new()
        .route("/networks", get(handlers::list_networks))
        .route("/api/networks", get(handlers::list_networks))
}

pub fn health_monitor_routes() -> Router<AppState> {
    Router::new().route(
        "/api/health-monitor/status",
        get(crate::health_monitor::get_health_monitor_status),
    )
}

pub fn migration_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/migrations/status",
            get(migration_handlers::get_migration_status),
        )
        .route(
            "/api/admin/migrations/register",
            post(migration_handlers::register_migration),
        )
        .route(
            "/api/admin/migrations/validate",
            get(migration_handlers::validate_migrations),
        )
        .route(
            "/api/admin/migrations/lock",
            get(migration_handlers::get_lock_status),
        )
        .route(
            "/api/admin/migrations/:version",
            get(migration_handlers::get_migration_version),
        )
        .route(
            "/api/admin/migrations/:version/rollback",
            post(migration_handlers::rollback_migration),
        )
}

pub fn compatibility_dashboard_routes() -> Router<AppState> {
    Router::new().route(
        "/api/compatibility-dashboard",
        get(compatibility_testing_handlers::get_compatibility_dashboard),
    )
}

pub fn category_routes() -> Router<AppState> {
    Router::new().route("/api/categories", get(category_handlers::list_categories))
}

pub fn canary_routes() -> Router<AppState> {
    Router::new()
        // Contract-scoped canary endpoints
        .route(
            "/api/contracts/:id/canary",
            get(canary_handlers::list_canaries).post(canary_handlers::create_canary),
        )
        // Canary-specific endpoints
        .route("/api/canary/:canary_id", get(canary_handlers::get_canary))
        .route(
            "/api/canary/:canary_id/advance",
            post(canary_handlers::advance_canary),
        )
        .route(
            "/api/canary/:canary_id/rollback",
            post(canary_handlers::rollback_canary),
        )
        .route(
            "/api/canary/:canary_id/complete",
            post(canary_handlers::complete_canary),
        )
        .route(
            "/api/canary/:canary_id/metrics",
            get(canary_handlers::list_canary_metrics).post(canary_handlers::record_canary_metric),
        )
}

pub fn ab_test_routes() -> Router<AppState> {
    Router::new()
        // Contract-scoped A/B test endpoints
        .route(
            "/api/contracts/:id/ab-tests",
            get(ab_test_handlers::list_ab_tests).post(ab_test_handlers::create_ab_test),
        )
        // A/B test-specific endpoints
        .route("/api/ab-tests/:test_id", get(ab_test_handlers::get_ab_test))
        .route(
            "/api/ab-tests/:test_id/start",
            post(ab_test_handlers::start_ab_test),
        )
        .route(
            "/api/ab-tests/:test_id/stop",
            post(ab_test_handlers::stop_ab_test),
        )
        .route(
            "/api/ab-tests/:test_id/cancel",
            post(ab_test_handlers::cancel_ab_test),
        )
        .route(
            "/api/ab-tests/:test_id/metrics",
            post(ab_test_handlers::record_ab_test_metric),
        )
        .route(
            "/api/ab-tests/:test_id/results",
            get(ab_test_handlers::get_ab_test_results),
        )
}

pub fn performance_routes() -> Router<AppState> {
    Router::new()
        // Contract-scoped performance endpoints
        .route(
            "/api/contracts/:id/perf/benchmarks",
            get(performance_handlers::list_benchmarks).post(performance_handlers::record_benchmark),
        )
        .route(
            "/api/contracts/:id/perf/metrics",
            get(performance_handlers::list_metrics).post(performance_handlers::record_metric),
        )
        .route(
            "/api/contracts/:id/perf/comparison",
            get(performance_handlers::get_performance_comparison),
        )
        .route(
            "/api/contracts/:id/perf/anomalies",
            get(performance_handlers::list_anomalies),
        )
        .route(
            "/api/contracts/:id/perf/alerts",
            get(performance_handlers::list_alerts),
        )
        .route(
            "/api/contracts/:id/perf/alert-configs",
            get(performance_handlers::list_alert_configs)
                .post(performance_handlers::create_alert_config),
        )
        .route(
            "/api/contracts/:id/perf/trends",
            get(performance_handlers::list_trends),
        )
        .route(
            "/api/contracts/:id/perf/summary",
            get(performance_handlers::get_performance_summary),
        )
        // Alert-specific action endpoints
        .route(
            "/api/perf/alerts/:alert_id/acknowledge",
            post(performance_handlers::acknowledge_alert),
        )
        .route(
            "/api/perf/alerts/:alert_id/resolve",
            post(performance_handlers::resolve_alert),
        )
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/audit-logs", get(handlers::get_all_audit_logs))
        .merge(migration_routes())
        // Category management (issue #414) – admin-only write endpoints
        .route(
            "/api/admin/categories",
            post(category_handlers::create_category),
        )
        .route(
            "/api/admin/categories/:id",
            put(category_handlers::update_category).delete(category_handlers::delete_category),
        )
        // Version revert (issue #486) – admin-only
        .route(
            "/api/admin/contracts/:id/versions/:version/revert",
            post(handlers::revert_contract_version),
        )
        .route_layer(middleware::from_fn(auth::require_admin))
}

pub fn federation_routes() -> Router<AppState> {
    Router::new()
        // Federated registry management (#499)
        .route(
            "/api/federation/registries",
            get(clone_federation_handlers::list_federated_registries)
                .post(clone_federation_handlers::register_federated_registry),
        )
        .route(
            "/api/federation/registries/:id",
            get(clone_federation_handlers::get_federated_registry),
        )
        // Sync operations
        .route(
            "/api/federation/sync",
            post(clone_federation_handlers::sync_from_federated_registry),
        )
        .route(
            "/api/federation/sync/:job_id",
            get(clone_federation_handlers::get_sync_job_status),
        )
        .route(
            "/api/federation/sync-history",
            get(clone_federation_handlers::get_federation_sync_history),
        )
        // Discovery
        .route(
            "/api/federation/discover",
            get(clone_federation_handlers::discover_federated_registries),
        )
        // Configuration
        .route(
            "/api/federation/config",
            get(clone_federation_handlers::get_federation_config),
        )
        // Contract federation attribution
        .route(
            "/api/contracts/:id/federation",
            get(clone_federation_handlers::get_contract_federation_attribution)
                .patch(clone_federation_handlers::update_contract_federation_settings),
        )
}

pub fn websocket_routes() -> Router<AppState> {
    Router::new().route(
        "/ws/contracts",
        axum::routing::get(websocket::websocket_handler),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// SECURITY SCANNING ROUTES (#498)
// ═══════════════════════════════════════════════════════════════════════════

pub fn security_scanning_routes() -> Router<AppState> {
    Router::new()
        // Security scanner management
        .route(
            "/api/security/scanners",
            get(security_scan_handlers::list_security_scanners)
                .post(security_scan_handlers::create_security_scanner),
        )
        // Contract security endpoints
        .route(
            "/api/contracts/:id/scans",
            get(security_scan_handlers::list_security_scans)
                .post(security_scan_handlers::trigger_security_scan),
        )
        .route(
            "/api/contracts/:id/scans/:scan_id",
            get(security_scan_handlers::get_security_scan),
        )
        .route(
            "/api/contracts/:id/security",
            get(security_scan_handlers::get_contract_security_summary),
        )
        .route(
            "/api/contracts/:id/security/score-history",
            get(security_scan_handlers::get_security_score_history),
        )
        .route(
            "/api/contracts/:id/issues",
            get(security_scan_handlers::list_security_issues),
        )
        .route(
            "/api/contracts/:id/issues/:issue_id",
            patch(security_scan_handlers::update_security_issue),
        )
}

// ═══════════════════════════════════════════════════════════════════════════
// SUBSCRIPTION & NOTIFICATION ROUTES (#493)
// ═══════════════════════════════════════════════════════════════════════════

pub fn subscription_routes() -> Router<AppState> {
    Router::new()
        // User subscriptions
        .route(
            "/api/me/subscriptions",
            get(subscription_handlers::list_user_subscriptions),
        )
        .route(
            "/api/contracts/:id/subscribe",
            post(subscription_handlers::subscribe_to_contract)
                .delete(subscription_handlers::unsubscribe_from_contract),
        )
        .route(
            "/api/subscriptions/:id",
            patch(subscription_handlers::update_subscription),
        )
        // Notification preferences
        .route(
            "/api/notifications/preferences",
            get(subscription_handlers::get_notification_preferences)
                .patch(subscription_handlers::update_notification_preferences),
        )
        // Notifications
        .route(
            "/api/notifications",
            get(subscription_handlers::list_notifications),
        )
        .route(
            "/api/notifications/:id/read",
            post(subscription_handlers::mark_notification_read),
        )
        .route(
            "/api/notifications/read-all",
            post(subscription_handlers::mark_all_notifications_read),
        )
        .route(
            "/api/notifications/statistics",
            get(subscription_handlers::get_notification_statistics),
        )
        // Webhooks
        .route(
            "/api/webhooks",
            get(subscription_handlers::list_webhooks)
                .post(subscription_handlers::create_webhook),
        )
        .route(
            "/api/webhooks/:id",
            delete(subscription_handlers::delete_webhook),
        )
}
