pub mod reviews;

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
use shared::{
    pagination::Cursor, AdvancedSearchRequest, AnalyticsEventType, AuditActionType,
    ChangePublisherRequest, Contract, ContractAnalyticsResponse, ContractAuditLog,
    ContractChangelogEntry, ContractChangelogResponse, ContractGetResponse,
    ContractInteractionResponse, ContractSearchParams, ContractSource, ContractVersion,
    CreateContractVersionRequest, CreateInteractionBatchRequest, CreateInteractionRequest,
    DeploymentStats, FavoriteSearch, FieldOperator, InteractionTimeSeriesPoint,
    InteractionTimeSeriesResponse, InteractionsListResponse, InteractionsQueryParams,
    InteractorStats, Network, NetworkConfig, NetworkEndpoints, NetworkInfo, NetworkListResponse,
    NetworkStatus, PaginatedResponse, PublishRequest, Publisher, QueryCondition, QueryNode,
    QueryOperator, SaveFavoriteSearchRequest, SearchSuggestion, SearchSuggestionsResponse, SemVer,
    TimelineEntry, TopUser, TrendingParams, UpdateContractMetadataRequest,
    UpdateContractStatusRequest, VerifyRequest,
};
use sqlx::QueryBuilder;
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
    onchain_verification::OnChainVerifier,
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

fn contract_timestamp_for_sort(
    contract: &Contract,
    sort_by: &shared::SortBy,
) -> Option<chrono::DateTime<chrono::Utc>> {
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
            |(id, name, network_type, rpc_env, explorer_env, friendbot_env)| {
                StaticNetworkDefinition {
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
                }
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
        let stale =
            now - snapshot.indexed_at > chrono::Duration::minutes(NETWORK_STALE_AFTER_MINUTES);

        if snapshot.consecutive_failures >= NETWORK_OFFLINE_FAILURE_THRESHOLD {
            return (
                NetworkStatus::Offline,
                snapshot.error_message.clone().or_else(|| {
                    Some(format!(
                        "{} consecutive indexer failures",
                        snapshot.consecutive_failures
                    ))
                }),
            );
        }

        if stale || snapshot.consecutive_failures >= NETWORK_DEGRADED_FAILURE_THRESHOLD {
            return (
                NetworkStatus::Degraded,
                if stale {
                    Some("Indexer status is stale".to_string())
                } else {
                    snapshot.error_message.clone().or_else(|| {
                        Some(format!(
                            "{} consecutive indexer failures",
                            snapshot.consecutive_failures
                        ))
                    })
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
            consecutive_failures: snapshot
                .as_ref()
                .map(|s| s.consecutive_failures)
                .unwrap_or(0),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, sqlx::FromRow, utoipa::ToSchema)]
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

const DEFAULT_CONTRACT_LIST_LIMIT: i64 = 50;
const MAX_CONTRACT_LIST_LIMIT: i64 = 1000;

fn validate_contract_list_pagination(
    params: &ContractSearchParams,
) -> Result<(i64, i64, i64), ApiError> {
    let limit = params.limit.unwrap_or(DEFAULT_CONTRACT_LIST_LIMIT);
    if !(1..=MAX_CONTRACT_LIST_LIMIT).contains(&limit) {
        return Err(ApiError::bad_request(
            "InvalidPaginationLimit",
            format!(
                "Invalid `limit` value {limit}. Expected an integer between 1 and {MAX_CONTRACT_LIST_LIMIT}."
            ),
        ));
    }

    if let Some(offset) = params.offset {
        if offset < 0 {
            return Err(ApiError::bad_request(
                "InvalidPaginationOffset",
                format!("Invalid `offset` value {offset}. Expected a non-negative integer."),
            ));
        }

        let page = (offset / limit) + 1;
        return Ok((limit, offset, page));
    }

    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1).max(0) * limit;
    Ok((limit, offset, page))
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

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = Object),
        (status = 503, description = "Service is unavailable or degraded", body = Object)
    ),
    tag = "Observability"
)]
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
                "version": VERSION,
                "timestamp": now,
                "uptime_secs": uptime
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "version": VERSION,
            "timestamp": now
        })),
    )
}

pub async fn health_check_live(State(state): State<AppState>) -> StatusCode {
    if state
        .is_shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    }
}

pub async fn health_check_ready(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let uptime = state.started_at.elapsed().as_secs();
    let now = chrono::Utc::now().to_rfc3339();

    if state
        .is_shutting_down
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        tracing::warn!(
            uptime_secs = uptime,
            "readiness check failing — shutting down"
        );
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "reason": "shutting_down",
                "version": VERSION,
                "timestamp": now
            })),
        );
    }

    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    if db_ok {
        tracing::info!(uptime_secs = uptime, "readiness check passed");
        (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "version": VERSION,
                "timestamp": now
            })),
        )
    } else {
        tracing::warn!(
            uptime_secs = uptime,
            "readiness check failed — db unreachable"
        );
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "reason": "database_unavailable",
                "version": VERSION,
                "timestamp": now
            })),
        )
    }
}

pub async fn health_check_detailed(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let uptime = state.started_at.elapsed().as_secs();
    let now = chrono::Utc::now().to_rfc3339();

    let is_shutting_down = state
        .is_shutting_down
        .load(std::sync::atomic::Ordering::SeqCst);

    let db_health = if sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok()
    {
        json!({"status": "healthy"})
    } else {
        json!({"status": "unhealthy", "error": "database connection failed"})
    };

    let cache_config = state.cache.config();
    let cache_health = json!({
        "status": "healthy",
        "enabled": cache_config.enabled,
        "max_capacity": cache_config.max_capacity
    });

    let overall_status = if is_shutting_down {
        "unhealthy"
    } else if db_health["status"] == "unhealthy" {
        "degraded"
    } else {
        "healthy"
    };

    let status_code = if is_shutting_down || db_health["status"] == "unhealthy" {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (
        status_code,
        Json(json!({
            "status": overall_status,
            "version": VERSION,
            "timestamp": now,
            "uptime_secs": uptime,
            "dependencies": {
                "database": db_health,
                "cache": cache_health
            }
        })),
    )
}

#[utoipa::path(
    get,
    path = "/api/stats",
    responses(
        (status = 200, description = "Global registry statistics", body = Object, example = json!({"total_contracts": 150, "verified_contracts": 120, "total_publishers": 45}))
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
            .put(
                "search",
                &cache_key,
                serialized,
                Some(Duration::from_secs(60)),
            )
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
    claims: Option<shared::AuthClaims>,
    params: Result<Query<ContractSearchParams>, QueryRejection>,
) -> axum::response::Response {
    let search_started_at = std::time::Instant::now();
    let Query(params) = match params {
        Ok(q) => q,
        Err(err) => return map_query_rejection(err).into_response(),
    };

    let (limit, offset, page) = match validate_contract_list_pagination(&params) {
        Ok(values) => values,
        Err(err) => return err.into_response(),
    };

    let cursor = params.cursor.as_ref().and_then(|c| Cursor::decode(c).ok());
    let (page, offset) = if cursor.is_some() {
        (1, 0)
    } else {
        (page, offset)
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
    let direction = if sort_order == shared::SortOrder::Asc {
        "ASC"
    } else {
        "DESC"
    };
    let direction_op = if sort_order == shared::SortOrder::Asc {
        ">"
    } else {
        "<"
    };
    let id_direction = if sort_order == shared::SortOrder::Asc {
        "ASC"
    } else {
        "DESC"
    };

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
        // Clean query for tsquery
        let cleaned_q = q.replace('\'', "''");
        query.push_str(&format!(
            "        ts_rank_cd(c.search_vector, plainto_tsquery('english', '{}')) as text_relevance,\n",
            cleaned_q
        ));
    } else {
        query.push_str("        0.0 as text_relevance,\n");
    }

    query.push_str(&format!(
        "        LOG(1 + cs.interaction_count + 2 * cs.deployment_count) as popularity_score,
        1.0 / (1.0 + EXTRACT(DAYS FROM (NOW() - c.updated_at)) / 30.0) as recency_score,
        (cs.avg_rating / 5.0) * LOG(1.0 + cs.review_count) as rating_score,
        LOG(1 + cs.user_interaction_count) as personal_boost
    FROM contracts c
    JOIN contract_stats cs ON c.id = cs.id
    WHERE (c.visibility = 'public'"
    ));

    let mut count_query =
        String::from("SELECT COUNT(*) FROM contracts c WHERE (c.visibility = 'public'");

    if let Some(claims) = claims {
        let visibility_clause = format!(
            " OR (c.visibility = 'private' AND c.organization_id IN (SELECT organization_id FROM organization_members om JOIN publishers p ON om.publisher_id = p.id WHERE p.stellar_address = '{}'))",
            claims.sub.replace('\'', "''")
        );
        query.push_str(&visibility_clause);
        count_query.push_str(&visibility_clause);
    }
    query.push_str(")");
    count_query.push_str(")");

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
        shared::SortBy::Deployments => "c.deployment_count".to_string(),
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
            return db_internal_error("list contracts with ranking", err).into_response();
        }
    };

    let total: i64 = match count_query.build_query_scalar().fetch_one(&state.db).await {
        Ok(v) => v,
        Err(err) => return db_internal_error("count filtered contracts", err).into_response(),
    };

    if params.query.is_some() {
        observe_search_query(
            "contracts",
            search_started_at,
            params.query.as_deref(),
            limit,
        );
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
    claims: Option<shared::AuthClaims>,
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

    // Visibility check
    if contract.visibility == shared::VisibilityType::Private {
        let is_member = if let Some(ref claims) = claims {
            if let Some(org_id) = contract.organization_id {
                crate::org_handlers::check_org_role(
                    &state.pool,
                    org_id,
                    &claims.sub,
                    shared::OrganizationRole::Viewer,
                )
                .await
                .is_ok()
            } else {
                false
            }
        } else {
            false
        };

        if !is_member {
            return Err(ApiError::forbidden(
                "AccessDenied",
                "This contract is private and you do not have access to it",
            ));
        }
    }

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

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/contracts/:id/versions/:version  (Issue #486)
// ─────────────────────────────────────────────────────────────────────────────

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/versions/{version}",
    params(
        ("id" = String, Path, description = "Contract UUID or on-chain contract_id"),
        ("version" = String, Path, description = "Semantic version string (e.g. 1.2.3)")
    ),
    responses(
        (status = 200, description = "Version details", body = ContractVersion),
        (status = 400, description = "Invalid contract ID"),
        (status = 404, description = "Version not found")
    ),
    tag = "Versions"
)]
pub async fn get_specific_contract_version(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
) -> ApiResult<Json<ContractVersion>> {
    let (contract_uuid, _) = fetch_contract_identity(&state, &id).await?;

    let version_row: Option<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2",
    )
    .bind(contract_uuid)
    .bind(&version)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get specific contract version", err))?;

    version_row.map(Json).ok_or_else(|| {
        ApiError::not_found(
            "VersionNotFound",
            format!("Version '{}' not found for contract {}", version, id),
        )
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /api/contracts/:id/versions/compare?from=v1&to=v2  (Issue #486)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct VersionCompareQuery {
    /// The base version to compare from
    pub from: String,
    /// The target version to compare to
    pub to: String,
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/versions/compare",
    params(
        ("id" = String, Path, description = "Contract UUID or on-chain contract_id"),
        VersionCompareQuery
    ),
    responses(
        (status = 200, description = "Version comparison result", body = shared::VersionCompareResponse),
        (status = 400, description = "Invalid contract ID or missing query params"),
        (status = 404, description = "One or both versions not found")
    ),
    tag = "Versions"
)]
pub async fn compare_contract_versions(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<VersionCompareQuery>,
) -> ApiResult<Json<shared::VersionCompareResponse>> {
    let (contract_uuid, _) = fetch_contract_identity(&state, &id).await?;

    let from_row: Option<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2",
    )
    .bind(contract_uuid)
    .bind(&params.from)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch from-version for compare", err))?;

    let from_row = from_row.ok_or_else(|| {
        ApiError::not_found(
            "VersionNotFound",
            format!("Version '{}' not found for contract {}", params.from, id),
        )
    })?;

    let to_row: Option<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2",
    )
    .bind(contract_uuid)
    .bind(&params.to)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch to-version for compare", err))?;

    let to_row = to_row.ok_or_else(|| {
        ApiError::not_found(
            "VersionNotFound",
            format!("Version '{}' not found for contract {}", params.to, id),
        )
    })?;

    let wasm_changed = from_row.wasm_hash != to_row.wasm_hash;

    let mut differences: Vec<shared::VersionFieldDiff> = Vec::new();

    if from_row.wasm_hash != to_row.wasm_hash {
        differences.push(shared::VersionFieldDiff {
            field: "wasm_hash".to_string(),
            from_value: Some(json!(from_row.wasm_hash)),
            to_value: Some(json!(to_row.wasm_hash)),
        });
    }
    if from_row.source_url != to_row.source_url {
        differences.push(shared::VersionFieldDiff {
            field: "source_url".to_string(),
            from_value: from_row.source_url.as_ref().map(|v| json!(v)),
            to_value: to_row.source_url.as_ref().map(|v| json!(v)),
        });
    }
    if from_row.commit_hash != to_row.commit_hash {
        differences.push(shared::VersionFieldDiff {
            field: "commit_hash".to_string(),
            from_value: from_row.commit_hash.as_ref().map(|v| json!(v)),
            to_value: to_row.commit_hash.as_ref().map(|v| json!(v)),
        });
    }
    if from_row.release_notes != to_row.release_notes {
        differences.push(shared::VersionFieldDiff {
            field: "release_notes".to_string(),
            from_value: from_row.release_notes.as_ref().map(|v| json!(v)),
            to_value: to_row.release_notes.as_ref().map(|v| json!(v)),
        });
    }
    if from_row.change_notes != to_row.change_notes {
        differences.push(shared::VersionFieldDiff {
            field: "change_notes".to_string(),
            from_value: from_row.change_notes.as_ref().map(|v| json!(v)),
            to_value: to_row.change_notes.as_ref().map(|v| json!(v)),
        });
    }

    Ok(Json(shared::VersionCompareResponse {
        contract_id: contract_uuid,
        from_version: from_row,
        to_version: to_row,
        differences,
        wasm_changed,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /api/admin/contracts/:id/versions/:version/revert  (Issue #486)
// Admin-only: revert a contract to a previous version by creating a new version
// that carries the same wasm_hash as the target, marked as a revert.
// ─────────────────────────────────────────────────────────────────────────────

#[utoipa::path(
    post,
    path = "/api/admin/contracts/{id}/versions/{version}/revert",
    params(
        ("id" = String, Path, description = "Contract UUID or on-chain contract_id"),
        ("version" = String, Path, description = "Version to revert to")
    ),
    request_body = shared::RevertVersionRequest,
    responses(
        (status = 201, description = "Revert version created", body = ContractVersion),
        (status = 400, description = "Invalid input"),
        (status = 404, description = "Contract or version not found")
    ),
    tag = "Versions"
)]
pub async fn revert_contract_version(
    State(state): State<AppState>,
    Path((id, target_version)): Path<(String, String)>,
    headers: HeaderMap,
    Json(req): Json<shared::RevertVersionRequest>,
) -> ApiResult<Json<ContractVersion>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;

    // Fetch the version we are reverting to.
    let target: Option<ContractVersion> = sqlx::query_as(
        "SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2",
    )
    .bind(contract_uuid)
    .bind(&target_version)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch target version for revert", err))?;

    let target = target.ok_or_else(|| {
        ApiError::not_found(
            "VersionNotFound",
            format!(
                "Version '{}' not found for contract {}",
                target_version, contract_id
            ),
        )
    })?;

    // Determine the current latest version so we can compute the next patch bump.
    let existing_versions: Vec<String> =
        sqlx::query_scalar("SELECT version FROM contract_versions WHERE contract_id = $1")
            .bind(contract_uuid)
            .fetch_all(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch versions for revert bump", err))?;

    let new_version = {
        let mut parsed: Vec<SemVer> = existing_versions
            .iter()
            .filter_map(|v| SemVer::parse(v))
            .collect();
        parsed.sort();
        match parsed.last() {
            Some(latest) => {
                // Bump the patch component of the latest version.
                format!("{}.{}.{}", latest.major, latest.minor, latest.patch + 1)
            }
            None => "0.0.1".to_string(),
        }
    };

    let change_notes = req.change_notes.unwrap_or_else(|| {
        format!("Reverted to version {}", target_version)
    });

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|err| db_internal_error("begin revert transaction", err))?;

    let version_row: ContractVersion = sqlx::query_as(
        "INSERT INTO contract_versions \
            (contract_id, version, wasm_hash, source_url, commit_hash, release_notes, change_notes, is_revert, reverted_from) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, $8) \
         RETURNING *",
    )
    .bind(contract_uuid)
    .bind(&new_version)
    .bind(&target.wasm_hash)
    .bind(&target.source_url)
    .bind(&target.commit_hash)
    .bind(Option::<String>::None) // release_notes left empty for reverts
    .bind(&change_notes)
    .bind(&target_version)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| db_internal_error("insert revert version", err))?;

    sqlx::query("UPDATE contracts SET current_version = $2 WHERE id = $1")
        .bind(contract_uuid)
        .bind(&new_version)
        .execute(&mut *tx)
        .await
        .map_err(|err| db_internal_error("update current_version on revert", err))?;

    tx.commit()
        .await
        .map_err(|err| db_internal_error("commit revert transaction", err))?;

    write_contract_audit_log(
        &state.db,
        AuditActionType::Rollback,
        contract_uuid,
        req.admin_id,
        json!({
            "reverted_to_version": target_version,
            "new_version": new_version,
            "change_notes": change_notes
        }),
        &extract_ip_address(&headers),
    )
    .await
    .map_err(|err| db_internal_error("write revert audit log", err))?;

    state.cache.invalidate_abi(&contract_id).await;
    state.cache.invalidate_abi(&contract_uuid.to_string()).await;

    Ok(Json(version_row))
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct UploadContractSourceRequest {
    pub source_base64: String,
    pub source_format: String,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ContractSourceResponse {
    pub id: Uuid,
    pub contract_version_id: Uuid,
    pub source_format: String,
    pub storage_backend: String,
    pub storage_key: String,
    pub source_hash: String,
    pub source_size: i64,
    pub source_base64: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::IntoParams)]
pub struct ContractSourceQuery {
    #[serde(default)]
    pub source_format: Option<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ContractSourceDiffResponse {
    pub base_version: String,
    pub target_version: String,
    pub source_format: String,
    pub diff: String,
}

#[utoipa::path(
    post,
    path = "/api/contracts/{id}/versions/{version}/source",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        ("version" = String, Path, description = "Contract version")
    ),
    request_body = UploadContractSourceRequest,
    responses(
        (status = 201, description = "Source uploaded", body = ContractSourceResponse),
        (status = 400, description = "Invalid input"),
        (status = 404, description = "Contract version not found")
    ),
    tag = "Source"
)]
pub async fn upload_contract_source(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
    Json(req): Json<UploadContractSourceRequest>,
) -> ApiResult<Json<ContractSourceResponse>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;

    let version_row: Option<ContractVersion> =
        sqlx::query_as("SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2")
            .bind(contract_uuid)
            .bind(&version)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch contract version", err))?;

    let version_row = version_row.ok_or_else(|| {
        ApiError::not_found(
            "ContractVersionNotFound",
            format!(
                "Version '{}' not found for contract {}",
                version, contract_id
            ),
        )
    })?;

    let source_bytes = BASE64
        .decode(&req.source_base64)
        .map_err(|_| ApiError::bad_request("InvalidBase64", "source_base64 must be base64"))?;

    let source_format = match req.source_format.to_lowercase().as_str() {
        "rust" => shared::models::SourceFormat::Rust,
        "wasm" => shared::models::SourceFormat::Wasm,
        other => {
            return Err(ApiError::bad_request(
                "InvalidSourceFormat",
                format!(
                    "Unsupported source format '{}', expected 'rust' or 'wasm'",
                    other
                ),
            ))
        }
    };

    let (backend, storage_key, source_hash) = state
        .source_storage
        .store_source(&contract_id, &version, source_format.clone(), &source_bytes)
        .await
        .map_err(|e| ApiError::internal(format!("source storage error: {}", e)))?;

    let source_size = source_bytes.len() as i64;

    let source_row: ContractSource = sqlx::query_as(
        "INSERT INTO contract_sources (contract_version_id, source_format, storage_backend, storage_key, source_hash, source_size) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(version_row.id)
    .bind(source_format)
    .bind(&backend)
    .bind(&storage_key)
    .bind(&source_hash)
    .bind(source_size)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("insert contract source", err))?;

    sqlx::query(
        "INSERT INTO source_access_logs (contract_source_id, action, actor, request_ip, user_agent, details) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(source_row.id)
    .bind("upload")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<serde_json::Value>)
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("insert source access log", err))?;

    Ok(Json(ContractSourceResponse {
        id: source_row.id,
        contract_version_id: source_row.contract_version_id,
        source_format: source_row.source_format.to_string(),
        storage_backend: source_row.storage_backend,
        storage_key: source_row.storage_key,
        source_hash: source_row.source_hash,
        source_size: source_row.source_size,
        source_base64: Some(req.source_base64),
        created_at: source_row.created_at,
    }))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/versions/{version}/source",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        ("version" = String, Path, description = "Contract version")
    ),
    responses(
        (status = 200, description = "Retrieve source", body = ContractSourceResponse),
        (status = 404, description = "Source not found"),
        (status = 500, description = "Integrity verification failed")
    ),
    tag = "Source"
)]
pub async fn get_contract_source(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
    Query(query): Query<ContractSourceQuery>,
) -> ApiResult<Json<ContractSourceResponse>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;

    let version_row: ContractVersion =
        sqlx::query_as("SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2")
            .bind(contract_uuid)
            .bind(&version)
            .fetch_one(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch contract version", err))?;

    let format = query
        .source_format
        .as_deref()
        .unwrap_or("rust")
        .to_lowercase();

    let source_format = match format.as_str() {
        "rust" => shared::models::SourceFormat::Rust,
        "wasm" => shared::models::SourceFormat::Wasm,
        other => {
            return Err(ApiError::bad_request(
                "InvalidSourceFormat",
                format!(
                    "Unsupported source format '{}', expected 'rust' or 'wasm'",
                    other
                ),
            ))
        }
    };

    let source_row: ContractSource = sqlx::query_as(
        "SELECT * FROM contract_sources WHERE contract_version_id = $1 AND source_format = $2",
    )
    .bind(version_row.id)
    .bind(source_format)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("fetch contract source", err))?;

    let source_bytes = state
        .source_storage
        .retrieve_source(&source_row.storage_backend, &source_row.storage_key)
        .await
        .map_err(|e| ApiError::internal(format!("source storage error: {}", e)))?;

    let check_hash = shared::source_storage::compute_sha256(&source_bytes);
    if check_hash != source_row.source_hash {
        return Err(ApiError::internal("Contract source integrity check failed"));
    }

    sqlx::query(
        "INSERT INTO source_access_logs (contract_source_id, action, actor, request_ip, user_agent, details) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(source_row.id)
    .bind("retrieve")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(Some(json!({"ip": "unknown"})))
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("insert source access log", err))?;

    Ok(Json(ContractSourceResponse {
        id: source_row.id,
        contract_version_id: source_row.contract_version_id,
        source_format: source_row.source_format.to_string(),
        storage_backend: source_row.storage_backend,
        storage_key: source_row.storage_key,
        source_hash: source_row.source_hash,
        source_size: source_row.source_size,
        source_base64: Some(BASE64.encode(source_bytes)),
        created_at: source_row.created_at,
    }))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/versions/{version}/source/diff",
    params(
        ("id" = String, Path, description = "Contract UUID"),
        ("version" = String, Path, description = "Contract version"),
        ("compare_version" = String, Query, description = "Contract version to compare against")
    ),
    responses(
        (status = 200, description = "Source diff", body = ContractSourceDiffResponse),
        (status = 404, description = "Version/source not found"),
        (status = 400, description = "Invalid input")
    ),
    tag = "Source"
)]
pub async fn get_contract_source_diff(
    State(state): State<AppState>,
    Path((id, version)): Path<(String, String)>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<ContractSourceDiffResponse>> {
    let (contract_uuid, contract_id) = fetch_contract_identity(&state, &id).await?;

    let compare_version = params
        .get("compare_version")
        .ok_or_else(|| {
            ApiError::bad_request("MissingCompareVersion", "compare_version is required")
        })?
        .to_string();

    async fn load_source(
        state: &AppState,
        contract_uuid: Uuid,
        version: &str,
        source_format: &str,
    ) -> Result<(String, Uuid), ApiError> {
        let version_row: ContractVersion = sqlx::query_as(
            "SELECT * FROM contract_versions WHERE contract_id = $1 AND version = $2",
        )
        .bind(contract_uuid)
        .bind(version)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch contract version", err))?;

        let sf = match source_format {
            "rust" => shared::models::SourceFormat::Rust,
            "wasm" => shared::models::SourceFormat::Wasm,
            _ => {
                return Err(ApiError::bad_request(
                    "InvalidSourceFormat",
                    "source_format must be 'rust' or 'wasm'",
                ));
            }
        };

        let source_row: ContractSource = sqlx::query_as(
            "SELECT * FROM contract_sources WHERE contract_version_id = $1 AND source_format = $2",
        )
        .bind(version_row.id)
        .bind(sf)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("fetch contract source", err))?;

        let bytes = state
            .source_storage
            .retrieve_source(&source_row.storage_backend, &source_row.storage_key)
            .await
            .map_err(|e| ApiError::internal(format!("source storage error: {}", e)))?;

        let check_hash = shared::source_storage::compute_sha256(&bytes);
        if check_hash != source_row.source_hash {
            return Err(ApiError::internal("Contract source integrity check failed"));
        }

        let data_str = String::from_utf8(bytes)
            .map_err(|_| ApiError::internal("Source content is not UTF-8 serializable"))?;

        Ok((data_str, source_row.id))
    }

    let source_format = params
        .get("source_format")
        .map(|s| s.as_str())
        .unwrap_or("rust");

    let (base_source, base_source_id) =
        load_source(&state, contract_uuid, &version, source_format).await?;
    let (compare_source, compare_source_id) =
        load_source(&state, contract_uuid, &compare_version, source_format).await?;

    let diff = difference::Changeset::new(&compare_source, &base_source, "\n");
    let diff_text = diff
        .diffs
        .iter()
        .map(|chunk| match chunk {
            difference::Difference::Same(txt) => format!(" {}", txt),
            difference::Difference::Add(txt) => format!("+{}", txt),
            difference::Difference::Rem(txt) => format!("-{}", txt),
        })
        .collect::<Vec<_>>()
        .join("\n");

    sqlx::query(
        "INSERT INTO source_access_logs (contract_source_id, action, actor, request_ip, user_agent, details) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(base_source_id)
    .bind("diff")
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(None::<String>)
    .bind(Some(json!({"compare_version": compare_version, "target_version": version})))
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("insert source access log", err))?;

    Ok(Json(ContractSourceDiffResponse {
        base_version: compare_version,
        target_version: version,
        source_format: source_format.to_string(),
        diff: diff_text,
    }))
}

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/changelog",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "Contract changelog with breaking-change markers", body = ContractChangelogResponse),
        (status = 400, description = "Invalid contract ID format"),
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

            // Note: For internal resolution in handlers, we generally don't bypass unless requested.
            // But these specific calls are for diffing, so we use default false.
            let old_abi = resolve_abi(&state, &old_selector, false).await?;
            let new_abi = resolve_abi(&state, &new_selector, false).await?;

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

    // prev_snapshot is populated when there is a prior version; used for delta storage.
    let mut prev_snapshot: Option<crate::patch_handlers::VersionSnapshot> = None;

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
            let old_abi = resolve_abi(&state, &old_selector, false).await?;
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

            // Fetch the previous version row for delta computation.
            let old_ver_str = old_version.to_string();
            #[derive(sqlx::FromRow)]
            struct PrevRow {
                wasm_hash: String,
                source_url: Option<String>,
                commit_hash: Option<String>,
                release_notes: Option<String>,
                state_schema: Option<serde_json::Value>,
            }
            let prev_row: Option<PrevRow> = sqlx::query_as(
                "SELECT wasm_hash, source_url, commit_hash, release_notes, state_schema \
                 FROM contract_versions \
                 WHERE contract_id = $1 AND version = $2",
            )
            .bind(contract_uuid)
            .bind(&old_ver_str)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| db_internal_error("fetch previous version for patch", err))?;

            if let Some(pr) = prev_row {
                let old_abi_json: serde_json::Value =
                    serde_json::from_str(&old_abi).unwrap_or(serde_json::Value::Null);
                prev_snapshot = Some(crate::patch_handlers::VersionSnapshot {
                    version: old_ver_str,
                    wasm_hash: pr.wasm_hash,
                    source_url: pr.source_url,
                    commit_hash: pr.commit_hash,
                    release_notes: pr.release_notes,
                    state_schema: pr.state_schema,
                    abi: old_abi_json,
                });
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
            (contract_id, version, wasm_hash, source_url, commit_hash, release_notes, change_notes, signature, publisher_key, signature_algorithm) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         RETURNING *",
    )
    .bind(contract_uuid)
    .bind(&req.version)
    .bind(&req.wasm_hash)
    .bind(&req.source_url)
    .bind(&req.commit_hash)
    .bind(&req.release_notes)
    .bind(&req.change_notes)
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

    sqlx::query(
        "UPDATE contracts SET deployment_count = deployment_count + 1, current_version = $2 WHERE id = $1",
    )
    .bind(contract_uuid)
    .bind(&req.version)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("update current_version", err))?;

    tx.commit()
        .await
        .map_err(|err| db_internal_error("commit contract version", err))?;

    state.cache.invalidate_abi(&contract_id).await;
    state.cache.invalidate_abi(&contract_uuid.to_string()).await;
    state
        .cache
        .invalidate_abi(&format!("{}@{}", contract_id, req.version))
        .await;

    // Store differential patch for the new version (Issue #501).
    let new_snapshot = crate::patch_handlers::VersionSnapshot {
        version: req.version.clone(),
        wasm_hash: req.wasm_hash.clone(),
        source_url: req.source_url.clone(),
        commit_hash: req.commit_hash.clone(),
        release_notes: req.release_notes.clone(),
        state_schema: None,
        abi: req.abi.clone(),
    };
    if let Err(e) = crate::patch_handlers::store_patch(
        &state.db,
        contract_uuid,
        prev_snapshot.as_ref(),
        &new_snapshot,
    )
    .await
    {
        tracing::error!("Failed to store differential patch for version {}: {}", req.version, e);
    }

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
            .publish(ContractEventEnvelope::version_created(
                &contract,
                &version_row,
            ));
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

    state
        .contract_events
        .publish(ContractEventEnvelope::deployed(
            &contract,
            Some(publisher.stellar_address.clone()),
        ));

    if req.is_cicd {
        crate::events::emit_cicd_pipeline(
            &state,
            contract.contract_id.clone(),
            "published".to_string(),
            4, // Step 4 of 5
        );
    }

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
    pub bypass_cache: Option<bool>,
}

/// Fetch ABI JSON string for contract (by id or id@version)
async fn resolve_contract_abi(
    state: &AppState,
    id: &str,
    version: Option<&str>,
    bypass_cache: bool,
) -> ApiResult<String> {
    let selector = if let Some(v) = version {
        format!("{}@{}", id, v)
    } else {
        id.to_string()
    };
    resolve_abi(state, &selector, bypass_cache).await
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
    Path(id): Path<String>,
    Query(query): Query<ContractAbiQuery>,
    State(state): State<AppState>,
) -> ApiResult<Json<Value>> {
    let bypass = query.bypass_cache.unwrap_or(false);
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref(), bypass).await?;
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
    let bypass = query.bypass_cache.unwrap_or(false);
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref(), bypass).await?;
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
    let bypass = query.bypass_cache.unwrap_or(false);
    let abi_json = resolve_contract_abi(&state, &id, query.version.as_deref(), bypass).await?;
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

#[utoipa::path(
    get,
    path = "/api/contracts/{id}/trust-score",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 501, description = "Not yet implemented – this endpoint is planned")
    ),
    tag = "Security"
)]
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
    let onchain_verifier = OnChainVerifier::new();
    let abi_json = resolve_abi(&state, &contract.contract_id).await.ok();
    let onchain_result = onchain_verifier
        .verify_contract(&state.cache, &contract, abi_json.as_deref())
        .await;

    let ip_address = extract_ip_address(&headers);
    let before_status = previous_status.unwrap_or_else(|| "pending".to_string());

    match (verification_result, onchain_result) {
        (Ok(result), Ok(onchain))
            if result.verified
                && onchain.contract_exists_on_chain
                && onchain.wasm_hash_matches
                && onchain.abi_valid =>
        {
            sqlx::query(
                "UPDATE verifications
                 SET status = 'verified', verified_at = NOW(), error_message = NULL
                 WHERE id = $1",
            )
            .bind(verification_id)
            .execute(&state.db)
            .await
            .map_err(|err| db_internal_error("mark verification as verified", err))?;

            // Update contract metadata (Issue #401)
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
                "deployed_wasm_hash": result.deployed_wasm_hash,
                "on_chain": onchain
            })))
        }
        (Ok(result), Ok(onchain)) => {
            let mut reasons = Vec::new();
            if !result.verified {
                reasons.push(
                    result.message.unwrap_or_else(|| {
                        "Verification failed due to bytecode mismatch".to_string()
                    }),
                );
            }
            if !onchain.contract_exists_on_chain {
                reasons.push("Contract does not exist on-chain".to_string());
            }
            if onchain.contract_exists_on_chain && !onchain.wasm_hash_matches {
                reasons.push("On-chain deployment does not match the stored WASM hash".to_string());
            }
            if onchain.contract_exists_on_chain && !onchain.abi_valid {
                reasons
                    .push("Stored ABI does not validate against the deployed contract".to_string());
            }
            let failure_message = reasons.join("; ");

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
        (Err(err), _) | (_, Err(err)) => {
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

        state
            .contract_events
            .publish(ContractEventEnvelope::metadata_updated(
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

    state
        .contract_events
        .publish(ContractEventEnvelope::status_updated(
            &contract_after,
            normalized_status.clone(),
            is_verified_after,
            None,
            ContractEventVisibility::Public,
        ));

    if req.error_message.is_some() {
        state
            .contract_events
            .publish(ContractEventEnvelope::status_updated(
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
    path = "/api/contracts/{id}/deployments",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "List of contract deployments", body = [ContractDeployment]),
        (status = 404, description = "Contract not found")
    ),
    tag = "Deployments"
)]
pub async fn get_contract_deployments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ContractDeployment>>> {
    let contract_uuid = Uuid::parse_str(&id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })?;

    let deployments: Vec<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments WHERE contract_id = $1 ORDER BY deployed_at DESC",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get contract deployments", err))?;

    Ok(Json(deployments))
}

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

        assert_eq!(
            sort_timestamp_column(&shared::SortBy::CreatedAt),
            Some("c.created_at")
        );
        assert_eq!(
            sort_timestamp_column(&shared::SortBy::UpdatedAt),
            Some("c.updated_at")
        );
        assert_eq!(
            sort_timestamp_column(&shared::SortBy::VerifiedAt),
            Some("c.verified_at")
        );
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

// ────────────────────────────────────────────────────────────────────────────
// ADVANCED SEARCH (Issue #51)
// ────────────────────────────────────────────────────────────────────────────

/// Advanced contract search using a recursive Query DSL
#[utoipa::path(
    post,
    path = "/api/contracts/search",
    request_body = AdvancedSearchRequest,
    responses(
        (status = 200, description = "Search results", body = PaginatedResponse<Contract>),
        (status = 400, description = "Invalid query DSL")
    ),
    tag = "Contracts"
)]
pub async fn advanced_search_contracts(
    State(state): State<AppState>,
    ValidatedJson(req): ValidatedJson<AdvancedSearchRequest>,
) -> ApiResult<Json<PaginatedResponse<Contract>>> {
    let limit = req.limit.unwrap_or(20).clamp(1, 100);
    let offset = req.offset.unwrap_or(0).max(0);
    let page = (offset / limit) + 1;

    let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
        sqlx::QueryBuilder::new("SELECT c.* FROM contracts c ");

    // Add joins for sorting/filtering if needed
    query_builder.push("LEFT JOIN contract_interactions ci ON c.id = ci.contract_id ");
    query_builder.push("LEFT JOIN contract_versions cv ON c.id = cv.contract_id ");
    query_builder.push("WHERE 1=1 ");

    // Recursively build the WHERE clause
    build_where_clause(&mut query_builder, &req.query)?;

    query_builder.push(" GROUP BY c.id ");

    // Sorting
    let sort_by = req.sort_by.unwrap_or(shared::SortBy::CreatedAt);
    let sort_order = req.sort_order.unwrap_or(shared::SortOrder::Desc);
    let direction = if sort_order == shared::SortOrder::Asc {
        "ASC"
    } else {
        "DESC"
    };

    query_builder.push(" ORDER BY ");
    match sort_by {
        shared::SortBy::CreatedAt => {
            query_builder.push("c.created_at ");
        }
        shared::SortBy::UpdatedAt => {
            query_builder.push("c.updated_at ");
        }
        shared::SortBy::Popularity | shared::SortBy::Interactions => {
            query_builder.push("COUNT(DISTINCT ci.id) ");
        }
        shared::SortBy::Deployments => {
            query_builder.push("COUNT(DISTINCT cv.id) ");
        }
        shared::SortBy::Relevance => {
            query_builder.push("c.created_at "); // Default relevance if no query term
        }
        _ => {
            query_builder.push("c.created_at ");
        }
    }
    query_builder.push(direction);
    query_builder.push(", c.id DESC ");

    // Pagination
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
    query_builder.push(" OFFSET ");
    query_builder.push_bind(offset);

    let query = query_builder.build_query_as::<Contract>();
    let contracts = query
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_internal_error("advanced search contracts", err))?;

    // Count total matches (naively for now, same filters)
    let mut count_builder: sqlx::QueryBuilder<'_, sqlx::Postgres> =
        sqlx::QueryBuilder::new("SELECT COUNT(DISTINCT c.id) FROM contracts c ");
    count_builder.push("WHERE 1=1 ");
    build_where_clause(&mut count_builder, &req.query)?;

    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_internal_error("count advanced search", err))?;

    Ok(Json(PaginatedResponse::new(contracts, total, page, limit)))
}

fn build_where_clause<'a>(
    builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    node: &'a QueryNode,
) -> ApiResult<()> {
    match node {
        QueryNode::Condition(cond) => {
            builder.push(" AND ");
            apply_condition(builder, cond)?;
        }
        QueryNode::Group {
            operator,
            conditions,
        } => {
            if conditions.is_empty() {
                return Ok(());
            }
            builder.push(" AND (");
            for (i, child) in conditions.iter().enumerate() {
                if i > 0 {
                    match operator {
                        QueryOperator::And => builder.push(" AND "),
                        QueryOperator::Or => builder.push(" OR "),
                    };
                }

                // For groups we need to wrap children
                match child {
                    QueryNode::Condition(c) => apply_condition(builder, c)?,
                    QueryNode::Group { .. } => {
                        builder.push(" (1=1 ");
                        build_where_clause(builder, child)?;
                        builder.push(") ");
                    }
                }
            }
            builder.push(") ");
        }
    }
    Ok(())
}

fn apply_condition<'a>(
    builder: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    cond: &'a QueryCondition,
) -> ApiResult<()> {
    let field = match cond.field.as_str() {
        "name" => "c.name",
        "description" => "c.description",
        "category" => "c.category",
        "network" => "c.network",
        "verified" => "c.is_verified",
        "publisher" => "c.publisher_id",
        _ => {
            return Err(ApiError::bad_request(
                "InvalidField",
                format!("Field '{}' is not searchable", cond.field),
            ))
        }
    };

    builder.push(field);
    match cond.operator {
        FieldOperator::Eq => {
            builder.push(" = ");
            builder.push_bind(cond.value.as_str().unwrap_or_default());
        }
        FieldOperator::Ne => {
            builder.push(" != ");
            builder.push_bind(cond.value.as_str().unwrap_or_default());
        }
        FieldOperator::Gt => {
            builder.push(" > ");
            builder.push_bind(cond.value.as_str().unwrap_or_default());
        }
        FieldOperator::Lt => {
            builder.push(" < ");
            builder.push_bind(cond.value.as_str().unwrap_or_default());
        }
        FieldOperator::In => {
            builder.push(" IN (");
            if let Some(arr) = cond.value.as_array() {
                let mut separated = builder.separated(", ");
                for val in arr {
                    separated.push_bind(val.as_str().unwrap_or_default());
                }
            }
            builder.push(")");
        }
        FieldOperator::Contains => {
            builder.push(" ILIKE ");
            let val = format!("%{}%", cond.value.as_str().unwrap_or_default());
            builder.push_bind(val);
        }
        FieldOperator::StartsWith => {
            builder.push(" ILIKE ");
            let val = format!("{}%", cond.value.as_str().unwrap_or_default());
            builder.push_bind(val);
        }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// FAVORITE SEARCHES (Issue #51)
// ────────────────────────────────────────────────────────────────────────────

/// List favorite searches for the current user
#[utoipa::path(
    get,
    path = "/api/favorites/search",
    responses(
        (status = 200, description = "List of favorite searches", body = [FavoriteSearch])
    ),
    tag = "Favorites"
)]
pub async fn list_favorite_searches(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<FavoriteSearch>>> {
    // For now, return all since we don't have a strict user_id auth yet
    let favorites: Vec<FavoriteSearch> =
        sqlx::query_as("SELECT * FROM favorite_searches ORDER BY created_at DESC")
            .fetch_all(&state.db)
            .await
            .map_err(|err| db_internal_error("list favorite searches", err))?;

    Ok(Json(favorites))
}

/// Save a new favorite search
#[utoipa::path(
    post,
    path = "/api/favorites/search",
    request_body = SaveFavoriteSearchRequest,
    responses(
        (status = 201, description = "Favorite search saved", body = FavoriteSearch)
    ),
    tag = "Favorites"
)]
pub async fn save_favorite_search(
    State(state): State<AppState>,
    ValidatedJson(req): ValidatedJson<SaveFavoriteSearchRequest>,
) -> ApiResult<Json<FavoriteSearch>> {
    let query_json = serde_json::to_value(&req.query).map_err(|e| {
        ApiError::bad_request("InvalidQuery", format!("Failed to serialize query: {}", e))
    })?;

    let favorite: FavoriteSearch = sqlx::query_as(
        "INSERT INTO favorite_searches (name, query_json) VALUES ($1, $2) RETURNING *",
    )
    .bind(&req.name)
    .bind(query_json)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("save favorite search", err))?;

    Ok(Json(favorite))
}

/// Delete a favorite search
#[utoipa::path(
    delete,
    path = "/api/favorites/search/{id}",
    params(
        ("id" = String, Path, description = "Favorite search ID")
    ),
    responses(
        (status = 204, description = "Favorite search deleted"),
        (status = 404, description = "Favorite search not found")
    ),
    tag = "Favorites"
)]
pub async fn delete_favorite_search(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| ApiError::bad_request("InvalidId", "Invalid favorite search ID format"))?;

    let result = sqlx::query("DELETE FROM favorite_searches WHERE id = $1")
        .bind(uuid)
        .execute(&state.db)
        .await
        .map_err(|err| db_internal_error("delete favorite search", err))?;

    if result.rows_affected() == 0 {
        return Err(ApiError::not_found(
            "FavoriteNotFound",
            "Favorite search not found",
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}
