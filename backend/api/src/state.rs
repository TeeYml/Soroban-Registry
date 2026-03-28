use crate::auth::AuthManager;
use crate::cache::{CacheConfig, CacheLayer};
use crate::contract_events::ContractEventHub;
use crate::health_monitor::HealthMonitorStatus;
use crate::resource_tracking::ResourceManager;
use prometheus::Registry;
use sqlx::PgPool;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::broadcast;

#[derive(Clone, Debug, serde::Serialize)]
pub enum RealtimeEvent {
    ContractDeployed {
        contract_id: String,
        contract_name: String,
        publisher: String,
        version: String,
        timestamp: String,
    },
    ContractUpdated {
        contract_id: String,
        update_type: String,
        details: serde_json::Value,
        timestamp: String,
    },
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub started_at: Instant,
    pub cache: Arc<CacheLayer>,
    pub registry: Registry,
    pub job_engine: Arc<soroban_batch::engine::JobEngine>,
    pub is_shutting_down: Arc<AtomicBool>,
    pub health_monitor_status: HealthMonitorStatus,
    pub auth_mgr: Arc<RwLock<AuthManager>>,
    pub resource_mgr: Arc<RwLock<ResourceManager>>,
    pub event_broadcaster: broadcast::Sender<RealtimeEvent>,
}

impl AppState {
    pub async fn new(
        db: PgPool,
        registry: Registry,
        job_engine: Arc<soroban_batch::engine::JobEngine>,
        is_shutting_down: Arc<AtomicBool>,
    ) -> Result<Self, crate::shared::error::RegistryError> {
        let config = CacheConfig::from_env();
        let auth_mgr = Arc::new(RwLock::new(
            AuthManager::from_env().expect("JWT config validated at startup"),
        ));
        let resource_mgr = Arc::new(RwLock::new(ResourceManager::new()));
        let (event_broadcaster, _) = broadcast::channel(100);
        Self {
            db,
            started_at: Instant::now(),
            cache: Arc::new(CacheLayer::new(config).await),
            registry,
            job_engine,
            is_shutting_down,
            health_monitor_status: HealthMonitorStatus::default(),
            auth_mgr,
            resource_mgr,
            event_broadcaster,
        }
    }
}
