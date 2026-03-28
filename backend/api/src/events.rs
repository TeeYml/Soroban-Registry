use crate::state::{AppState, RealtimeEvent};
use chrono::Utc;

pub fn emit_contract_deployment(
    state: &AppState,
    contract_id: String,
    contract_name: String,
    publisher: String,
    version: String,
) {
    let event = RealtimeEvent::ContractDeployed {
        contract_id,
        contract_name,
        publisher,
        version,
        timestamp: Utc::now().to_rfc3339(),
    };

    let _ = state.event_broadcaster.send(event);
}

pub fn emit_contract_update(
    state: &AppState,
    contract_id: String,
    update_type: String,
    details: serde_json::Value,
) {
    let event = RealtimeEvent::ContractUpdated {
        contract_id,
        update_type,
        details,
        timestamp: Utc::now().to_rfc3339(),
    };

    let _ = state.event_broadcaster.send(event);
}
