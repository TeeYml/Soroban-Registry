use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};
use shared::{BatchVerifyItem, BatchVerifyRequest, Contract};

use crate::{onchain_verification::OnChainVerifier, state::AppState};

pub async fn batch_verify_contracts(
    State(state): State<AppState>,
    Json(req): Json<BatchVerifyRequest>,
) -> impl IntoResponse {
    if req.contracts.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_request",
                "message": "contracts must not be empty"
            })),
        );
    }

    let verifier = OnChainVerifier::new();
    let results = stream::iter(req.contracts.into_iter().map(|item| {
        let state = state.clone();
        let verifier = verifier.clone();
        async move { verify_batch_item(&state, &verifier, item).await }
    }))
    .buffer_unordered(8)
    .collect::<Vec<Value>>()
    .await;

    let verified = results
        .iter()
        .filter(|result| result.get("verified").and_then(Value::as_bool) == Some(true))
        .count();
    let cached = results
        .iter()
        .filter(|result| result.pointer("/on_chain/cached").and_then(Value::as_bool) == Some(true))
        .count();

    (
        StatusCode::OK,
        Json(json!({
            "total": results.len(),
            "verified": verified,
            "failed": results.len().saturating_sub(verified),
            "cached": cached,
            "results": results
        })),
    )
}

async fn verify_batch_item(
    state: &AppState,
    onchain_verifier: &OnChainVerifier,
    item: BatchVerifyItem,
) -> Value {
    let contract = match sqlx::query_as::<_, Contract>(
        "SELECT * FROM contracts WHERE contract_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(&item.contract_id)
    .fetch_optional(&state.db)
    .await
    {
        Ok(Some(contract)) => contract,
        Ok(None) => {
            return json!({
                "contract_id": item.contract_id,
                "verified": false,
                "error": "contract_not_found"
            });
        }
        Err(err) => {
            return json!({
                "contract_id": item.contract_id,
                "verified": false,
                "error": format!("database error: {}", err)
            });
        }
    };

    let abi_json = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT abi FROM contract_abis WHERE contract_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(contract.id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .map(|value| value.to_string());

    let on_chain = match onchain_verifier
        .verify_contract(&state.cache, &contract, abi_json.as_deref())
        .await
    {
        Ok(result) => result,
        Err(err) => {
            return json!({
                "contract_id": contract.contract_id,
                "verified": false,
                "error": err.to_string()
            });
        }
    };

    let source_verification = match (&item.source_code, &item.compiler_version) {
        (Some(source_code), Some(compiler_version)) if !source_code.trim().is_empty() => Some(
            verifier::verify_contract(
                source_code,
                &contract.wasm_hash,
                Some(compiler_version),
                item.build_params.as_ref(),
            )
            .await,
        ),
        _ => None,
    };

    let verified = on_chain.contract_exists_on_chain
        && on_chain.wasm_hash_matches
        && on_chain.abi_valid
        && source_verification
            .as_ref()
            .map(|result| result.as_ref().map(|r| r.verified).unwrap_or(false))
            .unwrap_or(true);

    json!({
        "contract_id": contract.contract_id,
        "verified": verified,
        "network": contract.network.to_string(),
        "on_chain": on_chain,
        "source_verification": source_verification.map(|result| match result {
            Ok(value) => json!({
                "verified": value.verified,
                "compiled_wasm_hash": value.compiled_wasm_hash,
                "deployed_wasm_hash": value.deployed_wasm_hash,
                "message": value.message
            }),
            Err(err) => json!({
                "verified": false,
                "error": err.to_string()
            })
        })
    })
}
