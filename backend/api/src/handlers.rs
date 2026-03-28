use crate::validation::extractors::ValidatedJson;
use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::{json, Value};
use sqlx::QueryBuilder;
use shared::{
    pagination::Cursor, AnalyticsEventType, AuditActionType, ChangePublisherRequest, Contract,
    ContractAnalyticsResponse, ContractAuditLog, ContractChangelogEntry, ContractChangelogResponse,
    ContractGetResponse, ContractInteractionResponse, ContractSearchParams, ContractVersion,
    CreateContractVersionRequest, CreateInteractionBatchRequest, CreateInteractionRequest,
    DeploymentStats, InteractionTimeSeriesPoint, InteractionTimeSeriesResponse,
    InteractionsListResponse, InteractionsQueryParams, InteractorStats, Network, NetworkConfig,
    NetworkEndpoints, NetworkInfo, NetworkListResponse, NetworkStatus, PaginatedResponse,
    PublishRequest, Publisher, SearchSuggestion, SearchSuggestionsResponse, SemVer, TimelineEntry,
    TopUser, TrendingParams,
    UpdateContractMetadataRequest, UpdateContractStatusRequest, VerifyRequest,
};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use uuid::Uuid;

/// Query params for GET /contracts/:id (Issue #43)
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct GetContractQuery {
    pub network: Option<Network>,
    pub from_search: Option<bool>,
    pub search_query: Option<String>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct BatchContractsQuery {
    /// Comma-separated list of fields to include in each contract result.
    /// Example: fields=name,address,network
    pub fields: Option<String>,
}

use crate::{
    analytics,
    breaking_changes::{diff_abi, has_breaking_changes, resolve_abi},
    contract_events::{ContractEventEnvelope, ContractEventVisibility},
    dependency,
    error::{ApiError, ApiResult},
    state::AppState,
    type_safety::parser::parse_json_spec,
    type_safety::{generate_openapi, to_json, to_yaml},
};

pub(crate) fn db_internal_error(operation: &str, err: sqlx::Error) -> ApiError {
    tracing::error!(operation = operation, error = ?err, "database operation failed");
    ApiError::internal("An unexpected database error occurred")
}

#[allow(dead_code)]
fn map_json_rejection(err: JsonRejection) -> ApiError {
    ApiError::bad_request(
        "InvalidRequest",
        format!("Invalid JSON payload: {}", err.body_text()),
    )
}

fn map_query_rejection(err: QueryRejection) -> ApiError {
    ApiError::bad_request(
        "InvalidQuery",
        format!("Invalid query parameters: {}", err.body_text()),
    )
}

fn sort_timestamp_column(sort_by: &shared::SortBy) -> Option<&'static str> {
    match sort_by {
        shared::SortBy::CreatedAt => Some("c.created_at"),
        shared::SortBy::UpdatedAt => Some("c.updated_at"),
        shared::SortBy::VerifiedAt => Some("c.verified_at"),
        shared::SortBy::LastAccessedAt => Some("c.last_accessed_at"),
        _ => None,
    }
}

fn contract_timestamp_for_sort(contract: &Contract, sort_by: &shared::SortBy) -> Option<chrono::DateTime<chrono::Utc>> {
    match sort_by {
        shared::SortBy::CreatedAt => Some(contract.created_at),
        shared::SortBy::UpdatedAt => Some(contract.updated_at),
        shared::SortBy::VerifiedAt => contract.verified_at,
        shared::SortBy::LastAccessedAt => contract.last_accessed_at,
        _ => None,
    }
}

async fn track_contract_access(state: &AppState, contract_id: Uuid) {
    let cache_key = contract_id.to_string();
    if !state.cache.should_refresh_contract_access(&cache_key).await {
        return;
    }

    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(err) = sqlx::query("UPDATE contracts SET last_accessed_at = NOW() WHERE id = $1")
            .bind(contract_id)
            .execute(&db)
            .await
        {
            tracing::warn!(contract_id = %contract_id, error = ?err, "failed to refresh contract last_accessed_at");
        }
    });
}

const NETWORKS_CACHE_NAMESPACE: &str = "system";
const NETWORKS_CACHE_KEY: &str = "network_catalog";
const NETWORKS_REFRESH_INTERVAL_SECS: u64 = 60;
const NETWORK_DEGRADED_FAILURE_THRESHOLD: i32 = 1;
const NETWORK_OFFLINE_FAILURE_THRESHOLD: i32 = 5;
const NETWORK_STALE_AFTER_MINUTES: i64 = 10;
const SLOW_SEARCH_QUERY_THRESHOLD_MS: u128 = 200;

fn observe_search_query(
    kind: &str,
    started_at: std::time::Instant,
    query: Option<&str>,
    limit: i64,
) {
    let elapsed = started_at.elapsed();
    crate::metrics::SEARCH_QUERY_DURATION
        .with_label_values(&[kind])
        .observe(elapsed.as_secs_f64());

    if elapsed.as_millis() > SLOW_SEARCH_QUERY_THRESHOLD_MS {
        crate::metrics::SEARCH_SLOW_QUERIES
            .with_label_values(&[kind])
            .inc();
        tracing::warn!(
            query_type = kind,
            duration_ms = elapsed.as_millis(),
            query = query.unwrap_or(""),
            limit = limit,
            "slow search query detected"
        );
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct IndexerStateSnapshot {
    last_indexed_ledger_height: i64,
    indexed_at: chrono::DateTime<chrono::Utc>,
    consecutive_failures: i32,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct StaticNetworkDefinition {
    id: &'static str,
    name: &'static str,
    network_type: Network,
    rpc_url: String,
    explorer_url: String,
    friendbot_url: Option<String>,
}

fn default_rpc_url(network: &Network) -> &'static str {
    match network {
        Network::Mainnet => "https://rpc-mainnet.stellar.org",
        Network::Testnet => "https://rpc-testnet.stellar.org",
        Network::Futurenet => "https://rpc-futurenet.stellar.org",
    }
}

fn default_explorer_url(network: &Network) -> &'static str {
    match network {
        Network::Mainnet => "https://stellar.expert/explorer/public",
        Network::Testnet => "https://stellar.expert/explorer/testnet",
        Network::Futurenet => "https://stellar.expert/explorer/futurenet",
    }
}

fn default_friendbot_url(network: &Network) -> Option<&'static str> {
    match network {
        Network::Mainnet => None,
        Network::Testnet => Some("https://friendbot.stellar.org"),
        Network::Futurenet => Some("https://friendbot-futurenet.stellar.org"),
    }
}

fn configured_networks() -> Vec<StaticNetworkDefinition> {
    let entries = [
        (
            "mainnet",
            "Stellar Mainnet",
            Network::Mainnet,
            "STELLAR_RPC_MAINNET",
            "STELLAR_EXPLORER_MAINNET",
            "STELLAR_FRIENDBOT_MAINNET",
        ),
        (
            "testnet",
            "Stellar Testnet",
            Network::Testnet,
            "STELLAR_RPC_TESTNET",
            "STELLAR_EXPLORER_TESTNET",
            "STELLAR_FRIENDBOT_TESTNET",
        ),
        (
            "futurenet",
            "Stellar Futurenet",
            Network::Futurenet,
            "STELLAR_RPC_FUTURENET",
            "STELLAR_EXPLORER_FUTURENET",
            "STELLAR_FRIENDBOT_FUTURENET",
        ),
    ];

    entries
        .into_iter()
        .map(
            |(id, name, network_type, rpc_env, explorer_env, friendbot_env)| StaticNetworkDefinition {
                id,
                name,
                rpc_url: std::env::var(rpc_env)
                    .unwrap_or_else(|_| default_rpc_url(&network_type).to_string()),
                explorer_url: std::env::var(explorer_env)
                    .unwrap_or_else(|_| default_explorer_url(&network_type).to_string()),
                friendbot_url: std::env::var(friendbot_env)
                    .ok()
                    .or_else(|| default_friendbot_url(&network_type).map(str::to_string)),
                network_type,
            },
        )
        .collect()
}

fn derive_network_status(
    rpc_healthy: bool,
    snapshot: Option<&IndexerStateSnapshot>,
    now: chrono::DateTime<chrono::Utc>,
) -> (NetworkStatus, Option<String>) {
    if !rpc_healthy {
        return (
            NetworkStatus::Offline,
            Some("RPC health check failed".to_string()),
        );
    }

    if let Some(snapshot) = snapshot {
        let stale = now - snapshot.indexed_at > chrono::Duration::minutes(NETWORK_STALE_AFTER_MINUTES);

        if snapshot.consecutive_failures >= NETWORK_OFFLINE_FAILURE_THRESHOLD {
            return (
                NetworkStatus::Offline,
                snapshot
                    .error_message
                    .clone()
                    .or_else(|| Some(format!("{} consecutive indexer failures", snapshot.consecutive_failures))),
            );
        }

        if stale || snapshot.consecutive_failures >= NETWORK_DEGRADED_FAILURE_THRESHOLD {
            return (
                NetworkStatus::Degraded,
                if stale {
                    Some("Indexer status is stale".to_string())
                } else {
                    snapshot
                        .error_message
                        .clone()
                        .or_else(|| Some(format!("{} consecutive indexer failures", snapshot.consecutive_failures)))
                },
            );
        }
    }

    (NetworkStatus::Online, None)
}

async fn probe_network_health(client: &reqwest::Client, health_url: &str) -> bool {
    match client.get(health_url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

async fn fetch_network_catalog(db: &sqlx::PgPool) -> Result<NetworkListResponse, sqlx::Error> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let now = chrono::Utc::now();

    let mut networks = Vec::new();
    for definition in configured_networks() {
        let health_url = format!("{}/health", definition.rpc_url.trim_end_matches('/'));
        let snapshot: Option<IndexerStateSnapshot> = sqlx::query_as(
            "SELECT last_indexed_ledger_height, indexed_at, consecutive_failures, error_message
             FROM indexer_state
             WHERE network = $1",
        )
        .bind(&definition.network_type)
        .fetch_optional(db)
        .await?;

        let rpc_healthy = probe_network_health(&client, &health_url).await;
        let (status, status_message) = derive_network_status(rpc_healthy, snapshot.as_ref(), now);

        networks.push(NetworkInfo {
            id: definition.id.to_string(),
            name: definition.name.to_string(),
            network_type: definition.network_type,
            status,
            endpoints: NetworkEndpoints {
                rpc_url: definition.rpc_url,
                health_url,
                explorer_url: definition.explorer_url,
                friendbot_url: definition.friendbot_url,
            },
            last_checked_at: now,
            last_indexed_ledger_height: snapshot.as_ref().map(|s| s.last_indexed_ledger_height),
            last_indexed_at: snapshot.as_ref().map(|s| s.indexed_at),
            consecutive_failures: snapshot.as_ref().map(|s| s.consecutive_failures).unwrap_or(0),
            status_message,
        });
    }

    Ok(NetworkListResponse {
        networks,
        cached_at: now,
    })
}

async fn refresh_network_catalog_cache(state: &AppState) -> Result<NetworkListResponse, ApiError> {
    let response = fetch_network_catalog(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch network catalog", err))?;

    if let Ok(serialized) = serde_json::to_string(&response) {
        state
            .cache
            .put(
                NETWORKS_CACHE_NAMESPACE,
                NETWORKS_CACHE_KEY,
                serialized,
                Some(Duration::from_secs(NETWORKS_REFRESH_INTERVAL_SECS)),
            )
            .await;
    }

    Ok(response)
}

pub async fn run_network_catalog_refresh(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(NETWORKS_REFRESH_INTERVAL_SECS));

    loop {
        interval.tick().await;
        if let Err(err) = refresh_network_catalog_cache(&state).await {
            tracing::warn!(error = ?err, "failed to refresh network catalog cache");
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "contract_audit_event_type", rename_all = "snake_case")]
#[allow(dead_code)]
pub enum ContractAuditEventType {
    ContractCreated,
    MetadataUpdated,
    VerificationAdded,
    StatusChanged,
    PublisherChanged,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow)]
#[allow(dead_code)]
pub struct ContractAuditLogEntry {
    pub id: Uuid,
    pub event_type: ContractAuditEventType,
    pub contract_id: Uuid,
    pub user_id: Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub changes: serde_json::Value,
    pub ip_address: String,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct AuditLogQuery {
    #[serde(default = "default_audit_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_audit_limit() -> i64 {
    100
}

#[derive(Debug, serde::Deserialize)]
pub struct PublisherContractsQuery {
    #[serde(default = "default_contracts_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_contracts_limit() -> i64 {
    20
}

fn extract_ip_address(headers: &HeaderMap) -> String {
    if let Some(forwarded_for) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        let first = forwarded_for
            .split(',')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(ip) = first {
            return ip.to_string();
        }
    }

    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return real_ip.to_string();
    }

    "unknown".to_string()
}

fn parse_batch_fields(raw_fields: Option<&str>) -> Option<HashSet<String>> {
    let set: HashSet<String> = raw_fields
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect();

    if set.is_empty() {
        None
    } else {
        Some(set)
    }
}

fn contract_to_filtered_value(contract: &Contract, fields: Option<&HashSet<String>>) -> Value {
    if fields.is_none() {
        return serde_json::to_value(contract).unwrap_or_else(|_| Value::Null);
    }

    let Some(source) = serde_json::to_value(contract)
        .ok()
        .and_then(|v| v.as_object().cloned())
    else {
        return Value::Null;
    };

    let mut out = serde_json::Map::new();
    let selected = fields.expect("checked above");

    for field in selected {
        if field == "address" {
            out.insert(
                "address".to_string(),
                Value::String(contract.contract_id.clone()),
            );
            continue;
        }

        if let Some(value) = source.get(field) {
            out.insert(field.clone(), value.clone());
        }
    }

    Value::Object(out)
}

async fn write_contract_audit_log(
    db: &sqlx::PgPool,
    action_type: AuditActionType,
    contract_id: Uuid,
    user_id: Uuid,
    changes: serde_json::Value,
    ip_address: &str,
) -> Result<(), sqlx::Error> {
    let (old_value, new_value) = split_audit_changes(&changes, ip_address);

    sqlx::query(
        "INSERT INTO contract_audit_log (action_type, contract_id, old_value, new_value, changed_by)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(action_type)
    .bind(contract_id)
    .bind(old_value)
    .bind(new_value)
    .bind(user_id.to_string())
    .execute(db)
    .await?;

    Ok(())
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = Object),
        (status = 503, description = "Service is unavailable or degraded", body = Object)
    ),
    tag = "Observability"
)]
fn split_audit_changes(
    changes: &serde_json::Value,
    ip_address: &str,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let mut old_value = serde_json::Map::new();
    let mut new_value = serde_json::Map::new();
    let mut saw_before_after_pair = false;

    match changes {
        serde_json::Value::Object(fields) => {
            for (field, delta) in fields {
                match delta {
                    serde_json::Value::Object(delta_obj) => {
                        let before = delta_obj.get("before");
                        let after = delta_obj.get("after");

                        if before.is_some() || after.is_some() {
                            saw_before_after_pair = true;
                            if let Some(before) = before {
                                if !before.is_null() {
                                    old_value.insert(field.clone(), before.clone());
                                }
                            }
                            if let Some(after) = after {
                                if !after.is_null() {
                                    new_value.insert(field.clone(), after.clone());
                                }
                            }
                        } else {
                            new_value.insert(field.clone(), delta.clone());
                        }
                    }
                    _ => {
                        new_value.insert(field.clone(), delta.clone());
                    }
                }
            }
        }
        _ => {
            new_value.insert("changes".to_string(), changes.clone());
        }
    }

    if !saw_before_after_pair && new_value.is_empty() {
        new_value.insert("changes".to_string(), changes.clone());
    }

    new_value.insert(
        "_ip_address".to_string(),
        serde_json::Value::String(ip_address.to_string()),
    );

    let old_value = if old_value.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(old_value))
    };
    let new_value = if new_value.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(new_value))
    };

    (old_value, new_value)
}

fn parse_interaction_type(
    interaction_type: Option<&str>,
    method: Option<&str>,
) -> Result<String, ApiError> {
    let mut normalized = interaction_type
        .map(|v| v.trim().to_ascii_lowercase())
        .or_else(|| {
            if method.is_some() {
                Some("invoke".to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "invoke".to_string());

    if normalized == "invocation" {
        normalized = "invoke".to_string();
    }

    let valid = matches!(
        normalized.as_str(),
        "deploy" | "invoke" | "transfer" | "query" | "publish_success" | "publish_failed"
    );

    if !valid {
        return Err(ApiError::bad_request(
            "InvalidInteractionType",
            format!(
                "interaction_type '{}' is invalid; expected one of: deploy, invoke, transfer, query, publish_success, publish_failed",
                normalized
            ),
        ));
    }

    Ok(normalized)
}

fn infer_target_identifier_from_parameters(
    parameters: Option<&serde_json::Value>,
) -> Option<String> {
    let payload = parameters?.as_object()?;
    let candidate_keys = [
        "target_contract_id",
        "target",
        "callee",
        "to_contract",
        "to",
        "contract_id",
    ];

    for key in candidate_keys {
        let Some(value) = payload.get(key) else {
            continue;
        };
        if let Some(identifier) = value.as_str() {
            let trimmed = identifier.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

async fn resolve_call_target_contract(
    db: &sqlx::PgPool,
    explicit_target: Option<&str>,
    parameters: Option<&serde_json::Value>,
) -> Result<Option<Uuid>, ApiError> {
    let candidate = explicit_target
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| infer_target_identifier_from_parameters(parameters));

    let Some(identifier) = candidate else {
        return Ok(None);
    };

    dependency::resolve_contract_id(db, &identifier)
        .await
        .map_err(|err| {
            ApiError::internal(format!(
                "Failed to resolve interaction target contract: {}",
                err
            ))
        })
}

async fn record_contract_interaction(
    db: &sqlx::PgPool,
    input: ContractInteractionInsert<'_>,
) -> Result<Uuid, sqlx::Error> {
    let mut tx = db.begin().await?;

    let interaction_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO contract_interactions
          (
            contract_id, user_address, interaction_type, transaction_hash, method,
            parameters, return_value, interaction_timestamp, interaction_count, network, created_at
          )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, $9, $10)
        RETURNING id
        "#,
    )
    .bind(input.contract_id)
    .bind(input.account)
    .bind(input.interaction_type)
    .bind(input.transaction_hash)
    .bind(input.method)
    .bind(input.parameters)
    .bind(input.return_value)
    .bind(input.timestamp)
    .bind(input.network)
    .bind(input.timestamp)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        INSERT INTO contract_interaction_daily_aggregates
          (contract_id, interaction_type, network, day, count, updated_at)
        VALUES ($1, $2, $3, $4, 1, NOW())
        ON CONFLICT (contract_id, interaction_type, network, day)
        DO UPDATE SET
          count = contract_interaction_daily_aggregates.count + 1,
          updated_at = NOW()
        "#,
    )
    .bind(input.contract_id)
    .bind(input.interaction_type)
    .bind(input.network)
    .bind(input.timestamp.date_naive())
    .execute(&mut *tx)
    .await?;

    if input.interaction_type == "invoke" {
        if let Some(target_contract_id) = input.target_contract_id {
            if target_contract_id != input.contract_id {
                sqlx::query(
                    r#"
                    INSERT INTO contract_call_edge_daily_aggregates
                      (source_contract_id, target_contract_id, network, day, call_count, updated_at)
                    VALUES ($1, $2, $3, $4, 1, NOW())
                    ON CONFLICT (source_contract_id, target_contract_id, network, day)
                    DO UPDATE SET
                      call_count = contract_call_edge_daily_aggregates.call_count + 1,
                      updated_at = NOW()
                    "#,
                )
                .bind(input.contract_id)
                .bind(target_contract_id)
                .bind(input.network)
                .bind(input.timestamp.date_naive())
                .execute(&mut *tx)
                .await?;
            }
        }
    }

    tx.commit().await?;

    Ok(interaction_id)
}

struct ContractInteractionInsert<'a> {
    contract_id: Uuid,
    target_contract_id: Option<Uuid>,
    account: Option<&'a str>,
    interaction_type: &'a str,
    transaction_hash: Option<&'a str>,
    method: Option<&'a str>,
    parameters: Option<&'a serde_json::Value>,
    return_value: Option<&'a serde_json::Value>,
    timestamp: chrono::DateTime<chrono::Utc>,
    network: &'a Network,
}

pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let uptime = state.started_at.elapsed().as_secs();
    let now = chrono::Utc::now().to_rfc3339();

    if state
        .is_shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        tracing::warn!(uptime_secs = uptime, "health check failing — shutting down");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "shutting_down",
                "version": "0.1.0",
                "timestamp": now,
                "uptime_secs": uptime
            })),
        );
    }

    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    if db_ok {
        tracing::info!(uptime_secs = uptime, "health check passed");
        (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "version": "0.1.0",
                "timestamp": now,
                "uptime_secs": uptime
            })),
        )
    } else {
        tracing::warn!(
            uptime_secs = uptime,
            "health check degraded — db unreachable"
        );
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "version": "0.1.0",
                "timestamp": now,
                "uptime_secs": uptime
            })),
        )
    }
}

#[utoipa::path(
    get,
    path = "/api/stats",
    responses(
        (status = 200, description = "Global registry statistics", body = Object)
    ),
    tag = "Observability"
)]
pub async fn get_stats(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let total_contracts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contracts")
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("count contracts", err))?;

    let verified_contracts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contracts WHERE is_verified = true")
            .fetch_one(&state.db)
            .await
            .map_err(|err| db_internal_error("count verified contracts", err))?;

    let total_publishers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM publishers")
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("count publishers", err))?;

    Ok(Json(json!({
        "total_contracts": total_contracts,
        "verified_contracts": verified_contracts,
        "total_publishers": total_publishers,
    })))
}

#[utoipa::path(
    get,
    path = "/networks",
    responses(
        (status = 200, description = "Supported network metadata", body = NetworkListResponse)
    ),
    tag = "Networks"
)]
pub async fn list_networks(State(state): State<AppState>) -> ApiResult<Json<NetworkListResponse>> {
    if let (Some(cached), true) = state
        .cache
        .get(NETWORKS_CACHE_NAMESPACE, NETWORKS_CACHE_KEY)
        .await
    {
        if let Ok(payload) = serde_json::from_str::<NetworkListResponse>(&cached) {
            return Ok(Json(payload));
        }
    }

    let response = refresh_network_catalog_cache(&state).await?;
    Ok(Json(response))
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct SearchSuggestionsQuery {
    pub q: String,
    #[serde(default = "default_search_suggestion_limit")]
    pub limit: i64,
}

fn default_search_suggestion_limit() -> i64 {
    8
}

#[utoipa::path(
    get,
    path = "/api/contracts/suggestions",
    params(SearchSuggestionsQuery),
    responses(
        (status = 200, description = "Autocomplete suggestions for contract search", body = SearchSuggestionsResponse)
    ),
    tag = "Contracts"
)]
pub async fn get_contract_search_suggestions(
    State(state): State<AppState>,
    Query(query): Query<SearchSuggestionsQuery>,
) -> ApiResult<Json<SearchSuggestionsResponse>> {
    let started_at = std::time::Instant::now();
    let limit = query.limit.clamp(1, 20);
    let normalized = query.q.trim().to_ascii_lowercase();

    if normalized.is_empty() {
        return Ok(Json(SearchSuggestionsResponse { items: Vec::new() }));
    }

    let cache_key = format!("suggestions:{}:{}", normalized, limit);
    if let (Some(cached), true) = state.cache.get("search", &cache_key).await {
        if let Ok(payload) = serde_json::from_str::<SearchSuggestionsResponse>(&cached) {
            observe_search_query("suggestions", started_at, Some(&query.q), limit);
            return Ok(Json(payload));
        }
    }

    let prefix = format!("{}%", normalized);
    let rows: Vec<(String, String, f64)> = sqlx::query_as(
        r#"
        WITH candidates AS (
            SELECT DISTINCT ON (lower(name))
                name AS text,
                'contract' AS kind,
                GREATEST(
                    similarity(lower(name), $1),
                    CASE WHEN lower(name) LIKE $2 THEN 1.0 ELSE 0.0 END
                ) AS score
            FROM contracts
            WHERE lower(name) LIKE $2 OR lower(name) % $1

            UNION ALL

            SELECT DISTINCT ON (lower(category))
                category AS text,
                'category' AS kind,
                GREATEST(
                    similarity(lower(category), $1),
                    CASE WHEN lower(category) LIKE $2 THEN 0.95 ELSE 0.0 END
                ) AS score
            FROM contracts
            WHERE category IS NOT NULL
              AND (lower(category) LIKE $2 OR lower(category) % $1)
        )
        SELECT text, kind, score
        FROM candidates
        WHERE text IS NOT NULL
        ORDER BY score DESC, length(text), text
        LIMIT $3
        "#,
    )
    .bind(&normalized)
    .bind(&prefix)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch search suggestions", err))?;

    let response = SearchSuggestionsResponse {
        items: rows
            .into_iter()
            .map(|(text, kind, score)| SearchSuggestion { text, kind, score })
            .collect(),
    };

    if let Ok(serialized) = serde_json::to_string(&response) {
        state
            .cache
            .put("search", &cache_key, serialized, Some(Duration::from_secs(60)))
            .await;
    }

    observe_search_query("suggestions", started_at, Some(&query.q), limit);
    Ok(Json(response))
}

/// List and search contracts
#[utoipa::path(
    get,
    path = "/api/contracts",
    params(ContractSearchParams),
    responses(
        (status = 200, description = "List of contracts", body = PaginatedResponse<Contract>),
        (status = 400, description = "Invalid query parameters")
    ),
    tag = "Contracts"
)]
pub async fn list_contracts(
    State(state): State<AppState>,
    params: Result<Query<ContractSearchParams>, QueryRejection>,
) -> axum::response::Response {
    let search_started_at = std::time::Instant::now();
    let Query(params) = match params {
        Ok(q) => q,
        Err(err) => return map_query_rejection(err).into_response(),
    };

    let limit = params.limit.unwrap_or(20).clamp(1, 100);

    // Cursor logic
    let cursor = params.cursor.as_ref().and_then(|c| Cursor::decode(c).ok());

    let (page, offset) = if cursor.is_some() {
        (1, 0) // Ignore page/offset if cursor is present
    } else {
        let p = params.page.unwrap_or(1).max(1);
        (p, (p - 1).max(0) * limit)
    };

    let sort_by = params.sort_by.clone().unwrap_or_else(|| {
        if params.query.is_some() {
            shared::SortBy::Relevance
        } else {
            shared::SortBy::CreatedAt
        }
    });
    let sort_order = params.sort_order.clone().unwrap_or(shared::SortOrder::Desc);
    let network_list = params
        .networks
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .or_else(|| params.network.map(|n| vec![n]));

    let timestamp_sort_column = sort_timestamp_column(&sort_by);
    let direction = if sort_order == shared::SortOrder::Asc { "ASC" } else { "DESC" };
    let direction_op = if sort_order == shared::SortOrder::Asc { ">" } else { "<" };
    let id_direction = if sort_order == shared::SortOrder::Asc { "ASC" } else { "DESC" };

    // Weights for ranking (configurable via parameters or default)
    let w_text = params.w_text.unwrap_or(1.0);
    let w_pop = params.w_pop.unwrap_or(0.5);
    let w_rec = params.w_rec.unwrap_or(0.3);
    let w_rat = params.w_rat.unwrap_or(0.4);
    let w_pers = 0.5; // Weight for personalized boost

    // Build dynamic query with advanced ranking
    let mut query = String::from("WITH contract_stats AS (\n");
    query.push_str(
        "    SELECT 
                c.id,
                COUNT(DISTINCT ci.id) as interaction_count,
                COUNT(DISTINCT cv.id) as deployment_count,
                COALESCE(AVG(r.rating), 0) as avg_rating,
                COUNT(DISTINCT r.id) as review_count",
    );

    if let Some(ref uid) = params.user_id {
        let cleaned_uid = uid.replace('\'', "''");
        query.push_str(&format!(
            ",\n                COUNT(DISTINCT CASE WHEN ci.user_address = '{}' THEN ci.id END) as user_interaction_count",
            cleaned_uid
        ));
    } else {
        query.push_str(",\n                0 as user_interaction_count");
    }

    query.push_str(
        "\n            FROM contracts c
            LEFT JOIN contract_interactions ci ON c.id = ci.contract_id
            LEFT JOIN contract_versions cv ON c.id = cv.contract_id
            LEFT JOIN reviews r ON c.id = r.contract_id AND r.is_flagged = FALSE
            GROUP BY c.id
        ),\n",
    );

    query.push_str("ranked_contracts AS (\n");
    query.push_str("    SELECT \n");
    query.push_str("        c.*, \n");
    
    if let Some(ref q) = params.query {
        query.push(" AND contracts_build_tsquery(");
        query.push_bind(q);
        query.push(") @@ c.search_document");

        count_query.push(" AND contracts_build_tsquery(");
        count_query.push_bind(q);
        count_query.push(") @@ c.search_document");
    }

    if params.verified_only.unwrap_or(false) {
        query.push(" AND c.is_verified = true");
        count_query.push(" AND c.is_verified = true");
    }

    if let Some(ref category) = params.category {
        query.push(" AND c.category = ");
        query.push_bind(category);
        count_query.push(" AND c.category = ");
        count_query.push_bind(category);
    }

    // Filter by network(s)
    let network_list = params
        .networks
        .as_ref()
        .filter(|n| !n.is_empty())
        .cloned()
        .or_else(|| params.network.map(|n| vec![n]));
    if let Some(ref nets) = network_list {
        query.push(" AND c.network IN (");
        let mut separated = query.separated(", ");
        for net in nets {
            separated.push_bind(net);
        }
        separated.push_unseparated(")");

        count_query.push(" AND c.network IN (");
        let mut separated = count_query.separated(", ");
        for net in nets {
            separated.push_bind(net);
        }
        separated.push_unseparated(")");
    }

    if let Some(created_from) = params.created_from {
        query.push(" AND c.created_at >= ");
        query.push_bind(created_from);
        count_query.push(" AND c.created_at >= ");
        count_query.push_bind(created_from);
    }

    if let Some(created_to) = params.created_to {
        query.push(" AND c.created_at <= ");
        query.push_bind(created_to);
        count_query.push(" AND c.created_at <= ");
        count_query.push_bind(created_to);
    }

    if let Some(updated_from) = params.updated_from {
        query.push(" AND c.updated_at >= ");
        query.push_bind(updated_from);
        count_query.push(" AND c.updated_at >= ");
        count_query.push_bind(updated_from);
    }

    if let Some(updated_to) = params.updated_to {
        query.push(" AND c.updated_at <= ");
        query.push_bind(updated_to);
        count_query.push(" AND c.updated_at <= ");
        count_query.push_bind(updated_to);
    }

    if let Some(verified_from) = params.verified_from {
        query.push(" AND c.verified_at >= ");
        query.push_bind(verified_from);
        count_query.push(" AND c.verified_at >= ");
        count_query.push_bind(verified_from);
    }

    if let Some(verified_to) = params.verified_to {
        query.push(" AND c.verified_at <= ");
        query.push_bind(verified_to);
        count_query.push(" AND c.verified_at <= ");
        count_query.push_bind(verified_to);
    }

    if let Some(last_accessed_from) = params.last_accessed_from {
        query.push(" AND c.last_accessed_at >= ");
        query.push_bind(last_accessed_from);
        count_query.push(" AND c.last_accessed_at >= ");
        count_query.push_bind(last_accessed_from);
    }

    if let Some(last_accessed_to) = params.last_accessed_to {
        query.push(" AND c.last_accessed_at <= ");
        query.push_bind(last_accessed_to);
        count_query.push(" AND c.last_accessed_at <= ");
        count_query.push_bind(last_accessed_to);
    }

    if let Some(ref cursor) = cursor {
        if let Some(column) = timestamp_sort_column {
            query.push(" AND (");
            query.push(column);
            query.push(" ");
            query.push(direction_op);
            query.push(" ");
            query.push_bind(cursor.timestamp);
            query.push(" OR (");
            query.push(column);
            query.push(" = ");
            query.push_bind(cursor.timestamp);
            query.push(" AND c.id ");
            query.push(direction_op);
            query.push(" ");
            query.push_bind(cursor.id);
            query.push("))");
        }
    }

    query.push(" GROUP BY c.id");

    match sort_by {
        shared::SortBy::CreatedAt
        | shared::SortBy::UpdatedAt
        | shared::SortBy::VerifiedAt
        | shared::SortBy::LastAccessedAt => {
            query.push(" ORDER BY ");
            query.push(timestamp_sort_column.unwrap_or("c.created_at"));
            query.push(" ");
            query.push(direction);
            query.push(" NULLS LAST, c.id ");
            query.push(id_direction);
        }
        shared::SortBy::Popularity | shared::SortBy::Interactions => {
            query.push(" ORDER BY COUNT(DISTINCT ci.id) ");
            query.push(direction);
            query.push(", c.id ");
            query.push(id_direction);
        }
        shared::SortBy::Deployments => {
            query.push(" ORDER BY COUNT(DISTINCT cv.id) ");
            query.push(direction);
            query.push(", c.id ");
            query.push(id_direction);
        }
        shared::SortBy::Relevance => {
            if let Some(ref q) = params.query {
                let prefix = format!("{}%", q.to_ascii_lowercase());
                query.push(" ORDER BY (CASE WHEN lower(c.name) = lower(");
                query.push_bind(q);
                query.push(") THEN 3.0 WHEN lower(c.name) LIKE ");
                query.push_bind(&prefix);
                query.push(" THEN 1.5 ELSE 0.0 END + ts_rank_cd(c.search_document, contracts_build_tsquery(");
                query.push_bind(q);
                query.push("), 32)) ");
                query.push(direction);
                query.push(", c.id ");
                query.push(id_direction);
            } else {
                query.push(" ORDER BY c.created_at ");
                query.push(direction);
                query.push(", c.id ");
                query.push(id_direction);
            }
        }
    }

    query.push(" LIMIT ");
    query.push_bind(limit);
    query.push(" OFFSET ");
    query.push_bind(offset);

    let contracts: Vec<Contract> = match query.build_query_as().fetch_all(&state.db).await {
        Ok(rows) => rows,
        Err(err) => {
            tracing::error!(query = %query, error = ?err, "Search query failed");
            return db_internal_error("list contracts with ranking", err).into_response()
        },
    };

    let total: i64 = match count_query.build_query_scalar().fetch_one(&state.db).await {
        Ok(v) => v,
        Err(err) => return db_internal_error("count filtered contracts", err).into_response(),
    };

    if params.query.is_some() {
        observe_search_query("contracts", search_started_at, params.query.as_deref(), limit);
    }

    let mut response = PaginatedResponse::new(contracts, total, page, limit);

    // Generate next cursor if we have full page
    if response.items.len() >= limit as usize {
        if let Some(last) = response.items.last() {
            if let Some(timestamp) = contract_timestamp_for_sort(last, &sort_by) {
                let next_cursor = Cursor::new(timestamp, last.id).encode();
                response.next_cursor = Some(next_cursor);
            }
        }
    }

    // Generate prev cursor if we have items and are not on the first page
    if params.cursor.is_some() || page > 1 {
        if let Some(first) = response.items.first() {
            if let Some(timestamp) = contract_timestamp_for_sort(first, &sort_by) {
                let prev_cursor = Cursor::new(timestamp, first.id).encode();
                response.prev_cursor = Some(prev_cursor);
            }
        }
    }

    (StatusCode::OK, Json(response)).into_response()
}

/// Get a specific contract by ID. Optional ?network= returns network-specific config (Issue #43).
#[utoipa::path(
    get,
    path = "/api/contracts/{id}",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        GetContractQuery
    ),
    responses(
        (status = 200, description = "Contract details", body = ContractGetResponse),
        (status = 404, description = "Contract not found"),
        (status = 400, description = "Invalid contract ID format")
    ),
    tag = "Contracts"
)]
pub async fn get_contract(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<GetContractQuery>,
) -> ApiResult<Json<ContractGetResponse>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let mut contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("get contract by id", err),
        })?;

    let current_network = query.network;
    let network_config = if let Some(ref net) = current_network {
        let configs: Option<std::collections::HashMap<String, NetworkConfig>> = contract
            .network_configs
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let net_key = net.to_string();
        let config = configs.and_then(|m| m.get(&net_key).cloned());
        if let Some(ref cfg) = config {
            contract.contract_id = cfg.contract_id.clone();
            contract.is_verified = cfg.is_verified;
            contract.network = net.clone();
        }
        config
    } else {
        None
    };

    // Record search click if applicable
    if query.from_search.unwrap_or(false) {
        let _ = analytics::record_event(
            &state.db,
            shared::AnalyticsEventType::SearchClick,
            Some(contract.id),
            Some(contract.publisher_id),
            None,
            query.network.as_ref(),
            Some(serde_json::json!({
                "search_query": query.search_query,
                "timestamp": chrono::Utc::now()
            })),
        )
        .await;
    }
    track_contract_access(&state, contract.id).await;

    Ok(Json(ContractGetResponse {
        contract,
        current_network,
        network_config,
    }))
}

/// Fetch multiple contracts in a single request, preserving request order.
#[utoipa::path(
    post,
    path = "/api/contracts/batch",
    params(BatchContractsQuery),
    request_body = Vec<String>,
    responses(
        (status = 200, description = "Batch contract results in request order", body = [Object]),
        (status = 400, description = "Invalid request")
    ),
    tag = "Contracts"
)]
pub async fn get_contracts_batch(
    State(state): State<AppState>,
    Query(query): Query<BatchContractsQuery>,
    Json(contract_ids): Json<Vec<String>>,
) -> ApiResult<Json<Vec<Option<Value>>>> {
    if contract_ids.len() > 100 {
        return Err(ApiError::bad_request(
            "BatchTooLarge",
            format!(
                "Maximum of 100 contract IDs allowed, received {}",
                contract_ids.len()
            ),
        ));
    }

    let fields = parse_batch_fields(query.fields.as_deref());

    let parsed_uuids: Vec<Uuid> = contract_ids
        .iter()
        .filter_map(|id| Uuid::parse_str(id.trim()).ok())
        .collect();

    let normalized_contract_ids: Vec<String> = contract_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    let contracts: Vec<Contract> = sqlx::query_as(
        "SELECT * FROM contracts
         WHERE id = ANY($1)
            OR contract_id = ANY($2)",
    )
    .bind(&parsed_uuids)
    .bind(&normalized_contract_ids)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get batch contracts", err))?;

    let mut by_uuid: HashMap<Uuid, Contract> = HashMap::new();
    let mut by_contract_id: HashMap<String, Contract> = HashMap::new();

    for contract in contracts {
        by_contract_id.insert(contract.contract_id.clone(), contract.clone());
        by_uuid.insert(contract.id, contract);
    }

    let mut ordered_results: Vec<Option<Value>> = Vec::with_capacity(contract_ids.len());
    for requested in contract_ids {
        let trimmed = requested.trim();

        let contract = Uuid::parse_str(trimmed)
            .ok()
            .and_then(|id| by_uuid.get(&id))
            .or_else(|| by_contract_id.get(trimmed));

        ordered_results.push(contract.map(|c| contract_to_filtered_value(c, fields.as_ref())));
    }

    Ok(Json(ordered_results))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/versions",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "List of contract versions", body = [ContractVersion]),
        (status = 404, description = "Contract not found"),
        (status = 400, description = "Invalid contract ID format")
    ),
    tag = "Versions"
)]
pub async fn get_contract_versions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ContractVersion>>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let versions: Vec<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 ORDER BY created_at DESC",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get contract versions", err))?;

    Ok(Json(versions))
}

#[utoipa::path(
    post,
    path = "/api/contracts/{id}/versions",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = CreateContractVersionRequest,
    responses(
        (status = 201, description = "Version created successfully", body = ContractVersion),
        (status = 400, description = "Invalid input or version conflict"),
        (status = 404, description = "Contract not found")
    ),
    tag = "Versions"
)]
/// GET /api/contracts/:id/changelog (and /contracts/:id/changelog) — release history with breaking-change markers.
pub async fn get_contract_changelog(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ContractChangelogResponse>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;

    // Ascending order makes it easy to compute diffs against the previous version.
    let versions: Vec<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 ORDER BY created_at ASC",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get contract versions for changelog", err))?;

    let mut entries: Vec<ContractChangelogEntry> = Vec::with_capacity(versions.len());

    let mut prev_version: Option<String> = None;
    for v in &versions {
        let mut breaking = false;
        let mut breaking_changes: Vec<String> = Vec::new();

        if let Some(prev) = prev_version.as_deref() {
            let old_selector = format!("{}@{}", contract_id, prev);
            let new_selector = format!("{}@{}", contract_id, v.version);

            let old_abi = resolve_abi(&state, &old_selector).await?;
            let new_abi = resolve_abi(&state, &new_selector).await?;

            let old_spec = crate::type_safety::parser::parse_json_spec(&old_abi, &old_selector)
                .map_err(|e| {
                    ApiError::bad_request("InvalidABI", format!("Failed to parse old ABI: {}", e))
                })?;
            let new_spec = crate::type_safety::parser::parse_json_spec(&new_abi, &new_selector)
                .map_err(|e| {
                    ApiError::bad_request("InvalidABI", format!("Failed to parse new ABI: {}", e))
                })?;

            let changes = diff_abi(&old_spec, &new_spec);
            breaking = has_breaking_changes(&changes);
            breaking_changes = changes
                .into_iter()
                .filter(|c| c.severity == crate::breaking_changes::ChangeSeverity::Breaking)
                .map(|c| c.message)
                .collect();
        }

        entries.push(ContractChangelogEntry {
            version: v.version.clone(),
            created_at: v.created_at,
            commit_hash: v.commit_hash.clone(),
            source_url: v.source_url.clone(),
            release_notes: v.release_notes.clone(),
            breaking,
            breaking_changes,
        });

        prev_version = Some(v.version.clone());
    }

    // Most APIs return newest-first for timelines.
    entries.reverse();

    Ok(Json(ContractChangelogResponse {
        contract_id: contract_uuid,
        entries,
    }))
}

pub async fn create_contract_version(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ValidatedJson(req): ValidatedJson<CreateContractVersionRequest>,
) -> ApiResult<Json<ContractVersion>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;
    if !req.contract_id.trim().is_empty() && req.contract_id != contract_id {
        return Err(ApiError::bad_request(
            "ContractMismatch",
            "Contract ID in payload does not match path",
        ));
    }

    let new_version = SemVer::parse(&req.version).ok_or_else(|| {
        ApiError::bad_request(
            "InvalidVersion",
            "Version must be valid semver (e.g. 1.2.3)",
        )
    })?;

    // Optional Ed25519 signature verification for this contract version.
    // When a signature is provided, we require a matching publisher_key and
    // verify the detached signature over "{contract_id}:{version}:{wasm_hash}".
    let (version_signature, version_publisher_key, version_algorithm) =
        match (&req.signature, &req.publisher_key) {
            (Some(sig), Some(pk)) if !sig.trim().is_empty() && !pk.trim().is_empty() => {
                // Decode public key (base64, 32 bytes)
                let pk_bytes = BASE64.decode(pk.trim()).map_err(|_| {
                    ApiError::bad_request(
                        "InvalidPublisherKey",
                        "publisher_key must be valid base64-encoded Ed25519 public key",
                    )
                })?;
                let pk_array: [u8; 32] = pk_bytes.as_slice().try_into().map_err(|_| {
                    ApiError::bad_request(
                        "InvalidPublisherKey",
                        "publisher_key must decode to 32 bytes",
                    )
                })?;
                let verifying_key = VerifyingKey::from_bytes(&pk_array).map_err(|_| {
                    ApiError::bad_request(
                        "InvalidPublisherKey",
                        "publisher_key is not a valid Ed25519 public key",
                    )
                })?;

                // Decode signature (base64, 64 bytes)
                let sig_bytes = BASE64.decode(sig.trim()).map_err(|_| {
                    ApiError::bad_request(
                        "InvalidSignature",
                        "signature must be valid base64-encoded Ed25519 signature",
                    )
                })?;
                let sig_array: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| {
                    ApiError::bad_request("InvalidSignature", "signature must decode to 64 bytes")
                })?;
                let signature = Signature::from_bytes(&sig_array);

                // Construct signing message and verify
                let message = crate::signing_handlers::create_signing_message(
                    &req.wasm_hash,
                    &contract_id,
                    &req.version,
                );

                let crypto_valid = verifying_key.verify(&message, &signature).is_ok();
                if !crypto_valid {
                    return Err(ApiError::unprocessable(
                        "InvalidSignature",
                        "Ed25519 signature verification failed for this contract version",
                    ));
                }

                let algo = req
                    .signature_algorithm
                    .clone()
                    .unwrap_or_else(|| "ed25519".to_string());

                tracing::info!(
                    contract_id = %contract_id,
                    version = %req.version,
                    wasm_hash = %req.wasm_hash,
                    "contract version signature verified"
                );

                (
                    Some(sig.trim().to_string()),
                    Some(pk.trim().to_string()),
                    Some(algo),
                )
            }
            (None, None) => {
                // No signature metadata provided – proceed without cryptographic binding.
                (None, None, None)
            }
            (Some(s), None) if s.trim().is_empty() => (None, None, None),
            (None, Some(pk)) if pk.trim().is_empty() => (None, None, None),
            _ => {
                return Err(ApiError::bad_request(
                    "InvalidSignatureMetadata",
                    "signature and publisher_key must both be provided (or both omitted)",
                ));
            }
        };

    let existing_versions: Vec<String> =
        sqlx::query_scalar("SELECT version FROM contract_versions WHERE contract_id = $1")
            .bind(contract_uuid)
            .fetch_all(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch contract versions", err))?;

    if !existing_versions.is_empty() {
        let mut parsed: Vec<SemVer> = Vec::with_capacity(existing_versions.len());
        for version in &existing_versions {
            let parsed_version = SemVer::parse(version).ok_or_else(|| {
                ApiError::unprocessable(
                    "InvalidExistingVersion",
                    format!("Existing version '{}' is not valid semver", version),
                )
            })?;
            parsed.push(parsed_version);
        }
        parsed.sort();
        let latest_version = parsed.last().cloned();

        if let Some(old_version) = latest_version {
            let old_selector = format!("{}@{}", contract_id, old_version);
            let old_abi = resolve_abi(&state, &old_selector).await?;
            let old_spec = crate::type_safety::parser::parse_json_spec(&old_abi, &contract_id)
                .map_err(|e| {
                    ApiError::bad_request("InvalidABI", format!("Failed to parse old ABI: {}", e))
                })?;

            let new_spec =
                crate::type_safety::parser::parse_json_spec(&req.abi.to_string(), &contract_id)
                    .map_err(|e| {
                        ApiError::bad_request(
                            "InvalidABI",
                            format!("Failed to parse new ABI: {}", e),
                        )
                    })?;

            let changes = diff_abi(&old_spec, &new_spec);
            if has_breaking_changes(&changes) && new_version.major == old_version.major {
                return Err(ApiError::unprocessable(
                    "BreakingChangeWithoutMajorBump",
                    format!(
                        "Breaking changes detected; bump major version from {} to {}",
                        old_version, new_version
                    ),
                ));
            }
        }
    }

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|err| db_internal_error("begin transaction", err))?;

    let version_row: ContractVersion = sqlx::query_as(
        "INSERT INTO contract_versions \
            (contract_id, version, wasm_hash, source_url, commit_hash, release_notes, signature, publisher_key, signature_algorithm) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING *",
    )
    .bind(contract_uuid)
    .bind(&req.version)
    .bind(&req.wasm_hash)
    .bind(&req.source_url)
    .bind(&req.commit_hash)
    .bind(&req.release_notes)
    .bind(&version_signature)
    .bind(&version_publisher_key)
    .bind(&version_algorithm)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(db_err)
            if db_err.constraint() == Some("contract_versions_contract_id_version_key") =>
        {
            ApiError::unprocessable(
                "VersionAlreadyExists",
                format!("Version '{}' already exists for this contract", req.version),
            )
        }
        _ => db_internal_error("insert contract version", err),
    })?;

    sqlx::query(
        "INSERT INTO contract_abis (contract_id, version, abi) VALUES ($1, $2, $3) \
         ON CONFLICT (contract_id, version) DO UPDATE SET abi = EXCLUDED.abi",
    )
    .bind(contract_uuid)
    .bind(&req.version)
    .bind(&req.abi)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("insert contract abi", err))?;

    tx.commit()
        .await
        .map_err(|err| db_internal_error("commit contract version", err))?;

    state.cache.invalidate_abi(&contract_id).await;
    state.cache.invalidate_abi(&contract_uuid.to_string()).await;
    state
        .cache
        .invalidate_abi(&format!("{}@{}", contract_id, req.version))
        .await;

    // Post-commit dependency analysis
    let detected_deps = dependency::detect_dependencies_from_abi(&req.abi);
    if !detected_deps.is_empty() {
        if let Err(e) =
            dependency::save_dependencies(&state.db, contract_uuid, &detected_deps).await
        {
            tracing::error!(
                "Failed to save dependencies for version {}: {}",
                req.version,
                e
            );
        }
        // Invalidate global graph cache
        state
            .cache
            .invalidate("system", "global:dependency_graph")
            .await;
    }

    let _ = analytics::record_event(
        &state.db,
        AnalyticsEventType::VersionCreated,
        Some(version_row.contract_id),
        None, // Version creation usually doesn't need publisher_id explicitly if we have contract_id
        None,
        None,
        Some(json!({ "version": version_row.version })),
    )
    .await;

    if let Ok(contract) = sqlx::query_as::<_, Contract>("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
    {
        state
            .contract_events
            .publish(ContractEventEnvelope::version_created(&contract, &version_row));
    }

    Ok(Json(version_row))
}

async fn fetch_contract_identity(state: &AppState, id: &str) -> ApiResult<(Uuid, String)> {
    if let Ok(uuid) = Uuid::parse_str(id) {
        let row = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, contract_id FROM contracts WHERE id = $1",
        )
        .bind(uuid)
        .fetch_optional(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch contract", err))?;
        return row.ok_or_else(|| {
            ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            )
        });
    }

    let row = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, contract_id FROM contracts WHERE contract_id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch contract", err))?;

    row.ok_or_else(|| {
        ApiError::not_found(
            "ContractNotFound",
            format!("No contract found with ID: {}", id),
        )
    })
}

#[utoipa::path(
    post,
    path = "/api/contracts",
    request_body = PublishRequest,
    responses(
        (status = 201, description = "Contract published successfully", body = Contract),
        (status = 400, description = "Invalid input or contract ID"),
        (status = 409, description = "Contract already registered")
    ),
    tag = "Contracts"
)]
async fn ensure_contract_exists(
    state: &AppState,
    contract_uuid: Uuid,
    contract_id_raw: &str,
    operation: &str,
) -> ApiResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contracts WHERE id = $1)")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error(operation, err))?;

    if exists {
        Ok(())
    } else {
        Err(ApiError::not_found(
            "ContractNotFound",
            format!("No contract found with ID: {}", contract_id_raw),
        ))
    }
}

async fn fetch_contract_network(
    state: &AppState,
    contract_uuid: Uuid,
    contract_id_raw: &str,
    operation: &str,
) -> ApiResult<Network> {
    let network: Option<Network> =
        sqlx::query_scalar("SELECT network FROM contracts WHERE id = $1")
            .bind(contract_uuid)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| db_internal_error(operation, err))?;

    network.ok_or_else(|| {
        ApiError::not_found(
            "ContractNotFound",
            format!("No contract found with ID: {}", contract_id_raw),
        )
    })
}

pub async fn publish_contract(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<PublishRequest>,
) -> ApiResult<Json<Contract>> {
    let publisher: Publisher = sqlx::query_as(
        "INSERT INTO publishers (stellar_address) VALUES ($1)
         ON CONFLICT (stellar_address) DO UPDATE SET stellar_address = EXCLUDED.stellar_address
         RETURNING *",
    )
    .bind(&req.publisher_address)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("upsert publisher", err))?;

    let wasm_hash = req.wasm_hash.clone();
    let network_key = req.network.to_string();
    let mut config_map = serde_json::Map::new();
    config_map.insert(
        network_key,
        serde_json::json!({
            "contract_id": req.contract_id,
            "is_verified": false,
            "min_version": null,
            "max_version": null
        }),
    );
    let network_configs = serde_json::Value::Object(config_map);

    let contract: Contract = sqlx::query_as(
        "INSERT INTO contracts (contract_id, wasm_hash, name, description, publisher_id, network, category, tags, logical_id, network_configs)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING *"
    )
    .bind(&req.contract_id)
    .bind(&wasm_hash)
    .bind(&req.name)
    .bind(&req.description)
    .bind(publisher.id)
    .bind(&req.network)
    .bind(&req.category)
    .bind(&req.tags)
    .bind(Option::<Uuid>::None as Option<Uuid>)
    .bind(&network_configs)
    .fetch_one(&state.db)
    .await
    .map_err(|err| {
        if let sqlx::Error::Database(ref e) = err {
            if e.constraint() == Some("contracts_contract_id_network_key") {
                return ApiError::conflict(
                    "ContractAlreadyRegistered",
                    format!(
                        "Contract {} is already registered for network {}",
                        req.contract_id,
                        req.network
                    ),
                );
            }
        }
        db_internal_error("create contract", err)
    })?;

    // Set logical_id = id so this row is its own logical contract (Issue #43)
    let _ = sqlx::query("UPDATE contracts SET logical_id = id WHERE id = $1")
        .bind(contract.id)
        .execute(&state.db)
        .await;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract.id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch contract after insert", err))?;

    // Save dependencies if provided
    if !req.dependencies.is_empty() {
        if let Err(e) =
            dependency::save_dependencies(&state.db, contract.id, &req.dependencies).await
        {
            tracing::error!(
                "Failed to save initial dependencies for contract {}: {}",
                contract.contract_id,
                e
            );
        }
        // Invalidate global graph cache
        state
            .cache
            .invalidate("system", "global:dependency_graph")
            .await;
    }

    let creation_changes = json!({
        "contract_id": { "before": Value::Null, "after": contract.contract_id },
        "name": { "before": Value::Null, "after": contract.name },
        "description": { "before": Value::Null, "after": contract.description },
        "publisher_id": { "before": Value::Null, "after": contract.publisher_id },
        "network": { "before": Value::Null, "after": contract.network.to_string() },
        "is_verified": { "before": Value::Null, "after": contract.is_verified },
        "category": { "before": Value::Null, "after": contract.category },
        "tags": { "before": Value::Null, "after": contract.tags }
    });

    write_contract_audit_log(
        &state.db,
        AuditActionType::ContractPublished,
        contract.id,
        publisher.id,
        creation_changes,
        &extract_ip_address(&headers),
    )
    .await
    .map_err(|err| db_internal_error("write contract_created audit log", err))?;

    record_contract_interaction(
        &state.db,
        ContractInteractionInsert {
            contract_id: contract.id,
            target_contract_id: None,
            account: Some(&publisher.stellar_address),
            interaction_type: "publish_success",
            transaction_hash: None,
            method: None,
            parameters: None,
            return_value: None,
            timestamp: chrono::Utc::now(),
            network: &contract.network,
        },
    )
    .await
    .map_err(|err| db_internal_error("record publish_success interaction", err))?;

    let _ = analytics::record_event(
        &state.db,
        AnalyticsEventType::ContractPublished,
        Some(contract.id),
        Some(publisher.id),
        None,
        Some(&contract.network),
        Some(json!({ "name": contract.name })),
    )
    .await;

    state.contract_events.publish(ContractEventEnvelope::deployed(
        &contract,
        Some(publisher.stellar_address.clone()),
    ));

    Ok(Json(contract))
}

#[utoipa::path(
    post,
    path = "/api/publishers",
    request_body = Publisher,
    responses(
        (status = 201, description = "Publisher created successfully", body = Publisher),
        (status = 400, description = "Invalid input")
    ),
    tag = "Publishers"
)]
pub async fn create_publisher(
    State(state): State<AppState>,
    ValidatedJson(publisher): ValidatedJson<Publisher>,
) -> ApiResult<Json<Publisher>> {
    let created: Publisher = sqlx::query_as(
        "INSERT INTO publishers (stellar_address, username, email, github_url, website)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(&publisher.stellar_address)
    .bind(&publisher.username)
    .bind(&publisher.email)
    .bind(&publisher.github_url)
    .bind(&publisher.website)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("create publisher", err))?;

    let _ = analytics::record_event(
        &state.db,
        AnalyticsEventType::PublisherCreated,
        None,
        Some(created.id),
        None,
        None,
        Some(json!({ "stellar_address": created.stellar_address })),
    )
    .await;

    Ok(Json(created))
}

#[utoipa::path(
    get,
    path = "/api/publishers/{id}",
    params(
        ("id" = String, Path, description = "Publisher UUID")
    ),
    responses(
        (status = 200, description = "Publisher details", body = Publisher),
        (status = 404, description = "Publisher not found")
    ),
    tag = "Publishers"
)]
pub async fn get_publisher(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Publisher>> {
    let publisher_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidPublisherId",
            format!("Invalid publisher ID format: {}", id),
        )
    })?;

    let publisher: Publisher = sqlx::query_as("SELECT * FROM publishers WHERE id = $1")
        .bind(publisher_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "PublisherNotFound",
                format!("No publisher found with ID: {}", id),
            ),
            _ => db_internal_error("get publisher by id", err),
        })?;

    Ok(Json(publisher))
}

#[utoipa::path(
    get,
    path = "/api/publishers/{id}/contracts",
    params(
        ("id" = String, Path, description = "Publisher UUID")
    ),
    responses(
        (status = 200, description = "List of contracts by publisher", body = [Contract]),
        (status = 404, description = "Publisher not found")
    ),
    tag = "Publishers"
)]
pub async fn get_publisher_contracts(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PublisherContractsQuery>,
) -> ApiResult<Json<PaginatedResponse<Contract>>> {
    let publisher_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidPublisherId",
            format!("Invalid publisher ID format: {}", id),
        )
    })?;

    // Validate and cap limit (max 100)
    let limit = query.limit.clamp(1, 100);
    let offset = query.offset.max(0);

    // Get total count
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contracts WHERE publisher_id = $1")
        .bind(publisher_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("get publisher contracts count", err))?;

    // Fetch paginated results
    let contracts: Vec<Contract> = sqlx::query_as(
        "SELECT * FROM contracts WHERE publisher_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(publisher_uuid)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get publisher contracts", err))?;

    let page = (offset / limit) + 1;
    let response = PaginatedResponse::new(contracts, total, page, limit);

    Ok(Json(response))
}

/// Query for contract ABI and OpenAPI (optional version)
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ContractAbiQuery {
    pub version: Option<String>,
}

/// Fetch ABI JSON string for contract (by id or id@version)
async fn resolve_contract_abi(
    state: &AppState,
    id: &str,
    version: Option<&str>,
) -> ApiResult<String> {
    let selector = match version {
        Some(v) => format!("{}@{}", id, v),
        None => id.to_string(),
    };
    resolve_abi(state, &selector).await
}

// Contract ABI and OpenAPI endpoints
#[utoipa::path(
    get,
    path = "/api/contracts/{id}/abi",
    params(
        ("id" = String, Path, description = "Contract identifier (address or name)"),
        ContractAbiQuery
    ),
    responses(
        (status = 200, description = "Contract ABI", body = Object),
        (status = 404, description = "Contract or version not found")
    ),
    tag = "Artifacts"
)]
pub async fn get_contract_abi(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ContractAbiQuery>,
) -> ApiResult<Json<Value>> {
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref()).await?;
    let abi: Value = serde_json::from_str(&abi_json)
        .map_err(|e| ApiError::internal(format!("Invalid ABI JSON: {}", e)))?;
    Ok(Json(json!({ "abi": abi })))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/openapi.yaml",
    params(
        ("id" = String, Path, description = "Contract identifier"),
        ContractAbiQuery
    ),
    responses(
        (status = 200, description = "OpenAPI YAML specification", body = String),
        (status = 404, description = "Contract or version not found")
    ),
    tag = "Artifacts"
)]
pub async fn get_contract_openapi_yaml(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ContractAbiQuery>,
) -> ApiResult<Response> {
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref()).await?;
    let abi = parse_json_spec(&abi_json, &id)
        .map_err(|e| ApiError::bad_request("InvalidABI", format!("Failed to parse ABI: {}", e)))?;
    let doc = generate_openapi(&abi, Some("/invoke"));
    let yaml = to_yaml(&doc).map_err(|e| ApiError::internal(format!("OpenAPI YAML: {}", e)))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-yaml")
        .body(axum::body::Body::from(yaml))
        .map_err(|_| ApiError::internal("Failed to build response"))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/openapi.json",
    params(
        ("id" = String, Path, description = "Contract identifier"),
        ContractAbiQuery
    ),
    responses(
        (status = 200, description = "OpenAPI JSON specification", body = Object),
        (status = 404, description = "Contract or version not found")
    ),
    tag = "Artifacts"
)]
pub async fn get_contract_openapi_json(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ContractAbiQuery>,
) -> ApiResult<Response> {
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref()).await?;
    let abi = parse_json_spec(&abi_json, &id)
        .map_err(|e| ApiError::bad_request("InvalidABI", format!("Failed to parse ABI: {}", e)))?;
    let doc = generate_openapi(&abi, Some("/invoke"));
    let json = to_json(&doc).map_err(|e| ApiError::internal(format!("OpenAPI JSON: {}", e)))?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(json))
        .map_err(|_| ApiError::internal("Failed to build response"))
}

// Stubs for upstream added endpoints
fn planned_not_implemented_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "not_implemented",
            "message": "This endpoint is planned but not yet functional"
        })),
    )
}

pub async fn get_contract_state() -> impl IntoResponse {
    planned_not_implemented_response()
}

pub async fn update_contract_state() -> impl IntoResponse {
    planned_not_implemented_response()
}

/// GET /api/contracts/:id/analytics — timeline and top users from contract_interactions (Issue #46).
#[utoipa::path(
    get,
    path = "/api/contracts/{id}/analytics",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "Contract analytics and usage data", body = ContractAnalyticsResponse),
        (status = 404, description = "Contract not found")
    ),
    tag = "Analytics"
)]
pub async fn get_contract_analytics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ContractAnalyticsResponse>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    ensure_contract_exists(&state, contract_uuid, &id, "get contract for analytics").await?;

    let thirty_days_ago = chrono::Utc::now() - chrono::Duration::days(30);

    let unique_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address) FROM contract_interactions \
         WHERE contract_id = $1 AND user_address IS NOT NULL",
    )
    .bind(contract_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("analytics unique interactors", e))?;

    let top_user_rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT user_address, COUNT(*) AS cnt FROM contract_interactions \
         WHERE contract_id = $1 AND user_address IS NOT NULL \
         GROUP BY user_address ORDER BY cnt DESC LIMIT 10",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("analytics top users", e))?;

    let top_users: Vec<TopUser> = top_user_rows
        .into_iter()
        .filter_map(|(addr, count)| addr.map(|a| TopUser { address: a, count }))
        .collect();

    let timeline_rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        r#"
        SELECT d::date AS date, COALESCE(e.cnt, 0)::bigint AS count
        FROM generate_series(
            ($1::timestamptz)::date,
            CURRENT_DATE,
            '1 day'::interval
        ) d
        LEFT JOIN (
            SELECT created_at::date AS event_date, COUNT(*) AS cnt
            FROM contract_interactions
            WHERE contract_id = $2 AND created_at >= $1
            GROUP BY created_at::date
        ) e ON d::date = e.event_date
        ORDER BY d::date
        "#,
    )
    .bind(thirty_days_ago)
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("analytics timeline", e))?;

    let timeline: Vec<TimelineEntry> = timeline_rows
        .into_iter()
        .map(|(date, count)| TimelineEntry { date, count })
        .collect();

    Ok(Json(ContractAnalyticsResponse {
        contract_id: contract_uuid,
        deployments: DeploymentStats {
            count: 0,
            unique_users: 0,
            by_network: serde_json::json!({}),
        },
        interactors: InteractorStats {
            unique_count,
            top_users,
        },
        timeline,
    }))
}

pub async fn get_trust_score() -> impl IntoResponse {
    planned_not_implemented_response()
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/dependencies",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "List of direct dependencies", body = Object),
        (status = 404, description = "Contract not found")
    ),
    tag = "Graphs"
)]
pub async fn get_contract_dependencies(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let contract_uuid = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("InvalidContractId", format!("Invalid ID: {}", id)))?;

    let deps: Vec<shared::ContractDependency> =
        sqlx::query_as("SELECT * FROM contract_dependencies WHERE contract_id = $1")
            .bind(contract_uuid)
            .fetch_all(&state.db)
            .await
            .map_err(|e| db_internal_error("get_contract_dependencies", e))?;

    Ok(Json(json!({ "dependencies": deps })))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/dependents",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "List of direct dependents", body = Object),
        (status = 404, description = "Contract not found")
    ),
    tag = "Graphs"
)]
pub async fn get_contract_dependents(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let contract_uuid = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("InvalidContractId", format!("Invalid ID: {}", id)))?;

    let dependents: Vec<shared::ContractDependency> =
        sqlx::query_as("SELECT * FROM contract_dependencies WHERE dependency_contract_id = $1")
            .bind(contract_uuid)
            .fetch_all(&state.db)
            .await
            .map_err(|e| db_internal_error("get_contract_dependents", e))?;

    Ok(Json(json!({ "dependents": dependents })))
}

#[utoipa::path(
    get,
    path = "/api/contracts/graph",
    responses(
        (status = 200, description = "Full contract dependency graph", body = GraphResponse)
    ),
    tag = "Graphs"
)]
pub async fn get_contract_graph(
    State(state): State<AppState>,
    Query(query): Query<GetContractQuery>,
) -> ApiResult<Json<shared::GraphResponse>> {
    // Try cache first
    let cache_key = format!(
        "global:dependency_graph:{}",
        query
            .network
            .as_ref()
            .map(|network| network.to_string())
            .unwrap_or_else(|| "all".to_string())
    );
    if let (Some(cached), true) = state.cache.get("system", &cache_key).await {
        if let Ok(graph) = serde_json::from_str(&cached) {
            return Ok(Json(graph));
        }
    }

    let graph = dependency::build_dependency_graph(&state.db, query.network)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to build graph: {}", e)))?;

    // Invalidate/Refresh cache
    if let Ok(serialized) = serde_json::to_string(&graph) {
        state
            .cache
            .put(
                "system",
                &cache_key,
                serialized,
                Some(Duration::from_secs(300)),
            )
            .await;
    }

    Ok(Json(graph))
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ImpactQuery {
    pub change: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/impact",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        ImpactQuery
    ),
    responses(
        (status = 200, description = "Impact analysis for proposed changes", body = Object)
    ),
    tag = "Graphs"
)]
pub async fn get_impact_analysis(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ImpactQuery>,
) -> ApiResult<Json<shared::ImpactAnalysisResponse>> {
    let contract_uuid = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("InvalidContractId", format!("Invalid ID: {}", id)))?;

    let affected_ids = dependency::get_transitive_dependents(&state.db, contract_uuid)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to get impact: {}", e)))?;

    // Check for cycles involving this contract
    let has_cycles = affected_ids.contains(&contract_uuid);

    // Fetch details for affected contracts
    let affected_contracts: Vec<shared::Contract> = if !affected_ids.is_empty() {
        sqlx::query_as("SELECT * FROM contracts WHERE id = ANY($1)")
            .bind(&affected_ids)
            .fetch_all(&state.db)
            .await
            .map_err(|e| db_internal_error("get_impact_contracts", e))?
    } else {
        Vec::new()
    };

    Ok(Json(shared::ImpactAnalysisResponse {
        contract_id: contract_uuid,
        change_type: query.change,
        affected_count: affected_ids.len(),
        affected_contracts,
        has_cycles,
    }))
}

#[utoipa::path(
    get,
    path = "/api/contracts/trending",
    responses(
        (status = 200, description = "List of trending contracts", body = Object)
    ),
    tag = "Contracts"
)]
pub async fn get_trending_contracts(
    State(state): State<AppState>,
    Query(params): Query<TrendingParams>,
) -> ApiResult<Json<Value>> {
    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    let timeframe = params.timeframe.unwrap_or_else(|| "7d".to_string());
    let trailing_days = match timeframe.as_str() {
        "7d" => 7,
        "30d" => 30,
        "90d" => 90,
        _ => {
            return Err(ApiError::bad_request(
                "InvalidTimeframe",
                "timeframe must be one of: 7d, 30d, 90d",
            ));
        }
    };

    let rows: Vec<(Uuid, String, String, Network, i64, i64)> = sqlx::query_as(
        r#"
        WITH scored AS (
            SELECT
                c.id,
                c.contract_id,
                c.name,
                c.network,
                COALESCE(
                    SUM(a.count) FILTER (WHERE a.day >= CURRENT_DATE - INTERVAL '6 days'),
                    0
                )::bigint AS interactions_this_week,
                COALESCE(
                    SUM(a.count) FILTER (
                        WHERE a.day >= CURRENT_DATE - INTERVAL '13 days'
                          AND a.day < CURRENT_DATE - INTERVAL '6 days'
                    ),
                    0
                )::bigint AS interactions_last_week
            FROM contracts c
            LEFT JOIN contract_interaction_daily_aggregates a
              ON a.contract_id = c.id
             AND a.day >= CURRENT_DATE - make_interval(days => $1)
            GROUP BY c.id, c.contract_id, c.name, c.network
        )
        SELECT
            id,
            contract_id,
            name,
            network,
            interactions_this_week,
            interactions_last_week
        FROM scored
        WHERE interactions_this_week > interactions_last_week * 1.5
        ORDER BY
            CASE
                WHEN interactions_last_week = 0 THEN interactions_this_week::numeric
                ELSE interactions_this_week::numeric / interactions_last_week::numeric
            END DESC,
            interactions_this_week DESC
        LIMIT $2
        "#,
    )
    .bind(trailing_days)
    .bind(limit)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch trending contracts", err))?;

    let trending: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, contract_id, name, network, interactions_this_week, interactions_last_week)| {
                let ratio = if interactions_last_week == 0 {
                    if interactions_this_week > 0 {
                        serde_json::Value::String("inf".to_string())
                    } else {
                        serde_json::Value::from(0.0)
                    }
                } else {
                    serde_json::Value::from(
                        interactions_this_week as f64 / interactions_last_week as f64,
                    )
                };

                json!({
                    "id": id,
                    "contract_id": contract_id,
                    "name": name,
                    "network": network,
                    "interactions_this_week": interactions_this_week,
                    "interactions_last_week": interactions_last_week,
                    "ratio": ratio,
                    "is_trending": true
                })
            },
        )
        .collect();

    Ok(Json(json!({
        "timeframe": timeframe,
        "limit": limit,
        "trending": trending
    })))
}

#[utoipa::path(
    post,
    path = "/api/contracts/verify",
    request_body = VerifyRequest,
    responses(
        (status = 200, description = "Verification successful", body = Object),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Contract not found")
    ),
    tag = "Verification"
)]
pub async fn verify_contract(
    State(state): State<AppState>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<VerifyRequest>,
) -> ApiResult<Json<Value>> {
    let contract: Contract = sqlx::query_as(
        "SELECT * FROM contracts WHERE contract_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&req.contract_id)
    .fetch_one(&state.db)
    .await
    .map_err(|err| match err {
        sqlx::Error::RowNotFound => ApiError::not_found(
            "ContractNotFound",
            format!("No contract found with contract_id: {}", req.contract_id),
        ),
        _ => db_internal_error("fetch contract for verification", err),
    })?;

    let previous_status: Option<String> = sqlx::query_scalar(
        "SELECT status::text FROM verifications WHERE contract_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(contract.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch previous verification status", err))?;

    let verification_id: Uuid = sqlx::query_scalar(
        "INSERT INTO verifications (contract_id, status, source_code, build_params, compiler_version, verified_at, error_message)
         VALUES ($1, 'pending', $2, $3, $4, NULL, NULL)
         RETURNING id",
    )
    .bind(contract.id)
    .bind(&req.source_code)
    .bind(&req.build_params)
    .bind(&req.compiler_version)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("insert verification record", err))?;

    let verification_result = verifier::verify_contract(
        &req.source_code,
        &contract.wasm_hash,
        Some(&req.compiler_version),
        Some(&req.build_params),
    )
    .await;

    let ip_address = extract_ip_address(&headers);
    let before_status = previous_status.unwrap_or_else(|| "pending".to_string());

    match verification_result {
        Ok(result) if result.verified => {
            sqlx::query(
                "UPDATE verifications
                 SET status = 'verified', verified_at = NOW(), error_message = NULL
                 WHERE id = $1",
            )
            .bind(verification_id)
            .execute(&state.db)
            .await
            .map_err(|err| db_internal_error("mark verification as verified", err))?;

            sqlx::query(
                "UPDATE contracts SET is_verified = true, verified_at = NOW(), updated_at = NOW() WHERE id = $1",
            )
            .bind(contract.id)
            .execute(&state.db)
            .await
            .map_err(|err| db_internal_error("mark contract verified", err))?;

            let verification_changes = json!({
                "verification_id": { "before": Value::Null, "after": verification_id },
                "status": { "before": Value::Null, "after": "verified" },
                "compiler_version": { "before": Value::Null, "after": req.compiler_version },
                "verified_at": { "before": Value::Null, "after": chrono::Utc::now() },
                "compiled_wasm_hash": { "before": Value::Null, "after": result.compiled_wasm_hash },
                "deployed_wasm_hash": { "before": Value::Null, "after": result.deployed_wasm_hash }
            });

            write_contract_audit_log(
                &state.db,
                AuditActionType::VerificationChanged,
                contract.id,
                contract.publisher_id,
                verification_changes,
                &ip_address,
            )
            .await
            .map_err(|err| db_internal_error("write verification_added audit log", err))?;

            if before_status != "verified" {
                let status_changes = json!({
                    "status": { "before": before_status, "after": "verified" },
                    "is_verified": { "before": contract.is_verified, "after": true }
                });
                write_contract_audit_log(
                    &state.db,
                    AuditActionType::VerificationChanged,
                    contract.id,
                    contract.publisher_id,
                    status_changes,
                    &ip_address,
                )
                .await
                .map_err(|err| db_internal_error("write status_changed audit log", err))?;
            }

            record_contract_interaction(
                &state.db,
                ContractInteractionInsert {
                    contract_id: contract.id,
                    target_contract_id: None,
                    account: None,
                    interaction_type: "publish_success",
                    transaction_hash: None,
                    method: Some("verify"),
                    parameters: None,
                    return_value: None,
                    timestamp: chrono::Utc::now(),
                    network: &contract.network,
                },
            )
            .await
            .map_err(|err| db_internal_error("record verification interaction", err))?;

            let _ = analytics::record_event(
                &state.db,
                AnalyticsEventType::ContractVerified,
                Some(contract.id),
                Some(contract.publisher_id),
                None,
                Some(&contract.network),
                Some(json!({ "verification_id": verification_id })),
            )
            .await;
            let _ = analytics::record_event(
                &state.db,
                AnalyticsEventType::ContractVerified,
                Some(contract.id),
                Some(contract.publisher_id),
                None,
                Some(&contract.network),
                Some(json!({ "verification_id": verification_id })),
            )
            .await;

            Ok(Json(json!({
                "verified": true,
                "status": "verified",
                "verification_id": verification_id,
                "contract_id": contract.id,
                "compiled_wasm_hash": result.compiled_wasm_hash,
                "deployed_wasm_hash": result.deployed_wasm_hash
            })))
        }
        Ok(result) => {
            let failure_message = result
                .message
                .unwrap_or_else(|| "Verification failed due to bytecode mismatch".to_string());

            sqlx::query(
                "UPDATE verifications
                 SET status = 'failed', verified_at = NULL, error_message = $2
                 WHERE id = $1",
            )
            .bind(verification_id)
            .bind(&failure_message)
            .execute(&state.db)
            .await
            .map_err(|err| db_internal_error("mark verification as failed", err))?;

            let verification_changes = json!({
                "verification_id": { "before": Value::Null, "after": verification_id },
                "status": { "before": Value::Null, "after": "failed" },
                "compiler_version": { "before": Value::Null, "after": req.compiler_version },
                "error_message": { "before": Value::Null, "after": failure_message },
                "compiled_wasm_hash": { "before": Value::Null, "after": result.compiled_wasm_hash },
                "deployed_wasm_hash": { "before": Value::Null, "after": result.deployed_wasm_hash }
            });
            write_contract_audit_log(
                &state.db,
                AuditActionType::VerificationChanged,
                contract.id,
                contract.publisher_id,
                verification_changes,
                &ip_address,
            )
            .await
            .map_err(|err| db_internal_error("write failed verification audit log", err))?;

            if before_status != "failed" {
                let status_changes = json!({
                    "status": { "before": before_status, "after": "failed" },
                    "is_verified": { "before": contract.is_verified, "after": contract.is_verified }
                });
                write_contract_audit_log(
                    &state.db,
                    AuditActionType::VerificationChanged,
                    contract.id,
                    contract.publisher_id,
                    status_changes,
                    &ip_address,
                )
                .await
                .map_err(|err| db_internal_error("write failed status audit log", err))?;
            }

            Err(ApiError::unprocessable(
                "VerificationFailed",
                failure_message,
            ))
        }
        Err(err) => {
            let failure_message = err.to_string();

            sqlx::query(
                "UPDATE verifications
                 SET status = 'failed', verified_at = NULL, error_message = $2
                 WHERE id = $1",
            )
            .bind(verification_id)
            .bind(&failure_message)
            .execute(&state.db)
            .await
            .map_err(|db_err| db_internal_error("persist verifier error", db_err))?;

            let verification_changes = json!({
                "verification_id": { "before": Value::Null, "after": verification_id },
                "status": { "before": Value::Null, "after": "failed" },
                "compiler_version": { "before": Value::Null, "after": req.compiler_version },
                "error_message": { "before": Value::Null, "after": failure_message }
            });
            write_contract_audit_log(
                &state.db,
                AuditActionType::VerificationChanged,
                contract.id,
                contract.publisher_id,
                verification_changes,
                &ip_address,
            )
            .await
            .map_err(|db_err| db_internal_error("write verifier error audit log", db_err))?;

            if before_status != "failed" {
                let status_changes = json!({
                    "status": { "before": before_status, "after": "failed" },
                    "is_verified": { "before": contract.is_verified, "after": contract.is_verified }
                });
                write_contract_audit_log(
                    &state.db,
                    AuditActionType::VerificationChanged,
                    contract.id,
                    contract.publisher_id,
                    status_changes,
                    &ip_address,
                )
                .await
                .map_err(|db_err| {
                    db_internal_error("write verifier error status audit log", db_err)
                })?;
            }

            Err(ApiError::unprocessable(
                "VerificationFailed",
                failure_message,
            ))
        }
    }
}

#[utoipa::path(
    patch,
    path = "/api/contracts/{id}/metadata",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = UpdateContractMetadataRequest,
    responses(
        (status = 200, description = "Metadata updated successfully", body = Contract),
        (status = 404, description = "Contract not found"),
        (status = 400, description = "Invalid input")
    ),
    tag = "Contracts"
)]
pub async fn update_contract_metadata(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<UpdateContractMetadataRequest>,
) -> ApiResult<Json<Contract>> {
    if req.name.is_none()
        && req.description.is_none()
        && req.category.is_none()
        && req.tags.is_none()
    {
        return Err(ApiError::bad_request(
            "InvalidRequest",
            "At least one metadata field must be provided",
        ));
    }

    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let before: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("fetch contract for metadata update", err),
        })?;

    let after: Contract = sqlx::query_as(
        "UPDATE contracts
            SET name = COALESCE($2, name),
                description = COALESCE($3, description),
                category = COALESCE($4, category),
                tags = COALESCE($5, tags),
                updated_at = NOW()
          WHERE id = $1
          RETURNING *",
    )
    .bind(contract_uuid)
    .bind(req.name.as_deref())
    .bind(req.description.as_deref())
    .bind(req.category.as_deref())
    .bind(req.tags.as_ref())
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("update contract metadata", err))?;

    let mut changes = serde_json::Map::new();
    if before.name != after.name {
        changes.insert(
            "name".to_string(),
            json!({ "before": before.name, "after": after.name }),
        );
    }
    if before.description != after.description {
        changes.insert(
            "description".to_string(),
            json!({ "before": before.description, "after": after.description }),
        );
    }
    if before.category != after.category {
        changes.insert(
            "category".to_string(),
            json!({ "before": before.category, "after": after.category }),
        );
    }
    if before.tags != after.tags {
        changes.insert(
            "tags".to_string(),
            json!({ "before": before.tags, "after": after.tags }),
        );
    }

    if !changes.is_empty() {
        let changes_value = Value::Object(changes.clone());
        write_contract_audit_log(
            &state.db,
            AuditActionType::MetadataUpdated,
            after.id,
            req.user_id.unwrap_or(before.publisher_id),
            changes_value.clone(),
            &extract_ip_address(&headers),
        )
        .await
        .map_err(|err| db_internal_error("write metadata_updated audit log", err))?;

        let _ = analytics::record_event(
            &state.db,
            AnalyticsEventType::ContractUpdated,
            Some(after.id),
            Some(after.publisher_id),
            None,
            Some(&after.network),
            Some(json!({ "changes": changes })),
        )
        .await;

        state.contract_events.publish(ContractEventEnvelope::metadata_updated(
            &after,
            changes_value,
            ContractEventVisibility::Public,
        ));
    }

    Ok(Json(after))
}

#[utoipa::path(
    patch,
    path = "/api/contracts/{id}/publisher",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = ChangePublisherRequest,
    responses(
        (status = 200, description = "Publisher changed successfully", body = Contract),
        (status = 404, description = "Contract not found")
    ),
    tag = "Contracts"
)]
pub async fn change_contract_publisher(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<ChangePublisherRequest>,
) -> ApiResult<Json<Contract>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let before: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("fetch contract for publisher change", err),
        })?;

    let old_publisher_address: String =
        sqlx::query_scalar("SELECT stellar_address FROM publishers WHERE id = $1")
            .bind(before.publisher_id)
            .fetch_one(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch current publisher address", err))?;

    let new_publisher: Publisher = sqlx::query_as(
        "INSERT INTO publishers (stellar_address)
         VALUES ($1)
         ON CONFLICT (stellar_address) DO UPDATE SET stellar_address = EXCLUDED.stellar_address
         RETURNING *",
    )
    .bind(&req.publisher_address)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("upsert new publisher", err))?;

    let after: Contract = sqlx::query_as(
        "UPDATE contracts
            SET publisher_id = $2,
                updated_at = NOW()
          WHERE id = $1
          RETURNING *",
    )
    .bind(contract_uuid)
    .bind(new_publisher.id)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("update contract publisher", err))?;

    if before.publisher_id != after.publisher_id {
        let changes = json!({
            "publisher_id": { "before": before.publisher_id, "after": after.publisher_id },
            "publisher_address": { "before": old_publisher_address, "after": new_publisher.stellar_address }
        });
        write_contract_audit_log(
            &state.db,
            AuditActionType::PublisherChanged,
            after.id,
            req.user_id.unwrap_or(before.publisher_id),
            changes,
            &extract_ip_address(&headers),
        )
        .await
        .map_err(|err| db_internal_error("write publisher_changed audit log", err))?;
    }

    Ok(Json(after))
}

#[utoipa::path(
    patch,
    path = "/api/contracts/{id}/status",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = UpdateContractStatusRequest,
    responses(
        (status = 200, description = "Status updated successfully", body = Object),
        (status = 404, description = "Contract not found"),
        (status = 400, description = "Invalid status")
    ),
    tag = "Contracts"
)]
pub async fn update_contract_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ValidatedJson(req): ValidatedJson<UpdateContractStatusRequest>,
) -> ApiResult<Json<Value>> {
    let normalized_status = req.status.to_ascii_lowercase();
    if normalized_status != "pending"
        && normalized_status != "verified"
        && normalized_status != "failed"
    {
        return Err(ApiError::bad_request(
            "InvalidStatus",
            "status must be one of: pending, verified, failed",
        ));
    }

    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("fetch contract for status update", err),
        })?;

    let previous_status: Option<String> = sqlx::query_scalar(
        "SELECT status::text FROM verifications WHERE contract_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(contract_uuid)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch previous status for status update", err))?;

    let verified_at: Option<chrono::DateTime<chrono::Utc>> = if normalized_status == "verified" {
        Some(chrono::Utc::now())
    } else {
        None
    };
    let is_verified_after = normalized_status == "verified";

    let verification_id: Uuid = sqlx::query_scalar(
        "INSERT INTO verifications (contract_id, status, source_code, build_params, compiler_version, verified_at, error_message)
         VALUES ($1, $2::verification_status, NULL, NULL, NULL, $3, $4)
         RETURNING id",
    )
    .bind(contract_uuid)
    .bind(&normalized_status)
    .bind(verified_at)
    .bind(req.error_message.as_deref())
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("insert status verification row", err))?;

    let verified_at = if is_verified_after {
        Some(chrono::Utc::now())
    } else {
        contract.verified_at
    };

    sqlx::query("UPDATE contracts SET is_verified = $2, verified_at = COALESCE($3, verified_at), updated_at = NOW() WHERE id = $1")
        .bind(contract_uuid)
        .bind(is_verified_after)
        .bind(verified_at)
        .execute(&state.db)
        .await
        .map_err(|err| db_internal_error("update contract verification flag from status", err))?;

    let before_status = previous_status.unwrap_or_else(|| "pending".to_string());
    if before_status != normalized_status || contract.is_verified != is_verified_after {
        let changes = json!({
            "status": { "before": before_status, "after": normalized_status },
            "is_verified": { "before": contract.is_verified, "after": is_verified_after },
            "verification_id": { "before": Value::Null, "after": verification_id }
        });
        write_contract_audit_log(
            &state.db,
            AuditActionType::VerificationChanged,
            contract_uuid,
            req.user_id.unwrap_or(contract.publisher_id),
            changes,
            &extract_ip_address(&headers),
        )
        .await
        .map_err(|err| db_internal_error("write status_changed audit log", err))?;
    }

    if normalized_status == "verified" || normalized_status == "failed" {
        let interaction_type = if normalized_status == "verified" {
            "publish_success"
        } else {
            "publish_failed"
        };

        record_contract_interaction(
            &state.db,
            ContractInteractionInsert {
                contract_id: contract_uuid,
                target_contract_id: None,
                account: None,
                interaction_type,
                transaction_hash: None,
                method: Some("status_update"),
                parameters: None,
                return_value: None,
                timestamp: chrono::Utc::now(),
                network: &contract.network,
            },
        )
        .await
        .map_err(|err| db_internal_error("record status interaction", err))?;
    }

    let contract_after: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch contract after status update", err))?;

    state.contract_events.publish(ContractEventEnvelope::status_updated(
        &contract_after,
        normalized_status.clone(),
        is_verified_after,
        None,
        ContractEventVisibility::Public,
    ));

    if req.error_message.is_some() {
        state.contract_events.publish(ContractEventEnvelope::status_updated(
            &contract_after,
            normalized_status.clone(),
            is_verified_after,
            Some(json!({
                "error_message": req.error_message,
                "publisher_id": contract.publisher_id,
            })),
            ContractEventVisibility::Private,
        ));
    }

    Ok(Json(json!({
        "contract_id": contract_uuid,
        "verification_id": verification_id,
        "status": normalized_status,
        "is_verified": is_verified_after
    })))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/audit-log",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        AuditLogQuery
    ),
    responses(
        (status = 200, description = "Paginated audit logs for the contract", body = [ContractAuditLogEntry]),
        (status = 404, description = "Contract not found")
    ),
    tag = "Administration"
)]
pub async fn get_contract_audit_log(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<AuditLogQuery>,
) -> ApiResult<Json<Vec<ContractAuditLog>>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;
    let limit = params.limit.clamp(1, 500);
    let offset = params.offset.max(0);

    let _contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("check contract before audit log query", err),
        })?;
    ensure_contract_exists(
        &state,
        contract_uuid,
        &id,
        "check contract before audit log query",
    )
    .await?;

    let logs: Vec<ContractAuditLog> = sqlx::query_as(
        r#"
        SELECT id, contract_id, action_type, old_value, new_value, changed_by, "timestamp",
               previous_hash, hash, signature
          FROM contract_audit_log
         WHERE contract_id = $1
         ORDER BY "timestamp" DESC
         LIMIT $2 OFFSET $3
        "#,
    )
    .bind(contract_uuid)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch contract audit logs", err))?;

    Ok(Json(logs))
}

#[utoipa::path(
    get,
    path = "/api/admin/audit-logs",
    params(AuditLogQuery),
    responses(
        (status = 200, description = "Global audit logs (Admin only)", body = [ContractAuditLogEntry])
    ),
    tag = "Administration",
    security(("bearerAuth" = []))
)]
pub async fn get_all_audit_logs(
    State(state): State<AppState>,
    Query(params): Query<AuditLogQuery>,
) -> ApiResult<Json<Vec<ContractAuditLog>>> {
    let limit = params.limit.clamp(1, 500);
    let offset = params.offset.max(0);

    let logs: Vec<ContractAuditLog> = sqlx::query_as(
        r#"
        SELECT id, contract_id, action_type, old_value, new_value, changed_by, "timestamp",
               previous_hash, hash, signature
          FROM contract_audit_log
         ORDER BY "timestamp" DESC
         LIMIT $1 OFFSET $2
        "#,
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch all audit logs", err))?;

    Ok(Json(logs))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/deployments/status",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "Current deployment status", body = Object)
    ),
    tag = "Deployments"
)]
pub async fn get_deployment_status() -> impl IntoResponse {
    planned_not_implemented_response()
}

#[utoipa::path(
    post,
    path = "/api/deployments/green",
    responses(
        (status = 202, description = "Green deployment triggered", body = Object)
    ),
    tag = "Deployments"
)]
pub async fn deploy_green() -> impl IntoResponse {
    planned_not_implemented_response()
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/performance",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "Performance metrics and anomalies", body = Object)
    ),
    tag = "Analytics"
)]
pub async fn get_contract_performance() -> impl IntoResponse {
    Json(json!({"performance": {}}))
}

// ─── Contract interaction history (Issue #46) ─────────────────────────────────

/// GET /api/contracts/:id/interactions — list with optional filters (account, method, date range).
#[utoipa::path(
    get,
    path = "/api/contracts/{id}/interactions",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        InteractionsQueryParams
    ),
    responses(
        (status = 200, description = "List of contract interactions", body = InteractionsListResponse),
        (status = 404, description = "Contract not found")
    ),
    tag = "Analytics"
)]
pub async fn get_contract_interactions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<InteractionsQueryParams>,
) -> ApiResult<Json<Value>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    ensure_contract_exists(&state, contract_uuid, &id, "get contract for interactions").await?;

    if let Some(days) = params.days {
        let days = days.clamp(1, 365);

        let series_rows: Vec<(chrono::NaiveDate, String, i64)> = sqlx::query_as(
            r#"
            SELECT day, interaction_type, SUM(count)::bigint AS count
            FROM contract_interaction_daily_aggregates
            WHERE contract_id = $1
              AND day >= CURRENT_DATE - ($2::int - 1)
              AND ($3::text IS NULL OR interaction_type = $3)
              AND ($4::network_type IS NULL OR network = $4)
            GROUP BY day, interaction_type
            ORDER BY day ASC, interaction_type ASC
            "#,
        )
        .bind(contract_uuid)
        .bind(days as i32)
        .bind(params.interaction_type.as_deref())
        .bind(params.network.as_ref())
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch interaction time series", err))?;

        let (interactions_this_week, interactions_last_week): (i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COALESCE(
                    SUM(count) FILTER (WHERE day >= CURRENT_DATE - INTERVAL '6 days'),
                    0
                )::bigint AS interactions_this_week,
                COALESCE(
                    SUM(count) FILTER (
                        WHERE day >= CURRENT_DATE - INTERVAL '13 days'
                          AND day < CURRENT_DATE - INTERVAL '6 days'
                    ),
                    0
                )::bigint AS interactions_last_week
            FROM contract_interaction_daily_aggregates
            WHERE contract_id = $1
              AND ($2::text IS NULL OR interaction_type = $2)
              AND ($3::network_type IS NULL OR network = $3)
            "#,
        )
        .bind(contract_uuid)
        .bind(params.interaction_type.as_deref())
        .bind(params.network.as_ref())
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch weekly interaction trend", err))?;

        let response = InteractionTimeSeriesResponse {
            contract_id: contract_uuid,
            days,
            interactions_this_week,
            interactions_last_week,
            is_trending: (interactions_this_week as f64) > (interactions_last_week as f64 * 1.5),
            series: series_rows
                .into_iter()
                .map(
                    |(date, interaction_type, count)| InteractionTimeSeriesPoint {
                        date,
                        interaction_type,
                        count,
                    },
                )
                .collect(),
        };

        return Ok(Json(json!(response)));
    }

    let limit = params.limit.clamp(1, 100);

    // Cursor logic
    let cursor = params.cursor.as_ref().and_then(|c| Cursor::decode(c).ok());

    let offset = if cursor.is_some() {
        0
    } else {
        params.offset.max(0)
    };

    let from_ts = params
        .from_timestamp
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let to_ts = params
        .to_timestamp
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let rows: Vec<shared::ContractInteraction> = sqlx::query_as(
        r#"
        SELECT id, contract_id, user_address, interaction_type, transaction_hash,
               method, parameters, return_value, interaction_timestamp, interaction_count, network, created_at
        FROM contract_interactions
        WHERE contract_id = $1
          AND ($2::text IS NULL OR user_address = $2)
          AND ($3::text IS NULL OR method = $3)
          AND ($4::timestamptz IS NULL OR created_at >= $4)
          AND ($5::timestamptz IS NULL OR created_at <= $5)
          AND ($6::text IS NULL OR interaction_type = $6)
          AND ($7::network_type IS NULL OR network = $7)
          -- Cursor logic: tie-break with id
          AND ($8::timestamptz IS NULL OR (created_at < $8 OR (created_at = $8 AND id < $9)))
        ORDER BY created_at DESC, id DESC
        LIMIT $10 OFFSET $11
        "#,
    )
    .bind(contract_uuid)
    .bind(params.account.as_deref())
    .bind(params.method.as_deref())
    .bind(from_ts)
    .bind(to_ts)
    .bind(params.interaction_type.as_deref())
    .bind(params.network.as_ref())
    .bind(cursor.as_ref().map(|c| c.timestamp))
    .bind(cursor.as_ref().map(|c| c.id))
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("list contract interactions", err))?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM contract_interactions
        WHERE contract_id = $1
          AND ($2::text IS NULL OR user_address = $2)
          AND ($3::text IS NULL OR method = $3)
          AND ($4::timestamptz IS NULL OR created_at >= $4)
          AND ($5::timestamptz IS NULL OR created_at <= $5)
          AND ($6::text IS NULL OR interaction_type = $6)
          AND ($7::network_type IS NULL OR network = $7)
        "#,
    )
    .bind(contract_uuid)
    .bind(params.account.as_deref())
    .bind(params.method.as_deref())
    .bind(from_ts)
    .bind(to_ts)
    .bind(params.interaction_type.as_deref())
    .bind(params.network.as_ref())
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("count contract interactions", err))?;

    let items: Vec<ContractInteractionResponse> = rows
        .into_iter()
        .map(|r| ContractInteractionResponse {
            id: r.id,
            account: r.user_address,
            method: r.method,
            parameters: r.parameters,
            return_value: r.return_value,
            transaction_hash: r.transaction_hash,
            created_at: r.created_at,
        })
        .collect();

    let next_cursor = if items.len() >= limit as usize {
        items
            .last()
            .map(|last| Cursor::new(last.created_at, last.id).encode())
    } else {
        None
    };
    let prev_cursor = if params.cursor.is_some() || offset > 0 {
        items
            .first()
            .map(|first| Cursor::new(first.created_at, first.id).encode())
    } else {
        None
    };

    Ok(Json(json!(InteractionsListResponse {
        items,
        total,
        limit,
        offset,
        next_cursor,
        prev_cursor,
    })))
}

/// POST /api/contracts/:id/interactions — ingest one interaction.
#[utoipa::path(
    post,
    path = "/api/contracts/{id}/interactions",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = CreateInteractionRequest,
    responses(
        (status = 201, description = "Interaction logged", body = Object),
        (status = 404, description = "Contract not found")
    ),
    tag = "Analytics"
)]
pub async fn post_contract_interaction(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ValidatedJson(req): ValidatedJson<CreateInteractionRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let contract_network =
        fetch_contract_network(&state, contract_uuid, &id, "get contract for interaction").await?;

    let interaction_type =
        parse_interaction_type(req.interaction_type.as_deref(), req.method.as_deref())?;
    let created_at = req.timestamp.unwrap_or_else(chrono::Utc::now);
    let network = req.network.unwrap_or(contract_network);
    let target_contract_id = resolve_call_target_contract(
        &state.db,
        req.target_contract_id.as_deref(),
        req.parameters.as_ref(),
    )
    .await?;
    let interaction_id = record_contract_interaction(
        &state.db,
        ContractInteractionInsert {
            contract_id: contract_uuid,
            target_contract_id,
            account: req.account.as_deref(),
            interaction_type: &interaction_type,
            transaction_hash: req.transaction_hash.as_deref(),
            method: req.method.as_deref(),
            parameters: req.parameters.as_ref(),
            return_value: req.return_value.as_ref(),
            timestamp: created_at,
            network: &network,
        },
    )
    .await
    .map_err(|err| db_internal_error("insert contract interaction", err))?;

    tracing::info!(
        contract_id = %id,
        interaction_id = %interaction_id,
        "contract interaction logged"
    );

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": interaction_id })),
    ))
}

/// POST /api/contracts/:id/interactions/batch — ingest multiple interactions.
#[utoipa::path(
    post,
    path = "/api/contracts/{id}/interactions/batch",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    request_body = CreateInteractionBatchRequest,
    responses(
        (status = 201, description = "Batch of interactions logged", body = Object),
        (status = 404, description = "Contract not found")
    ),
    tag = "Analytics"
)]
pub async fn post_contract_interactions_batch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ValidatedJson(req): ValidatedJson<CreateInteractionBatchRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let contract_network = fetch_contract_network(
        &state,
        contract_uuid,
        &id,
        "get contract for interactions batch",
    )
    .await?;

    let mut ids = Vec::with_capacity(req.interactions.len());
    for i in &req.interactions {
        let interaction_type =
            parse_interaction_type(i.interaction_type.as_deref(), i.method.as_deref())?;
        let created_at = i.timestamp.unwrap_or_else(chrono::Utc::now);
        let network = i
            .network
            .clone()
            .unwrap_or_else(|| contract_network.clone());
        let target_contract_id = resolve_call_target_contract(
            &state.db,
            i.target_contract_id.as_deref(),
            i.parameters.as_ref(),
        )
        .await?;
        let interaction_id = record_contract_interaction(
            &state.db,
            ContractInteractionInsert {
                contract_id: contract_uuid,
                target_contract_id,
                account: i.account.as_deref(),
                interaction_type: &interaction_type,
                transaction_hash: i.transaction_hash.as_deref(),
                method: i.method.as_deref(),
                parameters: i.parameters.as_ref(),
                return_value: i.return_value.as_ref(),
                timestamp: created_at,
                network: &network,
            },
        )
        .await
        .map_err(|err| db_internal_error("insert contract interaction batch", err))?;
        ids.push(interaction_id);
    }

    tracing::info!(
        contract_id = %id,
        count = ids.len(),
        "contract interactions batch logged"
    );

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "ids": ids }))))
}

pub async fn route_not_found() -> impl IntoResponse {
    ApiError::not_found("ROUTE_NOT_FOUND", "Route not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use prometheus::Registry;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_health_check_shutdown_returns_503() {
        unsafe {
            std::env::set_var("JWT_SECRET", "abcdefghijklmnopqrstuvwxyz012345");
        }

        let is_shutting_down = Arc::new(AtomicBool::new(true));

        // Connect lazy so it doesn't fail immediately without a DB
        let db = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost:5432/soroban_registry")
            .unwrap();
        let registry = Registry::new();
        let (job_engine, _rx) = soroban_batch::engine::JobEngine::new();
        let state = AppState::new(db, registry, Arc::new(job_engine), is_shutting_down);

        let (status, json) = health_check(State(state)).await;

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let value = json.0;
        assert_eq!(value["status"], "shutting_down");
    }

    #[test]
    fn split_audit_changes_extracts_before_after() {
        let changes = json!({
            "name": { "before": "old-name", "after": "new-name" },
            "description": { "before": "old-desc", "after": "new-desc" },
            "is_verified": { "before": false, "after": true }
        });

        let (old_value, new_value) = split_audit_changes(&changes, "127.0.0.1");

        let old = old_value.expect("old_value should be populated");
        let new = new_value.expect("new_value should be populated");
        assert_eq!(old["name"], "old-name");
        assert_eq!(old["description"], "old-desc");
        assert_eq!(old["is_verified"], false);
        assert_eq!(new["name"], "new-name");
        assert_eq!(new["description"], "new-desc");
        assert_eq!(new["is_verified"], true);
        assert_eq!(new["_ip_address"], "127.0.0.1");
    }

    #[test]
    fn split_audit_changes_preserves_non_diff_payload() {
        let changes = json!({
            "status": "verified",
            "verification_id": "abc123"
        });

        let (old_value, new_value) = split_audit_changes(&changes, "unknown");

        assert!(old_value.is_none());
        let new = new_value.expect("new_value should be populated");
        assert_eq!(new["status"], "verified");
        assert_eq!(new["verification_id"], "abc123");
        assert_eq!(new["_ip_address"], "unknown");
    }

    #[test]
    fn timestamp_sort_helpers_cover_all_timestamp_fields() {
        let now = chrono::Utc::now();
        let contract = Contract {
            id: Uuid::nil(),
            contract_id: "C123".to_string(),
            wasm_hash: "hash".to_string(),
            name: "Demo".to_string(),
            description: None,
            publisher_id: Uuid::nil(),
            network: Network::Testnet,
            is_verified: true,
            category: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now + chrono::TimeDelta::seconds(10),
            verified_at: Some(now + chrono::TimeDelta::seconds(20)),
            last_accessed_at: Some(now + chrono::TimeDelta::seconds(30)),
            health_score: 0,
            is_maintenance: false,
            logical_id: None,
            network_configs: None,
        };

        assert_eq!(sort_timestamp_column(&shared::SortBy::CreatedAt), Some("c.created_at"));
        assert_eq!(sort_timestamp_column(&shared::SortBy::UpdatedAt), Some("c.updated_at"));
        assert_eq!(sort_timestamp_column(&shared::SortBy::VerifiedAt), Some("c.verified_at"));
        assert_eq!(
            sort_timestamp_column(&shared::SortBy::LastAccessedAt),
            Some("c.last_accessed_at")
        );
        assert_eq!(
            contract_timestamp_for_sort(&contract, &shared::SortBy::VerifiedAt),
            contract.verified_at
        );
        assert_eq!(
            contract_timestamp_for_sort(&contract, &shared::SortBy::LastAccessedAt),
            contract.last_accessed_at
        );
    }

    #[test]
    fn derive_network_status_marks_rpc_failures_offline_and_stale_states_degraded() {
        let now = chrono::Utc::now();
        let healthy_snapshot = IndexerStateSnapshot {
            last_indexed_ledger_height: 42,
            indexed_at: now,
            consecutive_failures: 0,
            error_message: None,
        };
        let stale_snapshot = IndexerStateSnapshot {
            last_indexed_ledger_height: 42,
            indexed_at: now - chrono::Duration::minutes(NETWORK_STALE_AFTER_MINUTES + 1),
            consecutive_failures: 0,
            error_message: None,
        };
        let failing_snapshot = IndexerStateSnapshot {
            last_indexed_ledger_height: 42,
            indexed_at: now,
            consecutive_failures: NETWORK_OFFLINE_FAILURE_THRESHOLD,
            error_message: Some("RPC unavailable".to_string()),
        };

        assert_eq!(
            derive_network_status(true, Some(&healthy_snapshot), now),
            (NetworkStatus::Online, None)
        );
        assert_eq!(
            derive_network_status(true, Some(&stale_snapshot), now).0,
            NetworkStatus::Degraded
        );
        assert_eq!(
            derive_network_status(false, Some(&healthy_snapshot), now).0,
            NetworkStatus::Offline
        );
        assert_eq!(
            derive_network_status(true, Some(&failing_snapshot), now).0,
            NetworkStatus::Offline
        );
    }
}
