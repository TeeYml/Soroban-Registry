//! Contract category management handlers (issue #414)
//!
//! Implements CRUD endpoints for the `contract_categories` table so that
//! administrators can manage the category taxonomy without a code deployment:
//!
//!   GET    /api/categories                    – list all categories (public)
//!   POST   /api/admin/categories              – create a new category
//!   PUT    /api/admin/categories/:id          – update a category
//!   DELETE /api/admin/categories/:id          – delete a category
//!
//! Deletion is guarded by a usage check: if any contracts currently reference
//! the category by name the request is rejected with 409 Conflict unless the
//! `force=true` query parameter is supplied, in which case those contracts have
//! their category field cleared before the category row is removed.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryRequest {
    /// Display name for the category (max 100 characters, must be unique).
    pub name: String,
    /// Optional human-readable description shown on the frontend.
    pub description: Option<String>,
    /// UUID of the parent category.  Supply to nest this category under another.
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryRequest {
    /// New display name.  Omit to leave unchanged.
    pub name: Option<String>,
    /// New description.  Send an empty string to clear it.
    pub description: Option<String>,
    /// New parent UUID.  Send `null` explicitly to make the category top-level.
    #[serde(default, deserialize_with = "deserialize_optional_uuid_string")]
    pub parent_id: Option<Option<String>>,
}

/// Query parameters for DELETE /api/admin/categories/:id
#[derive(Debug, Deserialize)]
pub struct DeleteCategoryQuery {
    /// When `true`, contracts that reference this category have their category
    /// field cleared before the category is deleted.  Defaults to `false`.
    #[serde(default)]
    pub force: bool,
}

/// Raw row returned by the list query (includes computed usage_count).
#[derive(Debug, FromRow)]
struct CategoryRow {
    id: Uuid,
    name: String,
    slug: String,
    description: Option<String>,
    parent_id: Option<Uuid>,
    is_default: bool,
    usage_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Public-facing category representation returned by all endpoints.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CategoryResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// UUID of the parent category, or `null` for top-level categories.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Whether this category was seeded at database creation time and should
    /// not be permanently removed.
    pub is_default: bool,
    /// Number of contracts currently assigned to this category.
    pub usage_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<CategoryRow> for CategoryResponse {
    fn from(row: CategoryRow) -> Self {
        Self {
            id: row.id.to_string(),
            name: row.name,
            slug: row.slug,
            description: row.description,
            parent_id: row.parent_id.map(|id| id.to_string()),
            is_default: row.is_default,
            usage_count: row.usage_count,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

// ── Slug helper ───────────────────────────────────────────────────────────────

/// Converts a category name into a URL-safe slug.
///
/// ```text
/// "DeFi Lending" → "defi-lending"
/// "DEX"          → "dex"
/// ```
fn to_slug(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

// ── Serde helper for nullable optional UUID ───────────────────────────────────

/// Deserialises `Option<Option<String>>` so the caller can distinguish between
/// a field that was omitted (outer `None`) and one explicitly set to `null`
/// (inner `None`).
fn deserialize_optional_uuid_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

// ── Internal helpers ──────────────────────────────────────────────────────────

const LIST_QUERY: &str = r#"
    SELECT
        cc.id,
        cc.name,
        cc.slug,
        cc.description,
        cc.parent_id,
        cc.is_default,
        cc.created_at,
        cc.updated_at,
        COUNT(c.id) AS usage_count
    FROM contract_categories cc
    LEFT JOIN contracts c ON c.category = cc.name
    GROUP BY cc.id
    ORDER BY cc.is_default DESC, cc.name ASC
"#;

fn db_err(op: &str, err: sqlx::Error) -> ApiError {
    tracing::error!(operation = op, error = ?err, "database operation failed");
    ApiError::internal("An unexpected database error occurred")
}

fn parse_category_id(id: &str) -> ApiResult<Uuid> {
    Uuid::parse_str(id).map_err(|_| {
        ApiError::bad_request(
            "InvalidCategoryId",
            format!("Invalid category ID format: {}", id),
        )
    })
}

fn parse_optional_parent_id(raw: &Option<String>) -> ApiResult<Option<Uuid>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => Uuid::parse_str(s).map(Some).map_err(|_| {
            ApiError::bad_request(
                "InvalidParentId",
                format!("Invalid parent category ID format: {}", s),
            )
        }),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// List all categories with their current usage counts.
///
/// Results are ordered so that default (seeded) categories appear first,
/// followed by custom categories alphabetically.
#[utoipa::path(
    get,
    path = "/api/categories",
    responses(
        (status = 200, description = "List of contract categories with usage counts", body = [CategoryResponse])
    ),
    tag = "Categories"
)]
pub async fn list_categories(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<CategoryResponse>>> {
    let rows: Vec<CategoryRow> = sqlx::query_as(LIST_QUERY)
        .fetch_all(&state.db)
        .await
        .map_err(|err| db_err("list categories", err))?;

    Ok(Json(rows.into_iter().map(CategoryResponse::from).collect()))
}

/// Create a new contract category.
///
/// The slug is derived automatically from the name.  Returns 409 Conflict if a
/// category with the same name or slug already exists.
#[utoipa::path(
    post,
    path = "/api/admin/categories",
    request_body = CreateCategoryRequest,
    responses(
        (status = 201, description = "Category created successfully", body = CategoryResponse),
        (status = 400, description = "Invalid input"),
        (status = 409, description = "A category with this name already exists")
    ),
    tag = "Categories",
    security(("bearer_auth" = []))
)]
pub async fn create_category(
    State(state): State<AppState>,
    Json(req): Json<CreateCategoryRequest>,
) -> ApiResult<(StatusCode, Json<CategoryResponse>)> {
    let name = req.name.trim().to_string();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::bad_request(
            "InvalidName",
            "name must be between 1 and 100 characters",
        ));
    }

    let slug = to_slug(&name);
    if slug.is_empty() {
        return Err(ApiError::bad_request(
            "InvalidName",
            "name must contain at least one alphanumeric character",
        ));
    }

    let parent_uuid = parse_optional_parent_id(&req.parent_id)?;

    // If a parent was supplied, verify it exists to give a clear error message
    // rather than a cryptic FK violation.
    if let Some(pid) = parent_uuid {
        let parent_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contract_categories WHERE id = $1)")
                .bind(pid)
                .fetch_one(&state.db)
                .await
                .map_err(|err| db_err("verify parent category", err))?;

        if !parent_exists {
            return Err(ApiError::not_found(
                "ParentCategoryNotFound",
                format!("No parent category found with ID: {}", pid),
            ));
        }
    }

    let row: CategoryRow = sqlx::query_as(
        r#"
        WITH inserted AS (
            INSERT INTO contract_categories (name, slug, description, parent_id)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        )
        SELECT i.*, 0::BIGINT AS usage_count
        FROM inserted i
        "#,
    )
    .bind(&name)
    .bind(&slug)
    .bind(req.description.as_deref())
    .bind(parent_uuid)
    .fetch_one(&state.db)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => ApiError::conflict(
            "CategoryAlreadyExists",
            format!("A category named '{}' already exists", name),
        ),
        _ => db_err("create category", err),
    })?;

    Ok((StatusCode::CREATED, Json(CategoryResponse::from(row))))
}

/// Update a category's name, description, or parent.
///
/// Only fields that are explicitly included in the JSON body are changed.
/// Changing the name automatically regenerates the slug.
#[utoipa::path(
    put,
    path = "/api/admin/categories/{id}",
    params(
        ("id" = String, Path, description = "Category UUID")
    ),
    request_body = UpdateCategoryRequest,
    responses(
        (status = 200, description = "Category updated successfully", body = CategoryResponse),
        (status = 400, description = "Invalid input"),
        (status = 404, description = "Category not found"),
        (status = 409, description = "A category with the new name already exists")
    ),
    tag = "Categories",
    security(("bearer_auth" = []))
)]
pub async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateCategoryRequest>,
) -> ApiResult<Json<CategoryResponse>> {
    let category_uuid = parse_category_id(&id)?;

    // Confirm the category exists before attempting the update.
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contract_categories WHERE id = $1)")
            .bind(category_uuid)
            .fetch_one(&state.db)
            .await
            .map_err(|err| db_err("check category existence", err))?;

    if !exists {
        return Err(ApiError::not_found(
            "CategoryNotFound",
            format!("No category found with ID: {}", id),
        ));
    }

    // Resolve the new parent UUID if one was supplied.
    let new_parent: Option<Option<Uuid>> = match &req.parent_id {
        None => None, // field was omitted → do not change parent
        Some(inner) => {
            let uuid = parse_optional_parent_id(inner)?;
            // Guard against self-referencing loops.
            if let Some(pid) = uuid {
                if pid == category_uuid {
                    return Err(ApiError::bad_request(
                        "CircularParent",
                        "A category cannot be its own parent",
                    ));
                }
                let parent_exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM contract_categories WHERE id = $1)",
                )
                .bind(pid)
                .fetch_one(&state.db)
                .await
                .map_err(|err| db_err("verify parent on update", err))?;

                if !parent_exists {
                    return Err(ApiError::not_found(
                        "ParentCategoryNotFound",
                        format!("No parent category found with ID: {}", pid),
                    ));
                }
            }
            Some(uuid)
        }
    };

    // Build the UPDATE statement dynamically so we only touch supplied fields.
    let updated: CategoryRow = {
        let name_val = req.name.as_deref().map(str::trim);
        if let Some(name) = name_val {
            if name.is_empty() || name.len() > 100 {
                return Err(ApiError::bad_request(
                    "InvalidName",
                    "name must be between 1 and 100 characters",
                ));
            }
        }

        let new_slug = name_val.map(to_slug);

        match new_parent {
            // parent unchanged
            None => sqlx::query_as(
                r#"
                WITH updated AS (
                    UPDATE contract_categories
                    SET
                        name        = COALESCE($2, name),
                        slug        = COALESCE($3, slug),
                        description = CASE WHEN $4::BOOLEAN THEN $5 ELSE description END,
                        updated_at  = NOW()
                    WHERE id = $1
                    RETURNING *
                )
                SELECT u.*, (
                    SELECT COUNT(*) FROM contracts c WHERE c.category = u.name
                )::BIGINT AS usage_count
                FROM updated u
                "#,
            )
            .bind(category_uuid)
            .bind(name_val)
            .bind(new_slug.as_deref())
            .bind(req.description.is_some())
            .bind(req.description.as_deref())
            .fetch_one(&state.db)
            .await
            .map_err(|err| match err {
                sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                    ApiError::conflict(
                        "CategoryAlreadyExists",
                        "A category with this name already exists",
                    )
                }
                _ => db_err("update category", err),
            })?,

            // parent explicitly changed (possibly to NULL)
            Some(parent_uuid) => sqlx::query_as(
                r#"
                WITH updated AS (
                    UPDATE contract_categories
                    SET
                        name        = COALESCE($2, name),
                        slug        = COALESCE($3, slug),
                        description = CASE WHEN $4::BOOLEAN THEN $5 ELSE description END,
                        parent_id   = $6,
                        updated_at  = NOW()
                    WHERE id = $1
                    RETURNING *
                )
                SELECT u.*, (
                    SELECT COUNT(*) FROM contracts c WHERE c.category = u.name
                )::BIGINT AS usage_count
                FROM updated u
                "#,
            )
            .bind(category_uuid)
            .bind(name_val)
            .bind(new_slug.as_deref())
            .bind(req.description.is_some())
            .bind(req.description.as_deref())
            .bind(parent_uuid)
            .fetch_one(&state.db)
            .await
            .map_err(|err| match err {
                sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => {
                    ApiError::conflict(
                        "CategoryAlreadyExists",
                        "A category with this name already exists",
                    )
                }
                _ => db_err("update category with parent", err),
            })?,
        }
    };

    Ok(Json(CategoryResponse::from(updated)))
}

/// Delete a category.
///
/// Default categories (`is_default = true`) cannot be deleted.
///
/// If any contracts are currently assigned to this category:
///   - Without `?force=true` the request fails with **409 Conflict** and
///     includes the current usage count in the response body.
///   - With `?force=true` those contracts have their `category` field cleared
///     (`NULL`) before the category row is removed.
#[utoipa::path(
    delete,
    path = "/api/admin/categories/{id}",
    params(
        ("id" = String, Path, description = "Category UUID"),
        ("force" = bool, Query, description = "Clear category from contracts before deleting")
    ),
    responses(
        (status = 204, description = "Category deleted"),
        (status = 400, description = "Invalid category ID"),
        (status = 403, description = "Cannot delete a default category"),
        (status = 404, description = "Category not found"),
        (status = 409, description = "Category is in use; supply ?force=true to override")
    ),
    tag = "Categories",
    security(("bearer_auth" = []))
)]
pub async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteCategoryQuery>,
) -> ApiResult<StatusCode> {
    let category_uuid = parse_category_id(&id)?;

    // Fetch the category so we can check is_default and name in one query.
    let row = sqlx::query_as::<_, (String, bool)>(
        "SELECT name, is_default FROM contract_categories WHERE id = $1",
    )
    .bind(category_uuid)
    .fetch_optional(&state.db)
    .await
    .map_err(|err| db_err("fetch category for delete", err))?;

    let (name, is_default) = row.ok_or_else(|| {
        ApiError::not_found(
            "CategoryNotFound",
            format!("No category found with ID: {}", id),
        )
    })?;

    if is_default {
        return Err(ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "DefaultCategory",
            format!("'{}' is a default category and cannot be deleted", name),
        ));
    }

    // Count contracts currently using this category.
    let usage_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contracts WHERE category = $1")
        .bind(&name)
        .fetch_one(&state.db)
        .await
        .map_err(|err| db_err("count category usage", err))?;

    if usage_count > 0 && !query.force {
        return Err(ApiError::conflict(
            "CategoryInUse",
            format!(
                "'{}' is assigned to {} contract(s). \
                 Pass ?force=true to clear those assignments and delete the category.",
                name, usage_count
            ),
        ));
    }

    // With force=true: clear the category field on affected contracts first.
    if usage_count > 0 && query.force {
        sqlx::query("UPDATE contracts SET category = NULL WHERE category = $1")
            .bind(&name)
            .execute(&state.db)
            .await
            .map_err(|err| db_err("clear contracts category on force delete", err))?;
    }

    sqlx::query("DELETE FROM contract_categories WHERE id = $1")
        .bind(category_uuid)
        .execute(&state.db)
        .await
        .map_err(|err| db_err("delete category", err))?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_slug_lowercase_alphanumeric() {
        assert_eq!(to_slug("DEX"), "dex");
        assert_eq!(to_slug("Lending"), "lending");
    }

    #[test]
    fn to_slug_replaces_spaces_with_hyphens() {
        assert_eq!(to_slug("DeFi Lending"), "defi-lending");
    }

    #[test]
    fn to_slug_collapses_consecutive_separators() {
        assert_eq!(to_slug("NFT -- Marketplace"), "nft-marketplace");
    }

    #[test]
    fn to_slug_trims_leading_trailing_whitespace() {
        assert_eq!(to_slug("  Bridge  "), "bridge");
    }

    #[test]
    fn parse_category_id_rejects_non_uuid() {
        assert!(parse_category_id("not-a-uuid").is_err());
    }

    #[test]
    fn parse_category_id_accepts_valid_uuid() {
        let id = Uuid::new_v4().to_string();
        assert!(parse_category_id(&id).is_ok());
    }

    #[test]
    fn parse_optional_parent_id_returns_none_for_empty_string() {
        assert!(matches!(
            parse_optional_parent_id(&Some(String::new())),
            Ok(None)
        ));
    }

    #[test]
    fn parse_optional_parent_id_returns_none_when_absent() {
        assert!(matches!(parse_optional_parent_id(&None), Ok(None)));
    }

    #[test]
    fn parse_optional_parent_id_returns_uuid_for_valid_input() {
        let id = Uuid::new_v4();
        let result = parse_optional_parent_id(&Some(id.to_string()));
        assert_eq!(result.unwrap(), Some(id));
    }
}
