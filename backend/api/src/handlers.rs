pub mod migrations;

use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use shared::{
    AbTest, AbTestAssignment, AbTestMetric, AbTestResult, AbTestStatus, AbTestVariant,
    AdvanceCanaryRequest, AlertSeverity, CanaryMetric, CanaryRelease, CanaryStatus,
    CanaryUserAssignment, Contract, ContractDeployment, ContractSearchParams, ContractVersion,
    CreateAbTestRequest, CreateAlertConfigRequest, CreateCanaryRequest, DeployGreenRequest,
    DeploymentEnvironment, DeploymentStatus, DeploymentSwitch, GetVariantRequest,
    HealthCheckRequest, MetricType, PaginatedResponse, PerformanceAlert, PerformanceAlertConfig,
    PerformanceAnomaly, PerformanceMetric, PerformanceTrend, PublishRequest, Publisher,
    RecordAbTestMetricRequest, RecordCanaryMetricRequest, RecordPerformanceMetricRequest,
    RolloutStage, SwitchDeploymentRequest, VariantType, VerifyRequest,
};
use rust_decimal::Decimal;
use std::str::FromStr;
use uuid::Uuid;

use crate::{
    analytics,
    error::{ApiError, ApiResult},
    state::AppState,
};

pub fn db_internal_error(operation: &str, err: sqlx::Error) -> ApiError {
    tracing::error!(operation = operation, error = ?err, "database operation failed");
    ApiError::internal("An unexpected database error occurred")
}

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

/// Health check ΓÇö probes DB connectivity and reports uptime.
/// Returns 200 when everything is reachable, 503 when the database
/// connection pool cannot satisfy a trivial query.
pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<serde_json::Value>) {
    let uptime = state.started_at.elapsed().as_secs();
    let now = chrono::Utc::now().to_rfc3339();

    // Quick connectivity probe ΓÇö keeps the query as cheap as possible
    // so that frequent polling from orchestrators doesn't add load.
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();

    if db_ok {
        tracing::info!(uptime_secs = uptime, "health check passed");

        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "ok",
                "version": "0.1.0",
                "timestamp": now,
                "uptime_secs": uptime
            })),
        )
    } else {
        tracing::warn!(
            uptime_secs = uptime,
            "health check degraded ΓÇö db unreachable"
        );

        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "status": "degraded",
                "version": "0.1.0",
                "timestamp": now,
                "uptime_secs": uptime
            })),
        )
    }
}

/// Get registry statistics
pub async fn get_stats(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
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

    Ok(Json(serde_json::json!({
        "total_contracts": total_contracts,
        "verified_contracts": verified_contracts,
        "total_publishers": total_publishers,
    })))
}

/// List and search contracts
pub async fn list_contracts(
    State(state): State<AppState>,
    params: Result<Query<ContractSearchParams>, QueryRejection>,
) -> axum::response::Response {
    let Query(params) = match params {
        Ok(q) => q,
        Err(err) => return map_query_rejection(err).into_response(),
    };

    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20);

    // bad input, bail early
    if page < 1 || limit < 1 || limit > 100 {
        return ApiError::bad_request(
            "InvalidPagination",
            "page must be >= 1 and limit must be between 1 and 100",
        )
        .into_response();
    }

    let offset = (page - 1) * limit;

    // Build dynamic query based on filters
    let mut query = String::from("SELECT * FROM contracts WHERE 1=1");
    let mut count_query = String::from("SELECT COUNT(*) FROM contracts WHERE 1=1");

    if let Some(ref q) = params.query {
        let search_clause = format!(" AND (name ILIKE '%{}%' OR description ILIKE '%{}%')", q, q);
        query.push_str(&search_clause);
        count_query.push_str(&search_clause);
    }

    if let Some(verified) = params.verified_only {
        if verified {
            query.push_str(" AND is_verified = true");
            count_query.push_str(" AND is_verified = true");
        }
    }

    if let Some(ref category) = params.category {
        let category_clause = format!(" AND category = '{}'", category);
        query.push_str(&category_clause);
        count_query.push_str(&category_clause);
    }

    query.push_str(&format!(
        " ORDER BY created_at DESC LIMIT {} OFFSET {}",
        limit, offset
    ));

    let contracts: Vec<Contract> = match sqlx::query_as(&query).fetch_all(&state.db).await {
        Ok(rows) => rows,
        Err(err) => return db_internal_error("list contracts", err).into_response(),
    };

    let total: i64 = match sqlx::query_scalar(&count_query).fetch_one(&state.db).await {
        Ok(n) => n,
        Err(err) => return db_internal_error("count filtered contracts", err).into_response(),
    };

    let paginated = PaginatedResponse::new(contracts, total, page, limit);

    // link headers for pagination
    let total_pages = paginated.total_pages;
    let mut links: Vec<String> = Vec::new();

    if page > 1 {
        links.push(format!(
            "</api/contracts?page={}&limit={}>; rel=\"prev\"",
            page - 1,
            limit
        ));
    }
    if page < total_pages {
        links.push(format!(
            "</api/contracts?page={}&limit={}>; rel=\"next\"",
            page + 1,
            limit
        ));
    }

    let mut response = (StatusCode::OK, Json(paginated)).into_response();

    if !links.is_empty() {
        if let Ok(value) = axum::http::HeaderValue::from_str(&links.join(", ")) {
            response.headers_mut().insert("link", value);
        }
    }

    response
}

pub async fn get_contract(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Contract>> {
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("get contract by id", err),
        })?;

    let active_deployment: Option<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 AND status = 'active'",
    )
    .bind(contract.id)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get active deployment", err))?;

    if let Some(deployment) = active_deployment {
        let mut contract_with_deployment = contract.clone();
        contract_with_deployment.wasm_hash = deployment.wasm_hash;
        Ok(Json(contract_with_deployment))
    } else {
    Ok(Json(contract))
    }
}

/// Get contract ABI
pub async fn get_contract_abi(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let abi: Option<serde_json::Value> = sqlx::query_scalar("SELECT abi FROM contracts WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    abi.map(Json).ok_or(StatusCode::NOT_FOUND)
}

/// Get contract version history
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
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("list versions", err))?;

    Ok(Json(versions))
}

/// Publish a new contract
pub async fn publish_contract(
    State(state): State<AppState>,
    payload: Result<Json<PublishRequest>, JsonRejection>,
) -> ApiResult<Json<Contract>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    // First, ensure publisher exists or create one
    let publisher: Publisher = sqlx::query_as(
        "INSERT INTO publishers (stellar_address) VALUES ($1)
         ON CONFLICT (stellar_address) DO UPDATE SET stellar_address = EXCLUDED.stellar_address
         RETURNING *",
    )
    .bind(&req.publisher_address)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("upsert publisher", err))?;

    // TODO: Fetch WASM hash from Stellar network
    let wasm_hash = "placeholder_hash".to_string();

    let contract: Contract = sqlx::query_as(
        "INSERT INTO contracts (contract_id, wasm_hash, name, description, publisher_id, network, category, tags)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&req.contract_id)
    .bind(&wasm_hash)
    .bind(&req.name)
    .bind(&req.description)
    .bind(publisher.id)
    .bind(&req.network)
    .bind(&req.category)
    .bind(&req.tags)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("create contract", err))?;

    // Fire-and-forget analytics event
    let pool = state.db.clone();
    let cid = contract.id;
    let addr = req.publisher_address.clone();
    let net = contract.network.clone();
    tokio::spawn(async move {
        if let Err(err) = analytics::record_event(
            &pool,
            AnalyticsEventType::ContractPublished,
            cid,
            Some(&addr),
            Some(&net),
            None,
        )
        .await
        {
            tracing::warn!(error = ?err, "failed to record contract_published event");
        }
    });
    sqlx::query(
        "INSERT INTO contract_deployments (contract_id, environment, status, wasm_hash, activated_at)
         VALUES ($1, 'blue', 'active', $2, NOW())
         ON CONFLICT (contract_id, environment) DO NOTHING",
    )
    .bind(contract.id)
    .bind(&wasm_hash)
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("create initial blue deployment", err))?;

    Ok(Json(contract))
}

/// Verify a contract
pub async fn verify_contract(
    State(state): State<AppState>,
    payload: Result<Json<VerifyRequest>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract_id = Uuid::parse_str(&req.contract_id).map_err(|_| {
        ApiError::bad_request("InvalidContractId", "Invalid contract ID format".to_string())
    })?;

    // Check if contract exists and get deployed wasm hash
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(contract_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|err| db_internal_error("get contract for verification", err))?
        .ok_or_else(|| ApiError::not_found("ContractNotFound", "Contract not found"))?;

    let pool = state.db.clone();
    let req_clone = req.clone();

    // Verify in background
    tokio::spawn(async move {
        // Create pending verification record
        let verification_id: Uuid = match sqlx::query_scalar(
            "INSERT INTO verifications (contract_id, status, source_code, build_params, compiler_version) 
             VALUES ($1, 'pending', $2, $3, $4) RETURNING id"
        )
        .bind(contract_id)
        .bind(&req_clone.source_code)
        .bind(&req_clone.build_params)
        .bind(&req_clone.compiler_version)
        .fetch_one(&pool)
        .await {
            Ok(id) => id,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to create pending verification record");
                return;
            }
        };

        match verifier::verify_contract(
            &req_clone.source_code,
            &req_clone.compiler_version,
            &req_clone.build_params,
            &contract.wasm_hash,
        ).await {
            Ok(output) => {
                let status = if output.is_verified { "verified" } else { "failed" };
                let verified_at = if output.is_verified { Some(chrono::Utc::now()) } else { None };

                if let Err(e) = sqlx::query(
                    "UPDATE verifications SET status = $1, error_message = $2, verified_at = $3 WHERE id = $4"
                )
                .bind(status)
                .bind(output.compiler_output)
                .bind(verified_at)
                .bind(verification_id)
                .execute(&pool)
                .await {
                    tracing::error!(error = ?e, "Failed to update verification record");
                }

                if output.is_verified {
                    if let Err(e) = sqlx::query("UPDATE contracts SET is_verified = true WHERE id = $1")
                        .bind(contract_id)
                        .execute(&pool)
                        .await {
                        tracing::error!(error = ?e, "Failed to mark contract as verified");
                    }
                }
            }
            Err(e) => {
                if let Err(db_e) = sqlx::query(
                    "UPDATE verifications SET status = 'failed', error_message = $1 WHERE id = $2"
                )
                .bind(e.to_string())
                .bind(verification_id)
                .execute(&pool)
                .await {
                    tracing::error!(error = ?db_e, "Failed to update verification record with error");
                }
            }
        }

        // Fire-and-forget analytics event
        if let Err(err) = analytics::record_event(
            &pool,
            AnalyticsEventType::ContractVerified,
            contract_id,
            None,
            None,
            Some(serde_json::json!({ "compiler_version": req_clone.compiler_version })),
        ).await {
            tracing::warn!(error = ?err, "failed to record contract_verified event");
        }
    });

    Ok(Json(serde_json::json!({
        "status": "pending",
        "message": "Verification started processing in background"
    })))
}

/// Create a publisher
pub async fn create_publisher(
    State(state): State<AppState>,
    payload: Result<Json<Publisher>, JsonRejection>,
) -> ApiResult<Json<Publisher>> {
    let Json(publisher) = payload.map_err(map_json_rejection)?;

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

    Ok(Json(created))
}

/// Get publisher by ID
pub async fn get_publisher(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Publisher>> {
    let publisher: Publisher = sqlx::query_as("SELECT * FROM publishers WHERE id = $1")
        .bind(id)
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

/// Get all contracts by a publisher
pub async fn get_publisher_contracts(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Contract>>> {
    let contracts: Vec<Contract> =
        sqlx::query_as("SELECT * FROM contracts WHERE publisher_id = $1 ORDER BY created_at DESC")
            .bind(id)
    .fetch_all(&state.db)
    .await
            .map_err(|err| db_internal_error("list publisher contracts", err))?;

    Ok(Json(contracts))
}

/// Get analytics for a specific contract
pub async fn get_contract_analytics(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ContractAnalyticsResponse>> {
    // Verify the contract exists
    let _contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("get contract for analytics", err),
        })?;

    let thirty_days_ago = chrono::Utc::now() - chrono::Duration::days(30);

    // Deployment stats
    let deploy_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM analytics_events WHERE contract_id = $1 AND event_type = 'contract_deployed'",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("deployment count", e))?;

    let unique_deployers: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address) FROM analytics_events WHERE contract_id = $1 AND event_type = 'contract_deployed' AND user_address IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("unique deployers", e))?;

    let by_network: serde_json::Value = sqlx::query_scalar(
        r#"SELECT COALESCE(jsonb_object_agg(COALESCE(network::text, 'unknown'), cnt), '{}'::jsonb)
        FROM (SELECT network, COUNT(*) AS cnt FROM analytics_events WHERE contract_id = $1 AND event_type = 'contract_deployed' GROUP BY network) sub"#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("network breakdown", e))?;

    let unique_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address) FROM analytics_events WHERE contract_id = $1 AND user_address IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("unique interactors", e))?;

    let top_user_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT user_address, COUNT(*) AS cnt FROM analytics_events WHERE contract_id = $1 AND user_address IS NOT NULL GROUP BY user_address ORDER BY cnt DESC LIMIT 10",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("top users", e))?;

    let top_users: Vec<TopUser> = top_user_rows
        .into_iter()
        .map(|(address, count)| TopUser { address, count })
        .collect();

    let timeline_rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        r#"SELECT d::date AS date, COALESCE(e.cnt, 0) AS count
        FROM generate_series(($1::timestamptz)::date, CURRENT_DATE, '1 day'::interval) d
        LEFT JOIN (SELECT DATE(created_at) AS event_date, COUNT(*) AS cnt FROM analytics_events WHERE contract_id = $2 AND created_at >= $1 GROUP BY DATE(created_at)) e ON d::date = e.event_date
        ORDER BY d::date"#,
    )
    .bind(thirty_days_ago)
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("timeline", e))?;

    let timeline: Vec<TimelineEntry> = timeline_rows
        .into_iter()
        .map(|(date, count)| TimelineEntry { date, count })
        .collect();

    Ok(Json(ContractAnalyticsResponse {
        contract_id: id,
        deployments: DeploymentStats { count: deploy_count, unique_users: unique_deployers, by_network },
        interactors: InteractorStats { unique_count, top_users },
        timeline,
    }))
}
pub async fn deploy_green(
    State(state): State<AppState>,
    payload: Result<Json<DeployGreenRequest>, JsonRejection>,
) -> ApiResult<Json<ContractDeployment>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("No contract found with ID: {}", id),
            ),
            _ => db_internal_error("get contract for analytics", err),
        })?;

    let thirty_days_ago = chrono::Utc::now() - chrono::Duration::days(30);

    // ΓöÇΓöÇ Deployment stats ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
    let deploy_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM analytics_events \
         WHERE contract_id = $1 AND event_type = 'contract_deployed'",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("deployment count", e))?;

    let unique_deployers: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address) FROM analytics_events \
         WHERE contract_id = $1 AND event_type = 'contract_deployed' AND user_address IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("unique deployers", e))?;

    let by_network: serde_json::Value = sqlx::query_scalar(
        r#"
        SELECT COALESCE(
            jsonb_object_agg(COALESCE(network::text, 'unknown'), cnt),
            '{}'::jsonb
        )
        FROM (
            SELECT network, COUNT(*) AS cnt
            FROM analytics_events
            WHERE contract_id = $1 AND event_type = 'contract_deployed'
            GROUP BY network
        ) sub
        "#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("network breakdown", e))?;

    // ΓöÇΓöÇ Interactor stats ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
    let unique_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address) FROM analytics_events \
         WHERE contract_id = $1 AND user_address IS NOT NULL",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| db_internal_error("unique interactors", e))?;

    let top_user_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT user_address, COUNT(*) AS cnt FROM analytics_events \
         WHERE contract_id = $1 AND user_address IS NOT NULL \
         GROUP BY user_address ORDER BY cnt DESC LIMIT 10",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("top users", e))?;

    let top_users: Vec<TopUser> = top_user_rows
        .into_iter()
        .map(|(address, count)| TopUser { address, count })
        .collect();

    // ΓöÇΓöÇ Timeline (last 30 days) ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
    let timeline_rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        r#"
        SELECT d::date AS date, COALESCE(e.cnt, 0) AS count
        FROM generate_series(
            ($1::timestamptz)::date,
            CURRENT_DATE,
            '1 day'::interval
        ) d
        LEFT JOIN (
            SELECT DATE(created_at) AS event_date, COUNT(*) AS cnt
            FROM analytics_events
            WHERE contract_id = $2
              AND created_at >= $1
            GROUP BY DATE(created_at)
        ) e ON d::date = e.event_date
        ORDER BY d::date
        "#,
    )
    .bind(thirty_days_ago)
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| db_internal_error("timeline", e))?;

    let timeline: Vec<TimelineEntry> = timeline_rows
        .into_iter()
        .map(|(date, count)| TimelineEntry { date, count })
        .collect();

    // ΓöÇΓöÇ Build response ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ
    Ok(Json(ContractAnalyticsResponse {
        contract_id: id,
        deployments: DeploymentStats {
            count: deploy_count,
            unique_users: unique_deployers,
            by_network,
        },
        interactors: InteractorStats {
            unique_count,
            top_users,
        },
        timeline,
    }))
}


pub async fn switch_deployment(
    State(state): State<AppState>,
    payload: Result<Json<SwitchDeploymentRequest>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(req) = payload.map_err(map_json_rejection)?;
    let force = req.force.unwrap_or(false);

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", req.contract_id),
            ),
            _ => db_internal_error("get contract for switch", err),
        })?;

    let mut tx = state.db.begin().await.map_err(|err| {
        db_internal_error("begin transaction for switch", err)
    })?;

    let active_deployment: Option<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 AND status = 'active'",
    )
    .bind(contract.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| db_internal_error("get active deployment", err))?;

    let from_env = active_deployment
        .as_ref()
        .map(|d| d.environment.clone())
        .unwrap_or(DeploymentEnvironment::Blue);

    let to_env = match from_env {
        DeploymentEnvironment::Blue => DeploymentEnvironment::Green,
        DeploymentEnvironment::Green => DeploymentEnvironment::Blue,
    };

    let green_deployment: Option<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 AND environment = 'green'",
    )
    .bind(contract.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| db_internal_error("get green deployment", err))?;

    if let Some(ref green) = green_deployment {
        if !force && green.status != DeploymentStatus::Testing {
            return Err(ApiError::bad_request(
                "InvalidDeploymentStatus",
                "Green deployment must be in testing status before switch",
            ));
        }
        if !force && green.health_checks_passed < 3 {
            return Err(ApiError::bad_request(
                "InsufficientHealthChecks",
                "Green deployment must pass at least 3 health checks before switch",
            ));
        }
    } else {
        return Err(ApiError::bad_request(
            "NoGreenDeployment",
            "No green deployment found",
        ));
    }

    if let Some(ref active) = active_deployment {
        sqlx::query("UPDATE contract_deployments SET status = 'inactive' WHERE id = $1")
            .bind(active.id)
            .execute(&mut *tx)
            .await
            .map_err(|err| db_internal_error("deactivate current deployment", err))?;
    }

    sqlx::query(
        "UPDATE contract_deployments 
         SET status = 'active', activated_at = NOW() 
         WHERE contract_id = $1 AND environment = $2",
    )
    .bind(contract.id)
    .bind(&to_env)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("activate new deployment", err))?;

    sqlx::query_as::<_, DeploymentSwitch>(
        "INSERT INTO deployment_switches (contract_id, from_environment, to_environment)
         VALUES ($1, $2, $3)
         RETURNING *",
    )
    .bind(contract.id)
    .bind(&from_env)
    .bind(&to_env)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| db_internal_error("record deployment switch", err))?;

    tx.commit().await.map_err(|err| {
        db_internal_error("commit deployment switch", err)
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "switched_from": from_env,
        "switched_to": to_env,
        "contract_id": req.contract_id
    })))
}

pub async fn rollback_deployment(
    State(state): State<AppState>,
    Path(contract_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", contract_id),
            ),
            _ => db_internal_error("get contract for rollback", err),
        })?;

    let mut tx = state.db.begin().await.map_err(|err| {
        db_internal_error("begin transaction for rollback", err)
    })?;

    let active_deployment: Option<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 AND status = 'active'",
    )
    .bind(contract.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| db_internal_error("get active deployment", err))?;

    let from_env = active_deployment
        .as_ref()
        .map(|d| d.environment.clone())
        .unwrap_or(DeploymentEnvironment::Green);

    let to_env = match from_env {
        DeploymentEnvironment::Blue => DeploymentEnvironment::Green,
        DeploymentEnvironment::Green => DeploymentEnvironment::Blue,
    };

    let target_deployment: Option<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 AND environment = $2",
    )
    .bind(contract.id)
    .bind(&to_env)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|err| db_internal_error("get target deployment", err))?;

    if target_deployment.is_none() {
        return Err(ApiError::bad_request(
            "NoDeploymentToRollback",
            format!("No {} deployment found to rollback to", to_env),
        ));
    }

    if let Some(ref active) = active_deployment {
        sqlx::query("UPDATE contract_deployments SET status = 'inactive' WHERE id = $1")
            .bind(active.id)
            .execute(&mut *tx)
            .await
            .map_err(|err| db_internal_error("deactivate current deployment", err))?;
    }

    sqlx::query(
        "UPDATE contract_deployments 
         SET status = 'active', activated_at = NOW() 
         WHERE contract_id = $1 AND environment = $2",
    )
    .bind(contract.id)
    .bind(&to_env)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("activate rollback deployment", err))?;

    sqlx::query_as::<_, DeploymentSwitch>(
        "INSERT INTO deployment_switches (contract_id, from_environment, to_environment, rollback)
         VALUES ($1, $2, $3, true)
         RETURNING *",
    )
    .bind(contract.id)
    .bind(&from_env)
    .bind(&to_env)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| db_internal_error("record rollback switch", err))?;

    tx.commit().await.map_err(|err| {
        db_internal_error("commit rollback", err)
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "rolled_back_from": from_env,
        "rolled_back_to": to_env,
        "contract_id": contract_id
    })))
}

pub async fn report_health_check(
    State(state): State<AppState>,
    payload: Result<Json<HealthCheckRequest>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", req.contract_id),
            ),
            _ => db_internal_error("get contract for health check", err),
        })?;

    let env_str = match req.environment {
        DeploymentEnvironment::Blue => "blue",
        DeploymentEnvironment::Green => "green",
    };

    if req.passed {
        sqlx::query(
            "UPDATE contract_deployments 
             SET health_checks_passed = health_checks_passed + 1, 
                 last_health_check_at = NOW()
             WHERE contract_id = $1 AND environment = $2",
        )
        .bind(contract.id)
        .bind(&req.environment)
        .execute(&state.db)
        .await
        .map_err(|err| db_internal_error("update health check passed", err))?;
    } else {
        sqlx::query(
            "UPDATE contract_deployments 
             SET health_checks_failed = health_checks_failed + 1, 
                 status = CASE WHEN health_checks_failed + 1 >= 3 THEN 'failed' ELSE status END,
                 last_health_check_at = NOW()
             WHERE contract_id = $1 AND environment = $2",
        )
        .bind(contract.id)
        .bind(&req.environment)
        .execute(&state.db)
        .await
        .map_err(|err| db_internal_error("update health check failed", err))?;
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "environment": env_str,
        "passed": req.passed
    })))
}

pub async fn get_deployment_status(
    State(state): State<AppState>,
    Path(contract_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", contract_id),
            ),
            _ => db_internal_error("get contract", err),
        })?;

    let deployments: Vec<ContractDeployment> = sqlx::query_as(
        "SELECT * FROM contract_deployments 
         WHERE contract_id = $1 
         ORDER BY deployed_at DESC",
    )
    .bind(contract.id)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get deployments", err))?;

    let active = deployments.iter().find(|d| matches!(d.status, DeploymentStatus::Active));
    let blue = deployments.iter().find(|d| matches!(d.environment, DeploymentEnvironment::Blue));
    let green = deployments.iter().find(|d| matches!(d.environment, DeploymentEnvironment::Green));

    Ok(Json(serde_json::json!({
        "contract_id": contract_id,
        "active": active,
        "blue": blue,
        "green": green
    })))
}

pub async fn route_not_found() -> ApiError {
    ApiError::not_found("RouteNotFound", "The requested endpoint does not exist")
}

use std::time::Duration;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct CacheParams {
    pub cache: Option<String>,
}

pub async fn get_contract_state(
    State(state): State<AppState>,
    Path((contract_id, key)): Path<(String, String)>,
    Query(params): Query<CacheParams>,
) -> ApiResult<Json<serde_json::Value>> {
    let use_cache = params.cache.as_deref() == Some("on");

    // Try cache first if enabled
    if use_cache {
        let (cached_value, was_hit) = state.cache.get(&contract_id, &key).await;
        if was_hit && cached_value.is_some() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&cached_value.unwrap()) {
                 return Ok(Json(val));
            }
        }
    }

    // Cache miss or cache disabled - fetch fresh value
    let fetch_start = std::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(100)).await; // Simulate contract read latency
    let fetch_duration = fetch_start.elapsed();
    
    let value = serde_json::json!({ 
        "contract_id": contract_id,
        "key": key,
        "value": &format!("state_of_{}_{}", contract_id, key), 
        "fetched_at": &chrono::Utc::now().to_rfc3339() 
    });
    
    // Always record latency for baseline metrics
    if use_cache {
        // Record the full miss latency for metrics
        state.cache.put(&contract_id, &key, value.to_string(), None).await;
    } else {
        // Record as uncached baseline when cache=off
        state.cache.record_uncached_latency(fetch_duration);
    }

    Ok(Json(value))
}

pub async fn update_contract_state(
    State(state): State<AppState>,
    Path((contract_id, key)): Path<(String, String)>,
    Json(_payload): Json<serde_json::Value>,
) -> ApiResult<Json<serde_json::Value>> {
    tokio::time::sleep(Duration::from_millis(200)).await;
    state.cache.invalidate(&contract_id, &key).await;
    Ok(Json(serde_json::json!({ "status": "updated", "invalidated": true })))
}

pub async fn get_cache_stats(
    State(state): State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    let metrics = state.cache.metrics();
    let hits = metrics.hits.load(std::sync::atomic::Ordering::Relaxed);
    let misses = metrics.misses.load(std::sync::atomic::Ordering::Relaxed);
    let cached_hit_count = metrics.cached_hit_count.load(std::sync::atomic::Ordering::Relaxed);
    let cache_miss_count = metrics.cache_miss_count.load(std::sync::atomic::Ordering::Relaxed);
    
    Ok(Json(serde_json::json!({
        "metrics": {
            "hit_rate_percent": metrics.hit_rate(),
            "avg_cached_hit_latency_us": metrics.avg_cached_hit_latency(),
            "avg_cache_miss_latency_us": metrics.avg_cache_miss_latency(),
            "avg_uncached_latency_us": metrics.avg_uncached_latency(),
            "improvement_factor": metrics.improvement_factor(),
            "hits": hits,
            "misses": misses,
            "cached_hit_entries_count": cached_hit_count,
            "cache_miss_entries_count": cache_miss_count,
        },
        "config": {
            "enabled": state.cache.config().enabled,
            "policy": format!("{:?}", state.cache.config().policy),
            "ttl_seconds": state.cache.config().global_ttl.as_secs(),
            "max_capacity": state.cache.config().max_capacity,
        }
    })))
}

pub async fn create_ab_test(
    State(state): State<AppState>,
    payload: Result<Json<CreateAbTestRequest>, JsonRejection>,
) -> ApiResult<Json<AbTest>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", req.contract_id),
            ),
            _ => db_internal_error("get contract for ab test", err),
        })?;

    let variant_a_uuid = Uuid::parse_str(&req.variant_a_deployment_id).map_err(|_| {
        ApiError::bad_request(
            "InvalidDeploymentId",
            format!("Invalid variant A deployment ID: {}", req.variant_a_deployment_id),
        )
    })?;

    let variant_b_uuid = Uuid::parse_str(&req.variant_b_deployment_id).map_err(|_| {
        ApiError::bad_request(
            "InvalidDeploymentId",
            format!("Invalid variant B deployment ID: {}", req.variant_b_deployment_id),
        )
    })?;

    let traffic_split = Decimal::try_from(req.traffic_split.unwrap_or(50.0))
        .map_err(|_| ApiError::bad_request("InvalidSplit", "Invalid traffic split"))?;

    let significance_threshold = Decimal::try_from(req.significance_threshold.unwrap_or(95.0))
        .map_err(|_| ApiError::bad_request("InvalidThreshold", "Invalid significance threshold"))?;

    let mut tx = state.db.begin().await.map_err(|err| {
        db_internal_error("begin transaction for ab test", err)
    })?;

    let test: AbTest = sqlx::query_as(
        "INSERT INTO ab_tests (
            contract_id, name, description, status, traffic_split,
            variant_a_deployment_id, variant_b_deployment_id, primary_metric,
            hypothesis, significance_threshold, min_sample_size, created_by
        ) VALUES ($1, $2, $3, 'draft', $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING *",
    )
    .bind(contract.id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(traffic_split)
    .bind(variant_a_uuid)
    .bind(variant_b_uuid)
    .bind(&req.primary_metric)
    .bind(&req.hypothesis)
    .bind(significance_threshold)
    .bind(req.min_sample_size.unwrap_or(1000))
    .bind(&req.created_by)
    .fetch_one(&mut *tx)
    .await
    .map_err(|err| db_internal_error("create ab test", err))?;

    sqlx::query(
        "INSERT INTO ab_test_variants (test_id, variant_type, deployment_id, traffic_percentage)
         VALUES ($1, 'control', $2, $3), ($1, 'treatment', $4, $5)",
    )
    .bind(test.id)
    .bind(variant_a_uuid)
    .bind(traffic_split)
    .bind(variant_b_uuid)
    .bind(Decimal::from(100) - traffic_split)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("create variants", err))?;

    tx.commit().await.map_err(|err| {
        db_internal_error("commit ab test", err)
    })?;

    Ok(Json(test))
}

pub async fn start_ab_test(
    State(state): State<AppState>,
    Path(test_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let test_uuid = Uuid::parse_str(&test_id).map_err(|_| {
        ApiError::bad_request("InvalidTestId", format!("Invalid test ID: {}", test_id))
    })?;

    sqlx::query(
        "UPDATE ab_tests 
         SET status = 'running', started_at = NOW()
         WHERE id = $1 AND status = 'draft'",
    )
    .bind(test_uuid)
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("start ab test", err))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "test_id": test_id,
        "status": "running"
    })))
}

pub async fn get_variant(
    State(state): State<AppState>,
    payload: Result<Json<GetVariantRequest>, JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let test_uuid = Uuid::parse_str(&req.test_id).map_err(|_| {
        ApiError::bad_request("InvalidTestId", format!("Invalid test ID: {}", req.test_id))
    })?;

    let variant: Option<String> = sqlx::query_scalar(
        "SELECT assign_variant($1, $2)",
    )
    .bind(test_uuid)
    .bind(&req.user_address)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get variant", err))?;

    if let Some(v) = variant {
        Ok(Json(serde_json::json!({
            "variant": v,
            "test_id": req.test_id,
            "user_address": req.user_address
        })))
    } else {
        Err(ApiError::not_found(
            "TestNotFound",
            format!("Test not found or not running: {}", req.test_id),
        ))
    }
}

pub async fn record_ab_test_metric(
    State(state): State<AppState>,
    payload: Result<Json<RecordAbTestMetricRequest>, JsonRejection>,
) -> ApiResult<Json<AbTestMetric>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let test_uuid = Uuid::parse_str(&req.test_id).map_err(|_| {
        ApiError::bad_request("InvalidTestId", format!("Invalid test ID: {}", req.test_id))
    })?;

    let variant = if let Some(ref user_addr) = req.user_address {
        let assignment: Option<VariantType> = sqlx::query_as(
            "SELECT variant_type FROM ab_test_assignments 
             WHERE test_id = $1 AND user_address = $2",
        )
        .bind(test_uuid)
        .bind(user_addr)
        .fetch_optional(&state.db)
        .await
        .map_err(|err| db_internal_error("get assignment", err))?;

        assignment.unwrap_or_else(|| {
            let hash = (test_uuid.to_string() + user_addr).as_bytes();
            let hash_val = (hash.iter().map(|&b| b as u32).sum::<u32>() % 100) as i32;
            let test: AbTest = sqlx::query_as("SELECT * FROM ab_tests WHERE id = $1")
                .bind(test_uuid)
                .fetch_one(&state.db)
                .await
                .ok()
                .unwrap();
            
            if hash_val < test.traffic_split.to_string().parse::<i32>().unwrap_or(50) {
                VariantType::Control
            } else {
                VariantType::Treatment
            }
        })
    } else {
        return Err(ApiError::bad_request(
            "UserAddressRequired",
            "User address required for metric recording",
        ));
    };

    let metric_value = Decimal::try_from(req.metric_value)
        .map_err(|_| ApiError::bad_request("InvalidMetric", "Invalid metric value"))?;

    let metric: AbTestMetric = sqlx::query_as(
        "INSERT INTO ab_test_metrics (
            test_id, variant_type, metric_name, metric_value, user_address, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *",
    )
    .bind(test_uuid)
    .bind(&variant)
    .bind(&req.metric_name)
    .bind(metric_value)
    .bind(&req.user_address)
    .bind(&req.metadata)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("record metric", err))?;

    Ok(Json(metric))
}

pub async fn get_ab_test_results(
    State(state): State<AppState>,
    Path(test_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let test_uuid = Uuid::parse_str(&test_id).map_err(|_| {
        ApiError::bad_request("InvalidTestId", format!("Invalid test ID: {}", test_id))
    })?;

    let test: AbTest = sqlx::query_as("SELECT * FROM ab_tests WHERE id = $1")
        .bind(test_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "TestNotFound",
                format!("Test not found: {}", test_id),
            ),
            _ => db_internal_error("get test", err),
        })?;

    let control_metrics: Vec<AbTestMetric> = sqlx::query_as(
        "SELECT * FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'control' 
         ORDER BY timestamp DESC LIMIT 1000",
    )
    .bind(test_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get control metrics", err))?;

    let treatment_metrics: Vec<AbTestMetric> = sqlx::query_as(
        "SELECT * FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'treatment' 
         ORDER BY timestamp DESC LIMIT 1000",
    )
    .bind(test_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get treatment metrics", err))?;

    let control_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ab_test_assignments 
         WHERE test_id = $1 AND variant_type = 'control'",
    )
    .bind(test_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("count control", err))?;

    let treatment_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ab_test_assignments 
         WHERE test_id = $1 AND variant_type = 'treatment'",
    )
    .bind(test_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("count treatment", err))?;

    let control_mean: Option<Decimal> = sqlx::query_scalar(
        "SELECT AVG(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'control' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get control mean", err))?;

    let treatment_mean: Option<Decimal> = sqlx::query_scalar(
        "SELECT AVG(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'treatment' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get treatment mean", err))?;

    let control_std: Option<Decimal> = sqlx::query_scalar(
        "SELECT STDDEV(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'control' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get control std", err))?;

    let treatment_std: Option<Decimal> = sqlx::query_scalar(
        "SELECT STDDEV(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'treatment' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get treatment std", err))?;

    let control_n = control_metrics.len() as i64;
    let treatment_n = treatment_metrics.len() as i64;

    let p_value = if control_n >= 30 && treatment_n >= 30 {
        if let (Some(c_mean), Some(t_mean), Some(c_std), Some(t_std)) =
            (control_mean, treatment_mean, control_std, treatment_std)
        {
            let c_mean_f64 = c_mean.to_string().parse::<f64>().unwrap_or(0.0);
            let t_mean_f64 = t_mean.to_string().parse::<f64>().unwrap_or(0.0);
            let c_std_f64 = c_std.to_string().parse::<f64>().unwrap_or(0.0);
            let t_std_f64 = t_std.to_string().parse::<f64>().unwrap_or(0.0);

            let pooled_var = ((control_n - 1) as f64 * c_std_f64 * c_std_f64
                + (treatment_n - 1) as f64 * t_std_f64 * t_std_f64)
                / (control_n + treatment_n - 2) as f64;

            if pooled_var > 0.0 {
                let pooled_std = pooled_var.sqrt();
                let se = pooled_std * (1.0 / control_n as f64 + 1.0 / treatment_n as f64).sqrt();
                let t_stat = (t_mean_f64 - c_mean_f64) / se;

                let p_val = 2.0 * (1.0 - normal_cdf_approx_f64(t_stat.abs()));
                Decimal::from_str(&format!("{:.6}", p_val)).ok()
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let significance = p_value.and_then(|p| {
        if p < Decimal::from_str("0.01").unwrap() {
            Some(Decimal::from(99))
        } else if p < Decimal::from_str("0.05").unwrap() {
            Some(Decimal::from(95))
        } else if p < Decimal::from_str("0.10").unwrap() {
            Some(Decimal::from(90))
        } else {
            Some(Decimal::ZERO)
        }
    });

    let winner = if let (Some(c_mean), Some(t_mean)) = (control_mean, treatment_mean) {
        if t_mean > c_mean && significance.is_some() && significance.unwrap() >= test.significance_threshold {
            Some("treatment")
        } else if c_mean > t_mean && significance.is_some() && significance.unwrap() >= test.significance_threshold {
            Some("control")
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "test": test,
        "control": {
            "users": control_count,
            "metrics_count": control_n,
            "mean": control_mean,
            "std_dev": control_std
        },
        "treatment": {
            "users": treatment_count,
            "metrics_count": treatment_n,
            "mean": treatment_mean,
            "std_dev": treatment_std
        },
        "statistics": {
            "p_value": p_value,
            "significance": significance,
            "winner": winner
        }
    })))
}

fn normal_cdf_approx_f64(x: f64) -> f64 {
    let sqrt_2 = 1.4142135623730951;
    let erf_val = erf_approx_f64(x / sqrt_2);
    0.5 * (1.0 + erf_val)
}

fn erf_approx_f64(x: f64) -> f64 {
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x_abs = x.abs();
    let t = 1.0 / (1.0 + p * x_abs);
    let y = 1.0 - (((((a5 * t + a4) * t + a3) * t + a2) * t + a1) * t) * (-x_abs * x_abs).exp();
    sign * y
}

pub async fn rollout_winning_variant(
    State(state): State<AppState>,
    Path(test_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let test_uuid = Uuid::parse_str(&test_id).map_err(|_| {
        ApiError::bad_request("InvalidTestId", format!("Invalid test ID: {}", test_id))
    })?;

    let test: AbTest = sqlx::query_as("SELECT * FROM ab_tests WHERE id = $1")
        .bind(test_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "TestNotFound",
                format!("Test not found: {}", test_id),
            ),
            _ => db_internal_error("get test", err),
        })?;

    let results = get_ab_test_results_internal(&state, test_uuid).await?;
    let winner = results["statistics"]["winner"].as_str();

    if winner.is_none() {
        return Err(ApiError::bad_request(
            "NoWinner",
            "No statistically significant winner found",
        ));
    }

    let winning_variant = winner.unwrap();
    let deployment_id = if winning_variant == "treatment" {
        test.variant_b_deployment_id
    } else {
        test.variant_a_deployment_id
    };

    let mut tx = state.db.begin().await.map_err(|err| {
        db_internal_error("begin transaction for rollout", err)
    })?;

    sqlx::query(
        "UPDATE ab_tests 
         SET status = 'completed', ended_at = NOW()
         WHERE id = $1",
    )
    .bind(test_uuid)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("complete test", err))?;

    sqlx::query(
        "UPDATE contract_deployments 
         SET status = 'active', activated_at = NOW()
         WHERE id = $1",
    )
    .bind(deployment_id)
    .execute(&mut *tx)
    .await
    .map_err(|err| db_internal_error("activate deployment", err))?;

    tx.commit().await.map_err(|err| {
        db_internal_error("commit rollout", err)
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "test_id": test_id,
        "winner": winning_variant,
        "deployment_id": deployment_id,
        "rolled_out": true
    })))
}

async fn get_ab_test_results_internal(
    state: &AppState,
    test_uuid: Uuid,
) -> Result<serde_json::Value, ApiError> {
    let test: AbTest = sqlx::query_as("SELECT * FROM ab_tests WHERE id = $1")
        .bind(test_uuid)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "TestNotFound",
                format!("Test not found: {}", test_uuid),
            ),
            _ => db_internal_error("get test", err),
        })?;

    let control_mean: Option<Decimal> = sqlx::query_scalar(
        "SELECT AVG(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'control' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get control mean", err))?;

    let treatment_mean: Option<Decimal> = sqlx::query_scalar(
        "SELECT AVG(metric_value) FROM ab_test_metrics 
         WHERE test_id = $1 AND variant_type = 'treatment' AND metric_name = $2",
    )
    .bind(test_uuid)
    .bind(&test.primary_metric)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_internal_error("get treatment mean", err))?;

    Ok(serde_json::json!({
        "statistics": {
            "winner": if let (Some(c), Some(t)) = (control_mean, treatment_mean) {
                if t > c { Some("treatment") } else { Some("control") }
            } else { None }
        }
    }))
}

pub async fn record_performance_metric(
    State(state): State<AppState>,
    payload: Result<Json<RecordPerformanceMetricRequest>, JsonRejection>,
) -> ApiResult<Json<PerformanceMetric>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", req.contract_id),
            ),
            _ => db_internal_error("get contract for metric", err),
        })?;

    let value = Decimal::try_from(req.value)
        .map_err(|_| ApiError::bad_request("InvalidMetric", "Invalid metric value"))?;

    let p50 = req.p50.and_then(|v| Decimal::try_from(v).ok());
    let p95 = req.p95.and_then(|v| Decimal::try_from(v).ok());
    let p99 = req.p99.and_then(|v| Decimal::try_from(v).ok());

    let metric: PerformanceMetric = sqlx::query_as(
        "INSERT INTO performance_metrics (
            contract_id, metric_type, function_name, value, p50, p95, p99, metadata
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING *",
    )
    .bind(contract.id)
    .bind(&req.metric_type)
    .bind(&req.function_name)
    .bind(value)
    .bind(p50)
    .bind(p95)
    .bind(p99)
    .bind(&req.metadata)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("record performance metric", err))?;

    Ok(Json(metric))
}

pub async fn get_contract_performance(
    State(state): State<AppState>,
    Path(contract_id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<serde_json::Value>> {
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", contract_id),
            ),
            _ => db_internal_error("get contract", err),
        })?;

    let timeframe = params.get("timeframe").map(|s| s.as_str()).unwrap_or("7d");
    let start_time = parse_timeframe(timeframe);

    let metrics: Vec<PerformanceMetric> = sqlx::query_as(
        "SELECT * FROM performance_metrics 
         WHERE contract_id = $1 AND timestamp >= $2
         ORDER BY timestamp DESC",
    )
    .bind(contract.id)
    .bind(start_time)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get metrics", err))?;

    let anomalies: Vec<PerformanceAnomaly> = sqlx::query_as(
        "SELECT * FROM performance_anomalies 
         WHERE contract_id = $1 AND detected_at >= $2 AND resolved = FALSE
         ORDER BY detected_at DESC",
    )
    .bind(contract.id)
    .bind(start_time)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get anomalies", err))?;

    let alerts: Vec<PerformanceAlert> = sqlx::query_as(
        "SELECT * FROM performance_alerts 
         WHERE contract_id = $1 AND triggered_at >= $2 AND resolved = FALSE
         ORDER BY triggered_at DESC",
    )
    .bind(contract.id)
    .bind(start_time)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get alerts", err))?;

    let trends: Vec<PerformanceTrend> = sqlx::query_as(
        "SELECT * FROM performance_trends 
         WHERE contract_id = $1 AND timeframe_start >= $2
         ORDER BY timeframe_start DESC",
    )
    .bind(contract.id)
    .bind(start_time)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get trends", err))?;

    let slow_functions = identify_slow_functions(&metrics);

    Ok(Json(serde_json::json!({
        "contract_id": contract_id,
        "timeframe": timeframe,
        "metrics": metrics,
        "anomalies": anomalies,
        "alerts": alerts,
        "trends": trends,
        "slow_functions": slow_functions,
        "summary": {
            "total_metrics": metrics.len(),
            "active_anomalies": anomalies.len(),
            "active_alerts": alerts.len(),
            "trends_analyzed": trends.len()
        }
    })))
}

fn parse_timeframe(timeframe: &str) -> DateTime<Utc> {
    let now = chrono::Utc::now();
    let duration = if timeframe.ends_with('d') {
        let days: i64 = timeframe[..timeframe.len() - 1].parse().unwrap_or(7);
        chrono::Duration::days(days)
    } else if timeframe.ends_with('h') {
        let hours: i64 = timeframe[..timeframe.len() - 1].parse().unwrap_or(24);
        chrono::Duration::hours(hours)
    } else if timeframe.ends_with('m') {
        let minutes: i64 = timeframe[..timeframe.len() - 1].parse().unwrap_or(60);
        chrono::Duration::minutes(minutes)
    } else {
        chrono::Duration::days(7)
    };
    now - duration
}

fn identify_slow_functions(metrics: &[PerformanceMetric]) -> Vec<serde_json::Value> {
    use std::collections::HashMap;
    let mut function_stats: HashMap<String, (Vec<f64>, usize)> = HashMap::new();

    for metric in metrics {
        if metric.metric_type == MetricType::ExecutionTime {
            if let Some(ref func_name) = metric.function_name {
                let entry = function_stats.entry(func_name.clone()).or_insert_with(|| (Vec::new(), 0));
                if let Ok(val) = metric.value.to_string().parse::<f64>() {
                    entry.0.push(val);
                }
                if let Some(ref p99) = metric.p99 {
                    if let Ok(p99_val) = p99.to_string().parse::<f64>() {
                        entry.0.push(p99_val);
                    }
                }
                entry.1 += 1;
            }
        }
    }

    let mut slow_functions: Vec<_> = function_stats
        .into_iter()
        .map(|(name, (values, count))| {
            let sum: f64 = values.iter().sum();
            let avg = if values.is_empty() { 0.0 } else { sum / values.len() as f64 };
            let max = values.iter().copied().fold(0.0, f64::max);
            serde_json::json!({
                "function_name": name,
                "avg_execution_time": avg,
                "max_execution_time": max,
                "sample_count": count
            })
        })
        .collect();

    slow_functions.sort_by(|a, b| {
        let a_val = a["avg_execution_time"].as_f64().unwrap_or(0.0);
        let b_val = b["avg_execution_time"].as_f64().unwrap_or(0.0);
        b_val.partial_cmp(&a_val).unwrap_or(std::cmp::Ordering::Equal)
    });

    slow_functions.into_iter().take(10).collect()
}

pub async fn create_alert_config(
    State(state): State<AppState>,
    payload: Result<Json<CreateAlertConfigRequest>, JsonRejection>,
) -> ApiResult<Json<PerformanceAlertConfig>> {
    let Json(req) = payload.map_err(map_json_rejection)?;

    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&req.contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", req.contract_id),
            ),
            _ => db_internal_error("get contract", err),
        })?;

    let threshold_value = Decimal::try_from(req.threshold_value)
        .map_err(|_| ApiError::bad_request("InvalidThreshold", "Invalid threshold value"))?;

    let severity = req.severity.unwrap_or(AlertSeverity::Warning);

    let config: PerformanceAlertConfig = sqlx::query_as(
        "INSERT INTO performance_alert_configs (
            contract_id, metric_type, threshold_type, threshold_value, severity
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (contract_id, metric_type, threshold_type)
        DO UPDATE SET threshold_value = EXCLUDED.threshold_value,
                      severity = EXCLUDED.severity,
                      updated_at = NOW()
        RETURNING *",
    )
    .bind(contract.id)
    .bind(&req.metric_type)
    .bind(&req.threshold_type)
    .bind(threshold_value)
    .bind(&severity)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_internal_error("create alert config", err))?;

    Ok(Json(config))
}

pub async fn get_performance_anomalies(
    State(state): State<AppState>,
    Path(contract_id): Path<String>,
) -> ApiResult<Json<Vec<PerformanceAnomaly>>> {
    let contract: Contract = sqlx::query_as("SELECT * FROM contracts WHERE contract_id = $1")
        .bind(&contract_id)
        .fetch_one(&state.db)
        .await
        .map_err(|err| match err {
            sqlx::Error::RowNotFound => ApiError::not_found(
                "ContractNotFound",
                format!("Contract not found: {}", contract_id),
            ),
            _ => db_internal_error("get contract", err),
        })?;

    let anomalies: Vec<PerformanceAnomaly> = sqlx::query_as(
        "SELECT * FROM performance_anomalies 
         WHERE contract_id = $1 AND resolved = FALSE
         ORDER BY detected_at DESC",
    )
    .bind(contract.id)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_internal_error("get anomalies", err))?;

    Ok(Json(anomalies))
}

pub async fn acknowledge_alert(
    State(state): State<AppState>,
    Path(alert_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let alert_uuid = Uuid::parse_str(&alert_id).map_err(|_| {
        ApiError::bad_request("InvalidAlertId", format!("Invalid alert ID: {}", alert_id))
    })?;

    sqlx::query(
        "UPDATE performance_alerts 
         SET acknowledged = TRUE, acknowledged_at = NOW()
         WHERE id = $1",
    )
    .bind(alert_uuid)
    .execute(&state.db)
    .await
    .map_err(|err| db_internal_error("acknowledge alert", err))?;

    Ok(Json(serde_json::json!({
        "success": true,
        "alert_id": alert_id
    })))
}
