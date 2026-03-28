use crate::breaking_changes;
use crate::custom_metrics_handlers;
use crate::deprecation_handlers;
use crate::handlers;
use crate::metrics_handler;
use serde_json::Value;
use shared::models::*;
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health_check,
        handlers::get_stats,
        handlers::list_contracts,
        handlers::get_contracts_batch,
        handlers::get_contract,
        handlers::get_contract_versions,
        handlers::create_contract_version,
        handlers::publish_contract,
        handlers::create_publisher,
        handlers::get_publisher,
        handlers::get_publisher_contracts,
        handlers::get_contract_abi,
        handlers::get_contract_openapi_yaml,
        handlers::get_contract_openapi_json,
        handlers::get_contract_analytics,
        handlers::get_contract_dependencies,
        handlers::get_contract_dependents,
        handlers::get_contract_graph,
        handlers::get_impact_analysis,
        handlers::get_trending_contracts,
        handlers::verify_contract,
        handlers::update_contract_metadata,
        handlers::change_contract_publisher,
        handlers::update_contract_status,
        handlers::get_contract_audit_log,
        handlers::get_all_audit_logs,
        handlers::get_deployment_status,
        handlers::deploy_green,
        handlers::get_contract_performance,
        handlers::get_contract_interactions,
        handlers::post_contract_interaction,
        handlers::post_contract_interactions_batch,
        breaking_changes::get_breaking_changes,
        custom_metrics_handlers::get_metric_catalog,
        custom_metrics_handlers::get_contract_metrics,
        custom_metrics_handlers::record_contract_metric,
        custom_metrics_handlers::record_metrics_batch,
        deprecation_handlers::get_deprecation_info,
        deprecation_handlers::deprecate_contract,
        metrics_handler::metrics_endpoint,
    ),
    components(
        schemas(
            Contract,
            ContractGetResponse,
            NetworkConfig,
            Network,
            UpgradeStrategy,
            ContractVersion,
            Verification,
            VerificationStatus,
            MaturityLevel,
            Publisher,
            ContractStats,
            GraphNode,
            GraphEdge,
            GraphResponse,
            PublishRequest,
            MigrationScript,
            DeploymentEnvironment,
            CanaryRelease,
            ABTest,
            PerformanceMetric,
            CustomMetric,
            MetricAnomaly,
            AnalyticsReport,
            ContractAuditLogEntry,
            ContractInteraction,
            ArtifactType,
            ContractDependency,
            ImpactAnalysisResponse,
            VerificationRequest,
            VerifyRequest,
            ContractAnalyticsResponse,
            DeploymentStats,
            InteractorStats,
            TopUser,
            TimelineEntry,
            RecordCustomMetricRequest,
            CustomMetricAggregate,
            CustomMetricType,
            DeprecationInfo,
            DeprecationStatus,
            DeprecateContractRequest,
            ChangePublisherRequest,
            UpdateContractStatusRequest,
            UpdateContractMetadataRequest,
            InteractionsListResponse,
            ContractInteractionResponse,
            CreateInteractionRequest,
            CreateInteractionBatchRequest,
            breaking_changes::ChangeSeverity,
            breaking_changes::BreakingChange,
            breaking_changes::BreakingChangeReport,
            custom_metrics_handlers::MetricSeriesResponse,
            custom_metrics_handlers::MetricSeriesPoint,
            custom_metrics_handlers::MetricSampleResponse,
            custom_metrics_handlers::MetricSample,
            custom_metrics_handlers::MetricCatalogEntry,
        )
    ),
    tags(
        (name = "Observability", description = "Monitor API health and performance"),
        (name = "Contracts", description = "Everything about contracts"),
        (name = "Publishers", description = "Publisher management"),
        (name = "Artifacts", description = "Contract ABIs and OpenAPI specs"),
        (name = "Analytics", description = "Usage and performance metrics"),
        (name = "Analysis", description = "Contract ABI analysis and breaking changes"),
        (name = "Graphs", description = "Dependency graphs and impact analysis"),
        (name = "Verification", description = "Source code verification"),
        (name = "Metrics", description = "Custom application metrics"),
        (name = "Maintenance", description = "Deprecation and version management"),
        (name = "Administration", description = "Administrative audit logs"),
        (name = "Deployments", description = "Deployment management"),
    ),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearerAuth",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::HttpBuilder::new()
                        .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}
