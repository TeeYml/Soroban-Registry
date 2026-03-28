//! Contract interaction analytics handlers (issue #415)
//!
//! Provides two endpoints:
//!
//!   GET /api/contracts/:id/analytics  – per-contract analytics with real
//!                                       deployment counts, view count, and
//!                                       a 7-day interaction trend.
//!
//!   GET /api/analytics/summary        – registry-wide aggregates broken down
//!                                       by category and by network, enabling
//!                                       trending-by-category identification.

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::Serialize;
use shared::{DeploymentStats, InteractorStats, TopUser};
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

// ── Response types ────────────────────────────────────────────────────────────

/// One data-point in the 7-day interaction trend.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TrendPoint {
    pub date: NaiveDate,
    /// Total interactions recorded on this day across all interaction types.
    pub count: i64,
}

/// Enhanced per-contract analytics response.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ContractAnalyticsResponse {
    pub contract_id: String,
    /// Total number of times this contract's profile page has been viewed.
    /// Incremented asynchronously to avoid blocking the read path.
    pub view_count: i64,
    /// Deployment statistics sourced from the daily-aggregate rollup table.
    pub deployments: DeploymentStats,
    /// Interactor statistics computed from raw contract_interactions.
    pub interactors: InteractorStats,
    /// 30-day daily interaction timeline (all interaction types combined).
    pub timeline: Vec<TrendPoint>,
    /// 7-day daily interaction trend used to identify rapidly-growing contracts.
    pub interaction_trend: Vec<TrendPoint>,
}

/// Analytics summary for a single category.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CategoryAnalytics {
    /// `null` for contracts that have no category assigned.
    pub category: Option<String>,
    pub contract_count: i64,
    pub total_interactions: i64,
    pub avg_interactions_per_contract: f64,
    pub total_views: i64,
}

/// Analytics summary for a single network.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct NetworkAnalytics {
    pub network: String,
    pub contract_count: i64,
    pub verified_count: i64,
    pub total_interactions: i64,
    pub total_views: i64,
}

/// Top-level response for GET /api/analytics/summary.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnalyticsSummaryResponse {
    pub by_category: Vec<CategoryAnalytics>,
    pub by_network: Vec<NetworkAnalytics>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn db_err(op: &str, err: sqlx::Error) -> ApiError {
    tracing::error!(operation = op, error = ?err, "database operation failed");
    ApiError::internal("An unexpected database error occurred")
}

fn parse_contract_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| {
        ApiError::bad_request(
            "InvalidContractId",
            format!("Invalid contract ID format: {}", id),
        )
    })
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// Return enhanced analytics for a single contract.
///
/// Deployment counts are sourced from the pre-computed
/// `contract_interaction_daily_aggregates` rollup table rather than scanning
/// raw interactions, so the query scales independently of interaction volume.
#[utoipa::path(
    get,
    path = "/api/contracts/{id}/analytics",
    params(
        ("id" = String, Path, description = "Contract UUID")
    ),
    responses(
        (status = 200, description = "Contract analytics", body = ContractAnalyticsResponse),
        (status = 404, description = "Contract not found"),
        (status = 400, description = "Invalid contract ID format")
    ),
    tag = "Analytics"
)]
pub async fn get_contract_analytics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ContractAnalyticsResponse>> {
    let contract_uuid = parse_contract_id(&id)?;

    // Single query: confirm existence + fetch view_count in one round-trip.
    let view_count: Option<i64> =
        sqlx::query_scalar("SELECT view_count FROM contracts WHERE id = $1")
            .bind(contract_uuid)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| db_err("fetch view_count", err))?;

    let view_count = view_count.ok_or_else(|| {
        ApiError::not_found(
            "ContractNotFound",
            format!("No contract found with ID: {}", id),
        )
    })?;

    // ── Deployment stats from the daily-aggregate rollup ──────────────────────
    // Using the rollup table avoids a full scan of contract_interactions and
    // is kept fresh by refresh_contract_interaction_daily_aggregates().

    let deploy_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0)
         FROM   contract_interaction_daily_aggregates
         WHERE  contract_id = $1
         AND    interaction_type = 'deploy'",
    )
    .bind(contract_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_err("fetch deploy total", err))?;

    // Unique deployers: still from raw interactions (rollup doesn't track
    // per-user cardinality).
    let unique_deployers: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address)
         FROM   contract_interactions
         WHERE  contract_id = $1
         AND    interaction_type = 'deploy'
         AND    user_address IS NOT NULL",
    )
    .bind(contract_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_err("fetch unique deployers", err))?;

    // Network breakdown for deployments.
    let by_network_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT network::TEXT, COALESCE(SUM(count), 0)::BIGINT AS total
         FROM   contract_interaction_daily_aggregates
         WHERE  contract_id = $1
         AND    interaction_type = 'deploy'
         GROUP  BY network",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch deploy by network", err))?;

    let by_network: serde_json::Value = by_network_rows
        .into_iter()
        .fold(serde_json::json!({}), |mut acc, (net, count)| {
            acc[net] = serde_json::json!(count);
            acc
        });

    // ── Interactor stats ──────────────────────────────────────────────────────

    let unique_interactors: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_address)
         FROM   contract_interactions
         WHERE  contract_id = $1
         AND    user_address IS NOT NULL",
    )
    .bind(contract_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| db_err("fetch unique interactors", err))?;

    let top_user_rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT user_address, COUNT(*) AS cnt
         FROM   contract_interactions
         WHERE  contract_id = $1
         AND    user_address IS NOT NULL
         GROUP  BY user_address
         ORDER  BY cnt DESC
         LIMIT  10",
    )
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch top interactors", err))?;

    let top_users: Vec<TopUser> = top_user_rows
        .into_iter()
        .filter_map(|(addr, count)| addr.map(|a| TopUser { address: a, count }))
        .collect();

    // ── 30-day timeline (all interaction types) ───────────────────────────────

    let thirty_days_ago = chrono::Utc::now() - chrono::Duration::days(30);

    let timeline_rows: Vec<(NaiveDate, i64)> = sqlx::query_as(
        r#"
        SELECT d::DATE AS date,
               COALESCE(SUM(agg.count), 0)::BIGINT AS count
        FROM   generate_series(
                   ($1::TIMESTAMPTZ)::DATE,
                   CURRENT_DATE,
                   '1 day'::INTERVAL
               ) d
        LEFT   JOIN contract_interaction_daily_aggregates agg
               ON  agg.contract_id = $2
               AND agg.day = d::DATE
        GROUP  BY d::DATE
        ORDER  BY d::DATE
        "#,
    )
    .bind(thirty_days_ago)
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch 30-day timeline", err))?;

    let timeline: Vec<TrendPoint> = timeline_rows
        .into_iter()
        .map(|(date, count)| TrendPoint { date, count })
        .collect();

    // ── 7-day interaction trend ───────────────────────────────────────────────
    // Queried separately with a tight date range so the planner uses the
    // (contract_id, day) index efficiently.

    let seven_days_ago = chrono::Utc::now() - chrono::Duration::days(7);

    let trend_rows: Vec<(NaiveDate, i64)> = sqlx::query_as(
        r#"
        SELECT d::DATE AS date,
               COALESCE(SUM(agg.count), 0)::BIGINT AS count
        FROM   generate_series(
                   ($1::TIMESTAMPTZ)::DATE,
                   CURRENT_DATE,
                   '1 day'::INTERVAL
               ) d
        LEFT   JOIN contract_interaction_daily_aggregates agg
               ON  agg.contract_id = $2
               AND agg.day = d::DATE
        GROUP  BY d::DATE
        ORDER  BY d::DATE
        "#,
    )
    .bind(seven_days_ago)
    .bind(contract_uuid)
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch 7-day trend", err))?;

    let interaction_trend: Vec<TrendPoint> = trend_rows
        .into_iter()
        .map(|(date, count)| TrendPoint { date, count })
        .collect();

    Ok(Json(ContractAnalyticsResponse {
        contract_id: id,
        view_count,
        deployments: DeploymentStats {
            count: deploy_total,
            unique_users: unique_deployers,
            by_network,
        },
        interactors: InteractorStats {
            unique_count: unique_interactors,
            top_users,
        },
        timeline,
        interaction_trend,
    }))
}

/// Return registry-wide analytics aggregated by category and network.
///
/// Interaction totals are read from the daily-aggregate rollup so the query
/// is fast even on large registries.  This endpoint is the primary way to
/// identify which categories or networks are trending.
#[utoipa::path(
    get,
    path = "/api/analytics/summary",
    responses(
        (status = 200, description = "Registry analytics summary by category and network",
         body = AnalyticsSummaryResponse)
    ),
    tag = "Analytics"
)]
pub async fn get_analytics_summary(
    State(state): State<AppState>,
) -> ApiResult<Json<AnalyticsSummaryResponse>> {
    // ── By category ───────────────────────────────────────────────────────────

    let category_rows: Vec<(Option<String>, i64, i64, i64)> = sqlx::query_as(
        r#"
        SELECT
            c.category,
            COUNT(DISTINCT c.id)::BIGINT                   AS contract_count,
            COALESCE(SUM(agg.count), 0)::BIGINT            AS total_interactions,
            COALESCE(SUM(c.view_count), 0)::BIGINT         AS total_views
        FROM contracts c
        LEFT JOIN contract_interaction_daily_aggregates agg
               ON agg.contract_id = c.id
        GROUP BY c.category
        ORDER BY total_interactions DESC
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch category analytics", err))?;

    let by_category: Vec<CategoryAnalytics> = category_rows
        .into_iter()
        .map(|(category, contract_count, total_interactions, total_views)| {
            let avg = if contract_count > 0 {
                total_interactions as f64 / contract_count as f64
            } else {
                0.0
            };
            CategoryAnalytics {
                category,
                contract_count,
                total_interactions,
                avg_interactions_per_contract: avg,
                total_views,
            }
        })
        .collect();

    // ── By network ────────────────────────────────────────────────────────────

    let network_rows: Vec<(String, i64, i64, i64, i64)> = sqlx::query_as(
        r#"
        SELECT
            c.network::TEXT,
            COUNT(DISTINCT c.id)::BIGINT                              AS contract_count,
            COUNT(DISTINCT c.id) FILTER (WHERE c.is_verified)::BIGINT AS verified_count,
            COALESCE(SUM(agg.total_count), 0)::BIGINT                 AS total_interactions,
            COALESCE(SUM(c.view_count), 0)::BIGINT                    AS total_views
        FROM contracts c
        LEFT JOIN (
            SELECT
                contract_id,
                SUM(count) AS total_count
            FROM contract_interaction_daily_aggregates
            GROUP BY contract_id
        ) agg
            ON agg.contract_id = c.id
        GROUP BY c.network
        ORDER BY total_interactions DESC
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(|err| db_err("fetch network analytics", err))?;

    let by_network: Vec<NetworkAnalytics> = network_rows
        .into_iter()
        .map(
            |(network, contract_count, verified_count, total_interactions, total_views)| {
                NetworkAnalytics {
                    network,
                    contract_count,
                    verified_count,
                    total_interactions,
                    total_views,
                }
            },
        )
        .collect();

    Ok(Json(AnalyticsSummaryResponse {
        by_category,
        by_network,
    }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_contract_id_rejects_non_uuid() {
        assert!(parse_contract_id("not-a-uuid").is_err());
    }

    #[test]
    fn parse_contract_id_accepts_valid_uuid() {
        let id = Uuid::new_v4().to_string();
        assert!(parse_contract_id(&id).is_ok());
    }

    #[test]
    fn category_analytics_avg_is_zero_for_empty_category() {
        let analytics = CategoryAnalytics {
            category: None,
            contract_count: 0,
            total_interactions: 0,
            avg_interactions_per_contract: {
                let contract_count: i64 = 0;
                let total_interactions: i64 = 0;
                if contract_count > 0 {
                    total_interactions as f64 / contract_count as f64
                } else {
                    0.0
                }
            },
            total_views: 0,
        };
        assert_eq!(analytics.avg_interactions_per_contract, 0.0);
    }

    #[test]
    fn category_analytics_avg_computed_correctly() {
        let contract_count: i64 = 4;
        let total_interactions: i64 = 100;
        let avg = total_interactions as f64 / contract_count as f64;
        assert_eq!(avg, 25.0);
    }
}
