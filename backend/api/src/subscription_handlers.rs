// Contract Subscription & Notification Handlers (#493)
// Enable users to subscribe to alerts for contract updates and changes

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use uuid::Uuid;

use crate::{
    auth,
    error::{ApiError, ApiResult},
    state::AppState,
};
use shared::{
    ContractSubscription, ContractSubscriptionSummary, CreateWebhookRequest,
    NotificationChannel, NotificationFrequency, NotificationQueueItem, NotificationType,
    SubscribeRequest, SubscriptionStatus, UpdateSubscriptionRequest,
    UpdateUserNotificationPreferencesRequest, UserNotificationPreferences,
    UserSubscriptionsResponse, WebhookConfiguration,
};

// ─── Query / response types ────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListSubscriptionsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct NotificationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub unread_only: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub struct NotificationListResponse {
    pub notifications: Vec<NotificationQueueItem>,
    pub total_count: i64,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct StatisticsQuery {
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
}

#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct DeliveriesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct WebhookDeliveryLog {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub notification_id: Option<Uuid>,
    pub event_type: String,
    pub status: String,
    pub response_code: Option<i32>,
    pub response_body: Option<String>,
    pub error_message: Option<String>,
    pub attempt_number: i32,
    pub delivery_duration_ms: Option<i64>,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, serde::Serialize)]
pub struct WebhookDeliveriesResponse {
    pub deliveries: Vec<WebhookDeliveryLog>,
    pub total_count: i64,
}

// ─── Subscribe / Unsubscribe ───────────────────────────────────────────────

/// POST /api/contracts/:id/subscribe
pub async fn subscribe_to_contract(
    State(state): State<AppState>,
    Path(contract_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<SubscribeRequest>,
) -> ApiResult<Json<ContractSubscription>> {
    let contract_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contracts WHERE id = $1)")
            .bind(contract_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !contract_exists {
        return Err(ApiError::not_found("contract", "Contract not found"));
    }

    let user_id = auth_user.publisher_id;

    let notification_types = req.notification_types.unwrap_or(vec![
        NotificationType::NewVersion,
        NotificationType::VerificationStatus,
        NotificationType::SecurityIssue,
    ]);
    let channels = req.channels.unwrap_or(vec![NotificationChannel::InApp]);
    let frequency = req.frequency.unwrap_or(NotificationFrequency::Realtime);

    let subscription = sqlx::query_as::<_, ContractSubscription>(
        r#"
        INSERT INTO contract_subscriptions
            (user_id, contract_id, status, notification_types, channels, frequency, min_severity,
             created_at, updated_at)
        VALUES ($1, $2, 'active', $3, $4, $5, $6, NOW(), NOW())
        ON CONFLICT (user_id, contract_id) DO UPDATE SET
            status = 'active',
            notification_types = EXCLUDED.notification_types,
            channels = EXCLUDED.channels,
            frequency = EXCLUDED.frequency,
            min_severity = EXCLUDED.min_severity,
            updated_at = NOW()
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(contract_id)
    .bind(&notification_types)
    .bind(&channels)
    .bind(&frequency)
    .bind(&req.min_severity)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Failed to create subscription: {}", e)))?;

    Ok(Json(subscription))
}

/// DELETE /api/contracts/:id/subscribe
pub async fn unsubscribe_from_contract(
    State(state): State<AppState>,
    Path(contract_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let rows_affected = sqlx::query(
        "DELETE FROM contract_subscriptions WHERE user_id = $1 AND contract_id = $2",
    )
    .bind(auth_user.publisher_id)
    .bind(contract_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .rows_affected();

    if rows_affected == 0 {
        return Err(ApiError::not_found("subscription", "Subscription not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── List / Update subscriptions ──────────────────────────────────────────

/// GET /api/me/subscriptions
pub async fn list_user_subscriptions(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<ListSubscriptionsQuery>,
) -> ApiResult<Json<UserSubscriptionsResponse>> {
    let user_id = auth_user.publisher_id;
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    // Use separate queries to avoid dynamic parameter numbering bugs.
    let (subscriptions, total_count) = if let Some(ref status) = query.status {
        let rows = sqlx::query_as::<_, ContractSubscriptionSummary>(
            r#"
            SELECT cs.id, cs.contract_id, c.name AS contract_name,
                   c.slug AS contract_slug, cs.status, cs.notification_types,
                   cs.channels, cs.frequency, cs.created_at
            FROM contract_subscriptions cs
            JOIN contracts c ON cs.contract_id = c.id
            WHERE cs.user_id = $1 AND cs.status = $2
            ORDER BY cs.created_at DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(user_id)
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        let count: i64 =
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM contract_subscriptions WHERE user_id = $1 AND status = $2",
            )
            .bind(user_id)
            .bind(status)
            .fetch_one(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        (rows, count)
    } else {
        let rows = sqlx::query_as::<_, ContractSubscriptionSummary>(
            r#"
            SELECT cs.id, cs.contract_id, c.name AS contract_name,
                   c.slug AS contract_slug, cs.status, cs.notification_types,
                   cs.channels, cs.frequency, cs.created_at
            FROM contract_subscriptions cs
            JOIN contracts c ON cs.contract_id = c.id
            WHERE cs.user_id = $1
            ORDER BY cs.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM contract_subscriptions WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&state.db)
                .await
                .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        (rows, count)
    };

    Ok(Json(UserSubscriptionsResponse {
        subscriptions,
        total_count,
    }))
}

/// PATCH /api/subscriptions/:id
pub async fn update_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> ApiResult<Json<ContractSubscription>> {
    // Verify ownership first.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM contract_subscriptions WHERE id = $1 AND user_id = $2)",
    )
    .bind(subscription_id)
    .bind(auth_user.publisher_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !exists {
        return Err(ApiError::not_found("subscription", "Subscription not found"));
    }

    // Apply each optional field individually to avoid string-interpolated SQL injection.
    if let Some(ref status) = req.status {
        sqlx::query(
            "UPDATE contract_subscriptions SET status = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(status)
        .bind(subscription_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref types) = req.notification_types {
        sqlx::query(
            "UPDATE contract_subscriptions SET notification_types = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(types)
        .bind(subscription_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref channels) = req.channels {
        sqlx::query(
            "UPDATE contract_subscriptions SET channels = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(channels)
        .bind(subscription_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref frequency) = req.frequency {
        sqlx::query(
            "UPDATE contract_subscriptions SET frequency = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(frequency)
        .bind(subscription_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref severity) = req.min_severity {
        sqlx::query(
            "UPDATE contract_subscriptions SET min_severity = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(severity)
        .bind(subscription_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }

    let subscription = sqlx::query_as::<_, ContractSubscription>(
        "SELECT * FROM contract_subscriptions WHERE id = $1",
    )
    .bind(subscription_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(subscription))
}

// ─── Notification preferences ─────────────────────────────────────────────

/// GET /api/notifications/preferences
pub async fn get_notification_preferences(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<Json<UserNotificationPreferences>> {
    let prefs = sqlx::query_as::<_, UserNotificationPreferences>(
        "SELECT * FROM user_preferences WHERE publisher_id = $1",
    )
    .bind(auth_user.publisher_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .ok_or_else(|| ApiError::not_found("preferences", "User preferences not found"))?;

    Ok(Json(prefs))
}

/// PATCH /api/notifications/preferences
pub async fn update_notification_preferences(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<UpdateUserNotificationPreferencesRequest>,
) -> ApiResult<Json<UserNotificationPreferences>> {
    let user_id = auth_user.publisher_id;

    if let Some(ref freq) = req.notification_frequency {
        sqlx::query(
            "UPDATE user_preferences SET notification_frequency = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(freq)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref channels) = req.notification_channels {
        sqlx::query(
            "UPDATE user_preferences SET notification_channels = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(channels)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(enabled) = req.email_notifications_enabled {
        sqlx::query(
            "UPDATE user_preferences SET email_notifications_enabled = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(enabled)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref url) = req.webhook_url {
        sqlx::query(
            "UPDATE user_preferences SET webhook_url = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(url)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref start) = req.quiet_hours_start {
        sqlx::query(
            "UPDATE user_preferences SET quiet_hours_start = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(start)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref end) = req.quiet_hours_end {
        sqlx::query(
            "UPDATE user_preferences SET quiet_hours_end = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(end)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }
    if let Some(ref tz) = req.timezone {
        sqlx::query(
            "UPDATE user_preferences SET timezone = $1, updated_at = NOW() WHERE publisher_id = $2",
        )
        .bind(tz)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;
    }

    let prefs = sqlx::query_as::<_, UserNotificationPreferences>(
        "SELECT * FROM user_preferences WHERE publisher_id = $1",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(prefs))
}

// ─── Notifications ─────────────────────────────────────────────────────────

/// GET /api/notifications
pub async fn list_notifications(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<NotificationQuery>,
) -> ApiResult<Json<NotificationListResponse>> {
    let user_id = auth_user.publisher_id;
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);
    let unread_only = query.unread_only.unwrap_or(false);

    let (notifications, total_count) = if unread_only {
        let rows = sqlx::query_as::<_, NotificationQueueItem>(
            r#"
            SELECT nq.*
            FROM notification_queue nq
            JOIN contract_subscriptions cs ON nq.subscription_id = cs.id
            WHERE cs.user_id = $1 AND nq.status != 'read'
            ORDER BY nq.priority ASC, nq.scheduled_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM notification_queue nq
            JOIN contract_subscriptions cs ON nq.subscription_id = cs.id
            WHERE cs.user_id = $1 AND nq.status != 'read'
            "#,
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        (rows, count)
    } else {
        let rows = sqlx::query_as::<_, NotificationQueueItem>(
            r#"
            SELECT nq.*
            FROM notification_queue nq
            JOIN contract_subscriptions cs ON nq.subscription_id = cs.id
            WHERE cs.user_id = $1
            ORDER BY nq.priority ASC, nq.scheduled_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM notification_queue nq
            JOIN contract_subscriptions cs ON nq.subscription_id = cs.id
            WHERE cs.user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

        (rows, count)
    };

    Ok(Json(NotificationListResponse {
        notifications,
        total_count,
    }))
}

/// POST /api/notifications/:id/read
pub async fn mark_notification_read(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    sqlx::query(
        r#"
        UPDATE notification_queue nq
        SET status = 'read'
        WHERE nq.id = $1
          AND EXISTS (
              SELECT 1 FROM contract_subscriptions cs
              WHERE cs.id = nq.subscription_id AND cs.user_id = $2
          )
        "#,
    )
    .bind(notification_id)
    .bind(auth_user.publisher_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(StatusCode::OK)
}

/// POST /api/notifications/read-all
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    sqlx::query(
        r#"
        UPDATE notification_queue nq
        SET status = 'read'
        WHERE nq.status != 'read'
          AND EXISTS (
              SELECT 1 FROM contract_subscriptions cs
              WHERE cs.id = nq.subscription_id AND cs.user_id = $1
          )
        "#,
    )
    .bind(auth_user.publisher_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(StatusCode::OK)
}

/// GET /api/notifications/statistics
pub async fn get_notification_statistics(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<StatisticsQuery>,
) -> ApiResult<Json<shared::NotificationStatistics>> {
    let user_id = auth_user.publisher_id;

    let stats = sqlx::query_as::<_, shared::NotificationStatistics>(
        r#"
        SELECT id, user_id, contract_id, period_start, period_end,
               new_version_count, verification_status_count, security_issue_count,
               security_scan_completed_count, breaking_change_count, deprecation_count,
               maintenance_count, compatibility_issue_count,
               total_sent, total_delivered, total_failed
        FROM notification_statistics
        WHERE user_id = $1 AND period_start >= $2 AND period_end <= $3
        ORDER BY period_start DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(query.period_start)
    .bind(query.period_end)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .unwrap_or_else(|| shared::NotificationStatistics {
        id: Uuid::nil(),
        user_id: Some(user_id),
        contract_id: None,
        period_start: query.period_start,
        period_end: query.period_end,
        new_version_count: 0,
        verification_status_count: 0,
        security_issue_count: 0,
        security_scan_completed_count: 0,
        breaking_change_count: 0,
        deprecation_count: 0,
        maintenance_count: 0,
        compatibility_issue_count: 0,
        total_sent: 0,
        total_delivered: 0,
        total_failed: 0,
    });

    Ok(Json(stats))
}

// ─── Webhook CRUD ──────────────────────────────────────────────────────────

/// GET /api/webhooks
pub async fn list_webhooks(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<Json<Vec<WebhookConfiguration>>> {
    let webhooks = sqlx::query_as::<_, WebhookConfiguration>(
        "SELECT * FROM webhook_configurations WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(auth_user.publisher_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(webhooks))
}

/// POST /api/webhooks
pub async fn create_webhook(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<CreateWebhookRequest>,
) -> ApiResult<Json<WebhookConfiguration>> {
    // Validate URL scheme — only https allowed in production.
    if !req.url.starts_with("https://") && !req.url.starts_with("http://localhost") {
        return Err(ApiError::bad_request(
            "Webhook URL must use HTTPS (http://localhost is allowed for testing)",
        ));
    }

    let webhook = sqlx::query_as::<_, WebhookConfiguration>(
        r#"
        INSERT INTO webhook_configurations
            (user_id, name, url, notification_types, is_active, verify_ssl, custom_headers,
             created_at, updated_at)
        VALUES ($1, $2, $3, $4, true, $5, $6, NOW(), NOW())
        RETURNING *
        "#,
    )
    .bind(auth_user.publisher_id)
    .bind(&req.name)
    .bind(&req.url)
    .bind(&req.notification_types)
    .bind(req.verify_ssl.unwrap_or(true))
    .bind(&req.custom_headers)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Failed to create webhook: {}", e)))?;

    Ok(Json(webhook))
}

/// DELETE /api/webhooks/:id
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let rows_affected = sqlx::query(
        "DELETE FROM webhook_configurations WHERE id = $1 AND user_id = $2",
    )
    .bind(webhook_id)
    .bind(auth_user.publisher_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .rows_affected();

    if rows_affected == 0 {
        return Err(ApiError::not_found("webhook", "Webhook not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Webhook delivery management ──────────────────────────────────────────

/// GET /api/webhooks/:id/deliveries
pub async fn get_webhook_deliveries(
    State(state): State<AppState>,
    Path(webhook_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<DeliveriesQuery>,
) -> ApiResult<Json<WebhookDeliveriesResponse>> {
    // Verify ownership.
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM webhook_configurations WHERE id = $1 AND user_id = $2)",
    )
    .bind(webhook_id)
    .bind(auth_user.publisher_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !owned {
        return Err(ApiError::not_found("webhook", "Webhook not found"));
    }

    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let deliveries = sqlx::query_as::<_, WebhookDeliveryLog>(
        r#"
        SELECT id, webhook_id, notification_id, event_type, status, response_code,
               response_body, error_message, attempt_number, delivery_duration_ms, created_at
        FROM notification_delivery_logs
        WHERE webhook_id = $1
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(webhook_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    let total_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notification_delivery_logs WHERE webhook_id = $1")
            .bind(webhook_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(WebhookDeliveriesResponse {
        deliveries,
        total_count,
    }))
}

/// POST /api/webhooks/:id/test  — sends a synthetic test event to the webhook.
pub async fn test_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let webhook = sqlx::query_as::<_, WebhookConfiguration>(
        "SELECT * FROM webhook_configurations WHERE id = $1 AND user_id = $2",
    )
    .bind(webhook_id)
    .bind(auth_user.publisher_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .ok_or_else(|| ApiError::not_found("webhook", "Webhook not found"))?;

    // Queue a synthetic test notification for async delivery.
    sqlx::query(
        r#"
        INSERT INTO notification_delivery_logs
            (webhook_id, event_type, status, attempt_number, created_at)
        VALUES ($1, 'test', 'pending', 0, NOW())
        "#,
    )
    .bind(webhook.id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Failed to queue test delivery: {}", e)))?;

    Ok(StatusCode::ACCEPTED)
}

/// POST /api/webhook-deliveries/:id/retry  — re-queues a failed delivery.
pub async fn retry_webhook_delivery(
    State(state): State<AppState>,
    Path(delivery_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    // Verify the delivery belongs to a webhook owned by this user.
    let owned: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM notification_delivery_logs ndl
            JOIN webhook_configurations wc ON wc.id = ndl.webhook_id
            WHERE ndl.id = $1 AND wc.user_id = $2
        )
        "#,
    )
    .bind(delivery_id)
    .bind(auth_user.publisher_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !owned {
        return Err(ApiError::not_found("delivery", "Delivery log not found"));
    }

    // Reset the delivery so the background worker picks it up again.
    let rows = sqlx::query(
        r#"
        UPDATE notification_delivery_logs
        SET status = 'pending', attempt_number = 0, error_message = NULL, updated_at = NOW()
        WHERE id = $1 AND status = 'failed'
        "#,
    )
    .bind(delivery_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .rows_affected();

    if rows == 0 {
        return Err(ApiError::bad_request(
            "Delivery is not in a failed state and cannot be retried",
        ));
    }

    Ok(StatusCode::ACCEPTED)
}
