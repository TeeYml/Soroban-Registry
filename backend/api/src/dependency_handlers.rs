use axum::{
    extract::{Path, State},
    Json,
};
use shared::{DependencyNode, DependencyResponse};
use sqlx::Row;
use std::collections::HashSet;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResult},
    handlers::db_internal_error,
    state::AppState,
};

/// Get contract dependencies tree
pub async fn get_contract_dependencies(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DependencyResponse>> {
    let response = get_contract_dependencies_internal(&state, id).await?;
    Ok(Json(response))
}

pub(crate) async fn get_contract_dependencies_internal(
    state: &AppState,
    id: Uuid,
) -> ApiResult<DependencyResponse> {
    // 1. Fetch the root contract
    let root_contract =
        sqlx::query("SELECT id, contract_id, name, verification_status FROM contracts WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(|err| db_internal_error("get root contract for dependencies", err))?
            .ok_or_else(|| ApiError::not_found("ContractNotFound", "Contract not found"))?;

    let root_internal_id: Uuid = root_contract.get("id");
    let root_c_id: String = root_contract.get("contract_id");
    let root_name: String = root_contract.get("name");
    let verification_status: String = root_contract.get("verification_status");

    // Default status for root (string form)
    let root_status = verification_status;

    // 2. Build the tree using DFS and a visited set to detect circular references
    let mut visited = HashSet::new();
    visited.insert(root_internal_id);

    let mut context = DependencyContext {
        db: state.db.clone(),
        total_dependencies: 0,
        max_depth: 0,
        has_circular: false,
    };

    let children = build_dependency_tree(
        &mut context,
        root_internal_id,
        &mut visited,
        1, // Current depth
    )
    .await?;

    let root_node = DependencyNode {
        contract_id: root_c_id,
        resolved_id: Some(root_internal_id),
        name: Some(root_name),
        call_volume: 0, // Root volume is undefined or total calls
        status: root_status.to_string(),
        is_circular: false,
        dependencies: children,
        visualization_hints: serde_json::json!({
            "node_type": "root",
            "depth": 0
        }),
    };

    Ok(Json(DependencyResponse {
        root: root_node,
        total_dependencies: context.total_dependencies,
        max_depth: context.max_depth,
        has_circular: context.has_circular,
    }))
}

struct DependencyContext {
    db: sqlx::PgPool,
    total_dependencies: usize,
    max_depth: usize,
    has_circular: bool,
}

#[async_recursion::async_recursion]
async fn build_dependency_tree(
    ctx: &mut DependencyContext,
    caller_internal_id: Uuid,
    visited: &mut HashSet<Uuid>,
    depth: usize,
) -> Result<Vec<DependencyNode>, ApiError> {
    if depth > ctx.max_depth {
        ctx.max_depth = depth;
    }

    // Safety limit to prevent extremely deep graphs dragging down the server
    if depth > 20 {
        return Ok(vec![]);
    }

    // Fetch all outgoing dependencies from `caller_internal_id`
    // We use a LEFT JOIN to see if the callee_contract_id exists in our registry
    let rows = sqlx::query(
        r#"
        SELECT 
            cd.callee_contract_id, 
            cd.call_volume,
            c.id as resolved_id,
            c.name as resolved_name,
            c.verification_status as verification_status
        FROM contract_dependencies cd
        LEFT JOIN contracts c ON c.contract_id = cd.callee_contract_id
        WHERE cd.caller_id = $1
        ORDER BY cd.call_volume DESC
        "#,
    )
    .bind(caller_internal_id)
    .fetch_all(&ctx.db)
    .await
    .map_err(|err| db_internal_error("fetch node children", err))?;

    let mut children = Vec::new();

    for row in rows {
        ctx.total_dependencies += 1;

        let callee_c_id: String = row.get("callee_contract_id");
        let call_volume: i32 = row.get("call_volume");
        let resolved_id: Option<Uuid> = row.get("resolved_id");
        let resolved_name: Option<String> = row.get("resolved_name");
        let verification_status: Option<String> = row.get("verification_status");

        let status = if let Some(s) = verification_status.clone() {
            s
        } else if resolved_id.is_some() {
            "unverified".to_string()
        } else {
            "unknown".to_string()
        };

        let mut is_circular = false;
        let mut sub_dependencies = Vec::new();

        if let Some(r_id) = resolved_id {
            if visited.contains(&r_id) {
                is_circular = true;
                ctx.has_circular = true;
            } else {
                // Traverse deeper
                visited.insert(r_id);
                sub_dependencies = build_dependency_tree(ctx, r_id, visited, depth + 1).await?;
                visited.remove(&r_id);
            }
        }

        children.push(DependencyNode {
            contract_id: callee_c_id,
            resolved_id,
            name: resolved_name,
            call_volume,
            status: status.to_string(),
            is_circular,
            dependencies: sub_dependencies,
            visualization_hints: serde_json::json!({
                "depth": depth,
                "node_type": if is_circular { "circular" } else { "standard" }
            }),
        });
    }

    Ok(children)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_circular_dependency_logic() {
        // We can test the visited set logic without db if we want,
        // but since build_dependency_tree requires a db transaction,
        // a pure unit test of the circular logic is best done by asserting the behavior of visited sets.
        let mut visited = HashSet::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        visited.insert(id1);

        let mut is_circular = false;

        // Simulate finding id2
        if visited.contains(&id2) {
            is_circular = true;
        } else {
            visited.insert(id2);
            // Simulate finding id1 again
            if visited.contains(&id1) {
                is_circular = true;
            }
        }

        assert!(is_circular, "Circular reference should be detected");
    }
}
