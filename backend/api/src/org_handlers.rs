use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use shared::{
    AuthClaims, CreateOrganizationRequest, InviteMemberRequest, Organization,
    OrganizationMember, OrganizationRole, UpdateOrganizationRequest,
};
use uuid::Uuid;
use crate::{error::ApiResult, state::AppState, handlers::db_internal_error};

/// Create a new organization
pub async fn create_organization(
    State(state): State<AppState>,
    claims: AuthClaims,
    Json(payload): Json<CreateOrganizationRequest>,
) -> ApiResult<(StatusCode, Json<Organization>)> {
    let mut tx = state.pool.begin().await
        .map_err(|e| db_internal_error("begin_transaction", e))?;

    // 1. Get publisher ID from Stellar address (claims.sub)
    let publisher_id: Uuid = sqlx::query_scalar!(
        "SELECT id FROM publishers WHERE stellar_address = $1",
        claims.sub
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| {
        if matches!(e, sqlx::Error::RowNotFound) {
            StatusCode::UNAUTHORIZED.into()
        } else {
            db_internal_error("get_publisher_id", e)
        }
    })?;

    // 2. Create organization
    let org = sqlx::query_as!(
        Organization,
        r#"
        INSERT INTO organizations (name, slug, description, is_private)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        "#,
        payload.name,
        payload.slug,
        payload.description,
        payload.is_private.unwrap_or(true)
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_internal_error("create_organization", e))?;

    // 3. Add creator as Admin
    sqlx::query!(
        r#"
        INSERT INTO organization_members (organization_id, publisher_id, role)
        VALUES ($1, $2, $3)
        "#,
        org.id,
        publisher_id,
        OrganizationRole::Admin as OrganizationRole
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| db_internal_error("add_org_admin", e))?;

    tx.commit().await.map_err(|e| db_internal_error("commit_transaction", e))?;

    Ok((StatusCode::CREATED, Json(org)))
}

/// Get organization by slug or ID
pub async fn get_organization(
    State(state): State<AppState>,
    claims: Option<AuthClaims>,
    Path(slug_or_id): Path<String>,
) -> ApiResult<Json<Organization>> {
    let org_id = Uuid::parse_str(&slug_or_id).ok();
    
    let org = if let Some(id) = org_id {
        sqlx::query_as!(Organization, "SELECT * FROM organizations WHERE id = $1", id)
            .fetch_optional(&state.pool)
            .await
    } else {
        sqlx::query_as!(Organization, "SELECT * FROM organizations WHERE slug = $1", slug_or_id)
            .fetch_optional(&state.pool)
            .await
    }
    .map_err(|e| db_internal_error("get_organization", e))?
    .ok_or(StatusCode::NOT_FOUND)?;

    // Visibility check for private organizations
    if org.is_private {
        let is_member = if let Some(ref claims) = claims {
            check_org_role(&state.pool, org.id, &claims.sub, OrganizationRole::Viewer)
                .await
                .is_ok()
        } else {
            false
        };

        if !is_member {
            return Err(StatusCode::FORBIDDEN.into());
        }
    }

    Ok(Json(org))
}

/// Update organization metadata
pub async fn update_organization(
    State(state): State<AppState>,
    claims: AuthClaims,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateOrganizationRequest>,
) -> ApiResult<Json<Organization>> {
    // Check if user is Admin of the org
    check_org_role(&state.pool, id, &claims.sub, OrganizationRole::Admin).await?;

    let org = sqlx::query_as!(
        Organization,
        r#"
        UPDATE organizations
        SET 
            name = COALESCE($1, name),
            description = COALESCE($2, description),
            is_private = COALESCE($3, is_private),
            updated_at = NOW()
        WHERE id = $4
        RETURNING *
        "#,
        payload.name,
        payload.description,
        payload.is_private,
        id
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_internal_error("update_organization", e))?;

    Ok(Json(org))
}

/// Helper to check if a user has a specific role (or higher) in an organization
pub async fn check_org_role(
    pool: &sqlx::PgPool,
    org_id: Uuid,
    user_address: &str,
    min_role: OrganizationRole,
) -> ApiResult<OrganizationRole> {
    let member = sqlx::query!(
        r#"
        SELECT role as "role: OrganizationRole"
        FROM organization_members om
        JOIN publishers p ON om.publisher_id = p.id
        WHERE om.organization_id = $1 AND p.stellar_address = $2
        "#,
        org_id,
        user_address
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| db_internal_error("check_org_role", e))?
    .ok_or(StatusCode::FORBIDDEN)?;

    let has_access = match (min_role, member.role) {
        (OrganizationRole::Admin, OrganizationRole::Admin) => true,
        (OrganizationRole::Member, OrganizationRole::Admin | OrganizationRole::Member) => true,
        (OrganizationRole::Viewer, _) => true,
        _ => false,
    };

    if has_access {
        Ok(member.role)
    } else {
        Err(StatusCode::FORBIDDEN.into())
    }
}

/// List all members of an organization
pub async fn list_org_members(
    State(state): State<AppState>,
    claims: AuthClaims,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<OrganizationMember>>> {
    // Check if user is a member of the org
    check_org_role(&state.pool, id, &claims.sub, OrganizationRole::Viewer).await?;

    let members = sqlx::query_as!(
        OrganizationMember,
        r#"
        SELECT organization_id, publisher_id, role as "role: OrganizationRole", joined_at
        FROM organization_members
        WHERE organization_id = $1
        "#,
        id
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| db_internal_error("list_org_members", e))?;

    Ok(Json(members))
}

/// Invite a member to an organization
pub async fn invite_member(
    State(state): State<AppState>,
    claims: AuthClaims,
    Path(id): Path<Uuid>,
    Json(payload): Json<InviteMemberRequest>,
) -> ApiResult<StatusCode> {
    // Check if user is an Admin of the org
    check_org_role(&state.pool, id, &claims.sub, OrganizationRole::Admin).await?;

    // Get inviter publisher ID
    let inviter_id: Uuid = sqlx::query_scalar!(
        "SELECT id FROM publishers WHERE stellar_address = $1",
        claims.sub
    )
    .fetch_one(&state.pool)
    .await
    .map_err(|e| db_internal_error("get_inviter_id", e))?;

    let token = Uuid::new_v4().to_string();
    let expires_at = Utc::now() + chrono::Duration::days(7);

    sqlx::query!(
        r#"
        INSERT INTO organization_invitations (organization_id, email, role, token, inviter_id, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        id,
        payload.email,
        payload.role as OrganizationRole,
        token,
        inviter_id,
        expires_at
    )
    .execute(&state.pool)
    .await
    .map_err(|e| db_internal_error("create_invitation", e))?;

    // In a real app, we would send an email here with the token.
    tracing::info!(email = %payload.email, token = %token, "Member invited to organization");

    Ok(StatusCode::ACCEPTED)
}

/// Accept an organization invitation
pub async fn accept_invitation(
    State(state): State<AppState>,
    claims: AuthClaims,
    Path(token): Path<String>,
) -> ApiResult<StatusCode> {
    let mut tx = state.pool.begin().await
        .map_err(|e| db_internal_error("begin_transaction", e))?;

    // 1. Validate invitation
    let invite = sqlx::query!(
        r#"
        SELECT * FROM organization_invitations
        WHERE token = $1 AND accepted_at IS NULL AND expires_at > NOW()
        "#,
        token
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| db_internal_error("get_invitation", e))?
    .ok_or(StatusCode::NOT_FOUND)?;

    // 2. Get publisher ID for accepting user
    let publisher_id: Uuid = sqlx::query_scalar!(
        "SELECT id FROM publishers WHERE stellar_address = $1",
        claims.sub
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_internal_error("get_publisher_id", e))?;

    // 3. Add member
    sqlx::query!(
        r#"
        INSERT INTO organization_members (organization_id, publisher_id, role)
        VALUES ($1, $2, $3)
        ON CONFLICT (organization_id, publisher_id) DO UPDATE SET role = EXCLUDED.role
        "#,
        invite.organization_id,
        publisher_id,
        invite.role
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| db_internal_error("add_member", e))?;

    // 4. Mark invitation as accepted
    sqlx::query!(
        "UPDATE organization_invitations SET accepted_at = NOW() WHERE id = $1",
        invite.id
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| db_internal_error("mark_invite_accepted", e))?;

    tx.commit().await.map_err(|e| db_internal_error("commit_transaction", e))?;

    Ok(StatusCode::OK)
}
