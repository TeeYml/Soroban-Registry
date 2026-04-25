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
    ContractSubscription, ContractSubscriptionSummary, CreateWebhookRequest, NotificationChannel,
    NotificationFrequency, NotificationType, SubscribeRequest, SubscriptionStatus,
    UpdateSubscriptionRequest, UpdateUserNotificationPreferencesRequest,
    UserNotificationPreferences, UserSubscriptionsResponse, WebhookConfiguration,
};

/// Query parameters for listing subscriptions
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct ListSubscriptionsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
}

/// Subscribe to a contract
///
/// POST /api/contracts/:id/subscribe
pub async fn subscribe_to_contract(
    State(state): State<AppState>,
    Path(contract_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<SubscribeRequest>,
) -> ApiResult<Json<ContractSubscription>> {
    // Verify contract exists
    let contract_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contracts WHERE id = $1)")
            .bind(contract_id)
            .fetch_one(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    if !contract_exists {
        return Err(ApiError::not_found("contract", "Contract not found"));
    }

    // Get user's publisher_id
    let user_id = auth_user.publisher_id;

    // Default values
    let notification_types = req.notification_types.unwrap_or(vec![
        NotificationType::NewVersion,
        NotificationType::VerificationStatus,
        NotificationType::SecurityIssue,
    ]);

    let channels = req.channels.unwrap_or(vec![NotificationChannel::InApp]);

    let frequency = req.frequency.unwrap_or(NotificationFrequency::Realtime);

    // Create or update subscription
    let subscription = sqlx::query_as::<_, ContractSubscription>(
        r#"
        INSERT INTO contract_subscriptions
        (user_id, contract_id, status, notification_types, channels, frequency, min_severity, created_at, updated_at)
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

/// Unsubscribe from a contract
///
/// DELETE /api/contracts/:id/subscribe
pub async fn unsubscribe_from_contract(
    State(state): State<AppState>,
    Path(contract_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let user_id = auth_user.publisher_id;

    let rows_affected =
        sqlx::query("DELETE FROM contract_subscriptions WHERE user_id = $1 AND contract_id = $2")
            .bind(user_id)
            .bind(contract_id)
            .execute(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
            .rows_affected();

    if rows_affected == 0 {
        return Err(ApiError::not_found(
            "subscription",
            "Subscription not found",
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// List user's subscriptions
///
/// GET /api/me/subscriptions
pub async fn list_user_subscriptions(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<ListSubscriptionsQuery>,
) -> ApiResult<Json<UserSubscriptionsResponse>> {
    let user_id = auth_user.publisher_id;
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let mut where_clause = "WHERE cs.user_id = $1".to_string();
    if let Some(status) = &query.status {
        where_clause.push_str(&format!(" AND cs.status = ${}", 2));
    }

    let subscriptions = sqlx::query_as::<_, ContractSubscriptionSummary>(&format!(
        r#"
        SELECT 
            cs.id,
            cs.contract_id,
            c.name as contract_name,
            c.slug as contract_slug,
            cs.status,
            cs.notification_types,
            cs.channels,
            cs.frequency,
            cs.created_at
        FROM contract_subscriptions cs
        JOIN contracts c ON cs.contract_id = c.id
        {}
        ORDER BY cs.created_at DESC
        LIMIT ${} OFFSET ${}
        "#,
        where_clause,
        if query.status.is_some() { 3 } else { 2 },
        if query.status.is_some() { 4 } else { 3 }
    ))
    .bind(user_id)
    .bind(&query.status.unwrap_or_default())
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    let total_count: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM contract_subscriptions {}",
        if query.status.is_some() {
            "WHERE user_id = $1 AND status = $2"
        } else {
            "WHERE user_id = $1"
        }
    ))
    .bind(user_id)
    .bind(&query.status.unwrap_or_default())
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(UserSubscriptionsResponse {
        subscriptions,
        total_count,
    }))
}

/// Update subscription preferences
///
/// PATCH /api/subscriptions/:id
pub async fn update_subscription(
    State(state): State<AppState>,
    Path(subscription_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<UpdateSubscriptionRequest>,
) -> ApiResult<Json<ContractSubscription>> {
    let user_id = auth_user.publisher_id;

    // Build dynamic update query
    let mut updates = Vec::new();
    if let Some(status) = &req.status {
        updates.push(format!("status = '{}'", status));
    }
    if let Some(types) = &req.notification_types {
        updates.push(format!("notification_types = {:?}", types));
    }
    if let Some(channels) = &req.channels {
        updates.push(format!("channels = {:?}", channels));
    }
    if let Some(frequency) = &req.frequency {
        updates.push(format!("frequency = '{}'", frequency));
    }
    if let Some(min_severity) = &req.min_severity {
        updates.push(format!("min_severity = '{}'", min_severity));
    }

    updates.push("updated_at = NOW()".to_string());

    if updates.is_empty() {
        return Err(ApiError::bad_request("No fields to update"));
    }

    let update_clause = updates.join(", ");

    let subscription = sqlx::query_as::<_, ContractSubscription>(&format!(
        r#"
        UPDATE contract_subscriptions
        SET {}
        WHERE id = $1 AND user_id = $2
        RETURNING *
        "#,
        update_clause
    ))
    .bind(subscription_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .ok_or_else(|| ApiError::not_found("subscription", "Subscription not found"))?;

    Ok(Json(subscription))
}

/// Get user notification preferences
///
/// GET /api/notifications/preferences
pub async fn get_notification_preferences(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<Json<UserNotificationPreferences>> {
    let user_id = auth_user.publisher_id;

    let prefs = sqlx::query_as::<_, UserNotificationPreferences>(
        "SELECT * FROM user_preferences WHERE publisher_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .ok_or_else(|| ApiError::not_found("preferences", "User preferences not found"))?;

    Ok(Json(UserNotificationPreferences {
        id: prefs.id,
        publisher_id: prefs.publisher_id,
        notification_frequency: prefs.notification_frequency,
        notification_channels: prefs.notification_channels,
        email_notifications_enabled: prefs.email_notifications_enabled,
        webhook_url: prefs.webhook_url,
        quiet_hours_start: prefs.quiet_hours_start,
        quiet_hours_end: prefs.quiet_hours_end,
        timezone: prefs.timezone,
        created_at: prefs.created_at,
        updated_at: prefs.updated_at,
    }))
}

/// Update user notification preferences
///
/// PATCH /api/notifications/preferences
pub async fn update_notification_preferences(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<UpdateUserNotificationPreferencesRequest>,
) -> ApiResult<Json<UserNotificationPreferences>> {
    let user_id = auth_user.publisher_id;

    // Build dynamic update
    let mut updates = Vec::new();
    let mut param_count = 1;

    if let Some(freq) = &req.notification_frequency {
        param_count += 1;
        updates.push(format!("notification_frequency = ${}", param_count));
    }
    if let Some(channels) = &req.notification_channels {
        param_count += 1;
        updates.push(format!("notification_channels = ${}", param_count));
    }
    if let Some(enabled) = &req.email_notifications_enabled {
        param_count += 1;
        updates.push(format!("email_notifications_enabled = ${}", param_count));
    }
    if let Some(webhook) = &req.webhook_url {
        param_count += 1;
        updates.push(format!("webhook_url = ${}", param_count));
    }
    if let Some(start) = &req.quiet_hours_start {
        param_count += 1;
        updates.push(format!("quiet_hours_start = ${}", param_count));
    }
    if let Some(end) = &req.quiet_hours_end {
        param_count += 1;
        updates.push(format!("quiet_hours_end = ${}", param_count));
    }
    if let Some(tz) = &req.timezone {
        param_count += 1;
        updates.push(format!("timezone = ${}", param_count));
    }

    updates.push(format!("updated_at = NOW()"));

    if updates.is_empty() {
        return Err(ApiError::bad_request("No fields to update"));
    }

    let update_clause = updates.join(", ");

    // This is a simplified version - in production you'd use proper parameterized queries
    let prefs = sqlx::query_as::<_, UserNotificationPreferences>(&format!(
        r#"
        UPDATE user_preferences
        SET {}
        WHERE publisher_id = $1
        RETURNING *
        "#,
        update_clause
    ))
    .bind(user_id)
    .bind(&req.notification_frequency)
    .bind(&req.notification_channels)
    .bind(&req.email_notifications_enabled)
    .bind(&req.webhook_url)
    .bind(&req.quiet_hours_start)
    .bind(&req.quiet_hours_end)
    .bind(&req.timezone)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(prefs))
}

/// List user's webhook configurations
///
/// GET /api/webhooks
pub async fn list_webhooks(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<Json<Vec<WebhookConfiguration>>> {
    let user_id = auth_user.publisher_id;

    let webhooks = sqlx::query_as::<_, WebhookConfiguration>(
        r#"
        SELECT * FROM webhook_configurations
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(webhooks))
}

/// Create a new webhook
///
/// POST /api/webhooks
pub async fn create_webhook(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Json(req): Json<CreateWebhookRequest>,
) -> ApiResult<Json<WebhookConfiguration>> {
    let user_id = auth_user.publisher_id;

    // In production, secret should be encrypted
    let webhook = sqlx::query_as::<_, WebhookConfiguration>(
        r#"
        INSERT INTO webhook_configurations
        (user_id, name, url, notification_types, is_active, verify_ssl, custom_headers, created_at, updated_at)
        VALUES ($1, $2, $3, $4, true, $5, $6, NOW(), NOW())
        RETURNING *
        "#,
    )
    .bind(user_id)
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

/// Delete a webhook
///
/// DELETE /api/webhooks/:id
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(webhook_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let user_id = auth_user.publisher_id;

    let rows_affected =
        sqlx::query("DELETE FROM webhook_configurations WHERE id = $1 AND user_id = $2")
            .bind(webhook_id)
            .bind(user_id)
            .execute(&state.db)
            .await
            .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
            .rows_affected();

    if rows_affected == 0 {
        return Err(ApiError::not_found("webhook", "Webhook not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// Get user's notifications (from notification queue)
///
/// GET /api/notifications
pub async fn list_notifications(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<NotificationQuery>,
) -> ApiResult<Json<NotificationListResponse>> {
    let user_id = auth_user.publisher_id;
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    // Get notifications from queue for this user's subscriptions
    let notifications = sqlx::query_as::<_, NotificationQueueItem>(
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

    let total_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM notification_queue nq
        JOIN contract_subscriptions cs ON nq.subscription_id = cs.id
        WHERE cs.user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(Json(NotificationListResponse {
        notifications,
        total_count,
    }))
}

/// Query parameters for notifications
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct NotificationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub unread_only: Option<bool>,
}

/// Notification list response
#[derive(Debug, serde::Serialize)]
pub struct NotificationListResponse {
    pub notifications: Vec<NotificationQueueItem>,
    pub total_count: i64,
}

/// Mark notification as read
///
/// POST /api/notifications/:id/read
pub async fn mark_notification_read(
    State(state): State<AppState>,
    Path(notification_id): Path<Uuid>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let user_id = auth_user.publisher_id;

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
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(StatusCode::OK)
}

/// Mark all notifications as read
///
/// POST /api/notifications/read-all
pub async fn mark_all_notifications_read(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
) -> ApiResult<StatusCode> {
    let user_id = auth_user.publisher_id;

    sqlx::query(
        r#"
        UPDATE notification_queue nq
        SET status = 'read'
        WHERE EXISTS (
            SELECT 1 FROM contract_subscriptions cs
            WHERE cs.id = nq.subscription_id AND cs.user_id = $1
        )
        AND nq.status != 'read'
        "#,
    )
    .bind(user_id)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?;

    Ok(StatusCode::OK)
}

/// Get notification statistics
///
/// GET /api/notifications/statistics
pub async fn get_notification_statistics(
    State(state): State<AppState>,
    auth_user: auth::AuthenticatedUser,
    Query(query): Query<StatisticsQuery>,
) -> ApiResult<Json<shared::NotificationStatistics>> {
    let user_id = auth_user.publisher_id;
    let period_start = query.period_start;
    let period_end = query.period_end;

    // In production, you'd aggregate from notification_delivery_logs
    // This is a simplified version
    let stats = sqlx::query_as::<_, shared::NotificationStatistics>(
        r#"
        SELECT 
            id, user_id, contract_id, period_start, period_end,
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
    .bind(period_start)
    .bind(period_end)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError::internal(format!("Database error: {}", e)))?
    .unwrap_or_else(|| shared::NotificationStatistics {
        id: Uuid::nil(),
        user_id: Some(user_id),
        contract_id: None,
        period_start,
        period_end,
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

/// Query parameters for statistics
#[derive(Debug, serde::Deserialize, utoipa::IntoParams)]
pub struct StatisticsQuery {
    pub period_start: chrono::NaiveDate,
    pub period_end: chrono::NaiveDate,
}
