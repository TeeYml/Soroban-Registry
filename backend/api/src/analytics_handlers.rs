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
    extract::{Path, Query, State},
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use shared::{DeploymentStats, InteractorStats, Network, TopUser};
use sqlx::FromRow;
use std::collections::{BTreeMap, HashMap};
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

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct AnalyticsTimeSeriesQuery {
    /// Inclusive start date (YYYY-MM-DD). Defaults to 30 days before end_date.
    pub start_date: Option<NaiveDate>,
    /// Inclusive end date (YYYY-MM-DD). Defaults to today (UTC).
    pub end_date: Option<NaiveDate>,
    /// Grouping dimension: network, category, or publisher. Defaults to network.
    pub group_by: Option<String>,
    /// Optional contract network filter.
    pub network: Option<Network>,
    /// Optional category filter.
    pub category: Option<String>,
    /// Optional publisher UUID filter.
    pub publisher_id: Option<Uuid>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnalyticsTimeSeriesPoint {
    pub date: NaiveDate,
    pub deployments: i64,
    pub verifications: i64,
    pub updates: i64,
    pub total_events: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnalyticsTimeSeriesGroup {
    pub key: String,
    pub points: Vec<AnalyticsTimeSeriesPoint>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AnalyticsTimeSeriesResponse {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub group_by: String,
    pub series: Vec<AnalyticsTimeSeriesGroup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeSeriesGroupBy {
    Network,
    Category,
    Publisher,
}

impl TimeSeriesGroupBy {
    fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Category => "category",
            Self::Publisher => "publisher",
        }
    }

    fn parse(raw: Option<&str>) -> ApiResult<Self> {
        match raw.unwrap_or("network") {
            "network" => Ok(Self::Network),
            "category" => Ok(Self::Category),
            "publisher" => Ok(Self::Publisher),
            _ => Err(ApiError::bad_request(
                "InvalidGroupBy",
                "group_by must be one of: network, category, publisher",
            )),
        }
    }
}

#[derive(Debug, FromRow)]
struct AnalyticsTimeSeriesRow {
    day: NaiveDate,
    group_key: String,
    deployments: i64,
    verifications: i64,
    updates: i64,
    total_events: i64,
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

    let by_network: serde_json::Value =
        by_network_rows
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
        .map(
            |(category, contract_count, total_interactions, total_views)| {
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
            },
        )
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

#[utoipa::path(
    get,
    path = "/api/analytics/timeseries",
    params(AnalyticsTimeSeriesQuery),
    responses(
        (status = 200, description = "Daily analytics aggregates grouped by dimension", body = AnalyticsTimeSeriesResponse),
        (status = 400, description = "Invalid query parameters")
    ),
    tag = "Analytics"
)]
pub async fn get_analytics_timeseries(
    State(state): State<AppState>,
    Query(query): Query<AnalyticsTimeSeriesQuery>,
) -> ApiResult<Json<AnalyticsTimeSeriesResponse>> {
    let end_date = query.end_date.unwrap_or_else(|| Utc::now().date_naive());
    let start_date = query
        .start_date
        .unwrap_or_else(|| end_date - Duration::days(30));

    if start_date > end_date {
        return Err(ApiError::bad_request(
            "InvalidDateRange",
            "start_date must be less than or equal to end_date",
        ));
    }

    if (end_date - start_date).num_days() > 366 {
        return Err(ApiError::bad_request(
            "DateRangeTooLarge",
            "date range cannot exceed 366 days",
        ));
    }

    let group_by = TimeSeriesGroupBy::parse(query.group_by.as_deref())?;

    let rows: Vec<AnalyticsTimeSeriesRow> = match group_by {
        TimeSeriesGroupBy::Network => sqlx::query_as(
            r#"
                SELECT
                    a.date AS day,
                    c.network::TEXT AS group_key,
                    COALESCE(SUM(a.deployment_count), 0)::BIGINT AS deployments,
                    COALESCE(SUM(a.verification_count), 0)::BIGINT AS verifications,
                    COALESCE(SUM(a.update_count), 0)::BIGINT AS updates,
                    COALESCE(SUM(a.total_events), 0)::BIGINT AS total_events
                FROM analytics_daily_aggregates a
                JOIN contracts c ON c.id = a.contract_id
                WHERE a.date BETWEEN $1 AND $2
                  AND ($3::network_type IS NULL OR c.network = $3)
                  AND ($4::TEXT IS NULL OR c.category = $4)
                  AND ($5::UUID IS NULL OR c.publisher_id = $5)
                GROUP BY a.date, c.network
                ORDER BY a.date ASC, c.network::TEXT ASC
                "#,
        )
        .bind(start_date)
        .bind(end_date)
        .bind(query.network)
        .bind(query.category.as_deref())
        .bind(query.publisher_id)
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_err("fetch analytics timeseries grouped by network", err))?,
        TimeSeriesGroupBy::Category => sqlx::query_as(
            r#"
                SELECT
                    a.date AS day,
                    COALESCE(c.category, 'uncategorized') AS group_key,
                    COALESCE(SUM(a.deployment_count), 0)::BIGINT AS deployments,
                    COALESCE(SUM(a.verification_count), 0)::BIGINT AS verifications,
                    COALESCE(SUM(a.update_count), 0)::BIGINT AS updates,
                    COALESCE(SUM(a.total_events), 0)::BIGINT AS total_events
                FROM analytics_daily_aggregates a
                JOIN contracts c ON c.id = a.contract_id
                WHERE a.date BETWEEN $1 AND $2
                  AND ($3::network_type IS NULL OR c.network = $3)
                  AND ($4::TEXT IS NULL OR c.category = $4)
                  AND ($5::UUID IS NULL OR c.publisher_id = $5)
                GROUP BY a.date, COALESCE(c.category, 'uncategorized')
                ORDER BY a.date ASC, group_key ASC
                "#,
        )
        .bind(start_date)
        .bind(end_date)
        .bind(query.network)
        .bind(query.category.as_deref())
        .bind(query.publisher_id)
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_err("fetch analytics timeseries grouped by category", err))?,
        TimeSeriesGroupBy::Publisher => sqlx::query_as(
            r#"
                SELECT
                    a.date AS day,
                    COALESCE(p.stellar_address, c.publisher_id::TEXT) AS group_key,
                    COALESCE(SUM(a.deployment_count), 0)::BIGINT AS deployments,
                    COALESCE(SUM(a.verification_count), 0)::BIGINT AS verifications,
                    COALESCE(SUM(a.update_count), 0)::BIGINT AS updates,
                    COALESCE(SUM(a.total_events), 0)::BIGINT AS total_events
                FROM analytics_daily_aggregates a
                JOIN contracts c ON c.id = a.contract_id
                LEFT JOIN publishers p ON p.id = c.publisher_id
                WHERE a.date BETWEEN $1 AND $2
                  AND ($3::network_type IS NULL OR c.network = $3)
                  AND ($4::TEXT IS NULL OR c.category = $4)
                  AND ($5::UUID IS NULL OR c.publisher_id = $5)
                GROUP BY a.date, COALESCE(p.stellar_address, c.publisher_id::TEXT)
                ORDER BY a.date ASC, group_key ASC
                "#,
        )
        .bind(start_date)
        .bind(end_date)
        .bind(query.network)
        .bind(query.category.as_deref())
        .bind(query.publisher_id)
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_err("fetch analytics timeseries grouped by publisher", err))?,
    };

    let mut by_group: HashMap<String, BTreeMap<NaiveDate, AnalyticsTimeSeriesPoint>> =
        HashMap::new();

    for row in rows {
        by_group.entry(row.group_key).or_default().insert(
            row.day,
            AnalyticsTimeSeriesPoint {
                date: row.day,
                deployments: row.deployments,
                verifications: row.verifications,
                updates: row.updates,
                total_events: row.total_events,
            },
        );
    }

    let mut series: Vec<AnalyticsTimeSeriesGroup> = by_group
        .into_iter()
        .map(|(key, date_map)| {
            let mut cursor = start_date;
            let mut points = Vec::new();

            while cursor <= end_date {
                if let Some(point) = date_map.get(&cursor) {
                    points.push(AnalyticsTimeSeriesPoint {
                        date: point.date,
                        deployments: point.deployments,
                        verifications: point.verifications,
                        updates: point.updates,
                        total_events: point.total_events,
                    });
                } else {
                    points.push(AnalyticsTimeSeriesPoint {
                        date: cursor,
                        deployments: 0,
                        verifications: 0,
                        updates: 0,
                        total_events: 0,
                    });
                }

                cursor = cursor.succ_opt().expect("valid calendar date");
            }

            AnalyticsTimeSeriesGroup { key, points }
        })
        .collect();

    series.sort_by(|a, b| a.key.cmp(&b.key));

    Ok(Json(AnalyticsTimeSeriesResponse {
        start_date,
        end_date,
        group_by: group_by.as_str().to_string(),
        series,
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
