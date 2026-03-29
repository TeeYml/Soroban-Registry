use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// EXISTING REGISTRY TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Represents a smart contract in the registry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[schema(example = json!({
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "contract_id": "C...1234",
    "wasm_hash": "a1b2c3d4...",
    "name": "YieldOptimizer",
    "description": "Optimizes yield across protocols",
    "publisher_id": "550e8400-e29b-41d4-a716-446655440001",
    "network": "mainnet",
    "is_verified": true,
    "category": "DeFi",
    "tags": ["yield", "optimization"],
    "created_at": "2023-10-27T10:00:00Z",
    "updated_at": "2023-10-27T10:00:00Z"
}))]
pub struct Contract {
    pub id: Uuid,
    pub contract_id: String,
    pub wasm_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub publisher_id: Uuid,
    pub network: Network,
    pub is_verified: bool,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub last_accessed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub health_score: i32,
    #[serde(default)]
    pub is_maintenance: bool,
    /// Groups rows that represent the same logical contract across networks (Issue #43)
    #[serde(default)]
    pub logical_id: Option<Uuid>,
    /// Per-network config: { "mainnet": { contract_id, is_verified, min_version, max_version }, ... }
    #[serde(default)]
    pub network_configs: Option<serde_json::Value>,
    /// Search relevance score (calculated at runtime)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance_score: Option<f64>,
    /// Organization that owns this contract (for private registries)
    pub organization_id: Option<Uuid>,
    /// Visibility level
    pub visibility: VisibilityType,
    /// The currently active version string for this contract (Issue #486)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<String>,
}

#[derive(
    Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema, PartialEq, Default,
)]
#[sqlx(type_name = "visibility_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum VisibilityType {
    #[default]
    Public,
    Private,
}

/// Response for GET /contracts/:id with optional network-specific slice (Issue #43)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractGetResponse {
    #[serde(flatten)]
    pub contract: Contract,
    /// When ?network= is set, the requested network
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_network: Option<Network>,
    /// When ?network= is set, that network's config slice
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_config: Option<NetworkConfig>,
}

/// Per-network config: address, verified status, min/max version (Issue #43)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NetworkConfig {
    pub contract_id: String,
    pub is_verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NetworkStatus {
    Online,
    Offline,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NetworkEndpoints {
    pub rpc_url: String,
    pub health_url: String,
    pub explorer_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendbot_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub network_type: Network,
    pub status: NetworkStatus,
    pub endpoints: NetworkEndpoints,
    pub last_checked_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_indexed_ledger_height: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_indexed_at: Option<DateTime<Utc>>,
    pub consecutive_failures: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NetworkListResponse {
    pub networks: Vec<NetworkInfo>,
    pub cached_at: DateTime<Utc>,
}

/// Network where the contract is deployed
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "network_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Futurenet,
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::Mainnet => write!(f, "mainnet"),
            Network::Testnet => write!(f, "testnet"),
            Network::Futurenet => write!(f, "futurenet"),
        }
    }
}

/// Upgrade strategy for contract upgrades
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "upgrade_strategy_type", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum UpgradeStrategy {
    Proxy,
    Uups,
    DataMigration,
    ShadowContract,
}

/// Contract version information
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractVersion {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub version: String,
    pub wasm_hash: String,
    pub source_url: Option<String>,
    pub commit_hash: Option<String>,
    pub release_notes: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_schema: Option<serde_json::Value>,
    /// Optional Ed25519 signature over "{contract_id}:{version}:{wasm_hash}"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Publisher's public key corresponding to the signature (base64-encoded ed25519 key)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_key: Option<String>,
    /// Signature algorithm identifier (e.g. "ed25519")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_algorithm: Option<String>,
    /// Structured notes describing what changed in this version (Issue #486)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_notes: Option<String>,
    /// True when this version was created by reverting to a previous version (Issue #486)
    #[serde(default)]
    pub is_revert: bool,
    /// The version string that was reverted to, when is_revert = true (Issue #486)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverted_from: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// MULTI-TENANCY TYPES (Issue #420)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema, PartialEq)]
#[sqlx(type_name = "organization_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum OrganizationRole {
    Admin,
    Member,
    Viewer,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_private: bool,
    pub quota_contracts: i32,
    pub rate_limit_requests: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct OrganizationMember {
    pub organization_id: Uuid,
    pub publisher_id: Uuid,
    pub role: OrganizationRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct OrganizationInvitation {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub email: String,
    pub role: OrganizationRole,
    pub token: String,
    pub inviter_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateOrganizationRequest {
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub is_private: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InviteMemberRequest {
    pub email: String,
    pub role: OrganizationRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateOrganizationRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_private: Option<bool>,
}

/// Verification status and details
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct Verification {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub status: VerificationStatus,
    pub source_code: Option<String>,
    pub build_params: Option<serde_json::Value>,
    pub compiler_version: Option<String>,
    pub verified_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Verification status enum
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "verification_status", rename_all = "lowercase")]
pub enum VerificationStatus {
    Pending,
    Verified,
    Failed,
}

/// Security audit status of the contract (Issue #401)
#[derive(
    Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema, PartialEq, Default,
)]
#[sqlx(type_name = "audit_status_type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuditStatus {
    #[default]
    None,
    Pending,
    Passed,
    Failed,
}

/// Contract maturity level - indicates stability and production readiness
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub enum MaturityLevel {
    Experimental,
    Beta,
    Stable,
    Production,
}

/// Publisher/developer information
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[schema(example = json!({
    "id": "550e8400-e29b-41d4-a716-446655440001",
    "stellar_address": "GABC...",
    "username": "SorobanDev",
    "email": "dev@soroban.io",
    "github_url": "https://github.com/sorobandev",
    "website": "https://soroban.io",
    "created_at": "2023-10-27T10:00:00Z"
}))]
pub struct Publisher {
    pub id: Uuid,
    pub stellar_address: String,
    pub username: Option<String>,
    pub email: Option<String>,
    pub github_url: Option<String>,
    pub website: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Contract interaction statistics
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractStats {
    pub contract_id: Uuid,
    pub total_deployments: i64,
    pub total_interactions: i64,
    pub unique_users: i64,
    pub last_interaction: Option<DateTime<Utc>>,
}

/// GraphNode (minimal contract info for graph rendering)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct GraphNode {
    pub id: Uuid,
    pub contract_id: String,
    pub name: String,
    pub network: Network,
    pub is_verified: bool,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

/// Graph edge (dependency relationship)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct GraphEdge {
    pub source: Uuid,
    pub target: Uuid,
    pub dependency_type: String,
    pub call_frequency: Option<i64>,
    pub call_volume: Option<i64>,
    pub is_estimated: bool,
    pub is_circular: bool,
}

/// Full graph response
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GraphResponse {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Request to publish a new contract
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PublishRequest {
    pub contract_id: String,
    pub wasm_hash: String,
    pub name: String,
    pub description: Option<String>,
    pub network: Network,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub source_url: Option<String>,
    pub publisher_address: String,
    // Dependencies (new field)
    #[serde(default)]
    pub dependencies: Vec<DependencyDeclaration>,
    /// Whether this was published via CI/CD (Issue #529)
    #[serde(default)]
    pub is_cicd: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateContractMetadataRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChangePublisherRequest {
    pub publisher_address: String,
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct UpdateContractStatusRequest {
    pub status: String,
    pub error_message: Option<String>,
    pub user_id: Option<Uuid>,
}

/// Request to create a new contract version with ABI
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateContractVersionRequest {
    pub contract_id: String,
    pub version: String,
    pub wasm_hash: String,
    pub abi: serde_json::Value,
    pub source_url: Option<String>,
    pub commit_hash: Option<String>,
    pub release_notes: Option<String>,
    /// Structured change notes for this version (Issue #486)
    #[serde(default)]
    pub change_notes: Option<String>,
    /// Optional Ed25519 signature and publisher key metadata for this version
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub publisher_key: Option<String>,
    #[serde(default)]
    pub signature_algorithm: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// VERSION TRACKING TYPES (Issue #486)
// ═══════════════════════════════════════════════════════════════════════════

/// A single field-level difference between two contract versions
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VersionFieldDiff {
    /// Name of the field that changed
    pub field: String,
    /// Value in the `from` version (null if the field was absent)
    pub from_value: Option<serde_json::Value>,
    /// Value in the `to` version (null if the field was removed)
    pub to_value: Option<serde_json::Value>,
}

/// Response for GET /api/contracts/:id/versions/compare
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VersionCompareResponse {
    pub contract_id: Uuid,
    pub from_version: ContractVersion,
    pub to_version: ContractVersion,
    /// List of fields that differ between the two versions
    pub differences: Vec<VersionFieldDiff>,
    /// Whether the WASM hash changed (indicates a code change)
    pub wasm_changed: bool,
}

/// Request body for POST /api/admin/contracts/:id/versions/:version/revert
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RevertVersionRequest {
    /// Optional notes explaining why the revert was performed
    pub change_notes: Option<String>,
    /// UUID of the admin performing the revert (used for audit log)
    pub admin_id: Uuid,
}

// ────────────────────────────────────────────────────────────────────────────
// Deprecation management (issue #65)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeprecationStatus {
    Active,
    Deprecated,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeprecationInfo {
    pub contract_id: String,
    pub status: DeprecationStatus,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub retirement_at: Option<DateTime<Utc>>,
    pub replacement_contract_id: Option<String>,
    pub migration_guide_url: Option<String>,
    pub notes: Option<String>,
    pub days_remaining: Option<i64>,
    pub dependents_notified: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeprecateContractRequest {
    pub retirement_at: DateTime<Utc>,
    pub replacement_contract_id: Option<String>,
    pub migration_guide_url: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct DeprecationNotification {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub deprecated_contract_id: Uuid,
    pub message: String,
    pub created_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

/// Response for impact analysis
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ImpactAnalysisResponse {
    pub contract_id: Uuid,
    pub change_type: Option<String>,
    pub affected_count: usize,
    pub affected_contracts: Vec<Contract>,
    pub has_cycles: bool,
}
/// Dependency declaration in publish request
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DependencyDeclaration {
    pub name: String,
    pub version_constraint: String,
}

/// Contract dependency record (database row)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractDependency {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub dependency_name: String,
    pub dependency_contract_id: Option<Uuid>,
    pub version_constraint: String,
    pub created_at: DateTime<Utc>,
}

/// Tracks migration scripts between contract versions
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct MigrationScript {
    pub id: Uuid,
    pub from_version: Uuid,
    pub to_version: Uuid,
    pub script_path: String,
    pub checksum: String,
    pub created_at: DateTime<Utc>,
}

/// Recursive dependency tree node for API response
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DependencyTreeNode {
    pub contract_id: String, // Public key ID
    pub name: String,
    pub current_version: String,
    pub constraint_to_parent: String,
    pub dependencies: Vec<DependencyTreeNode>,
}

/// Request to verify a contract
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VerifyRequest {
    pub contract_id: String,
    pub source_code: String,
    pub build_params: serde_json::Value,
    pub compiler_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchVerifyItem {
    pub contract_id: String,
    #[serde(default)]
    pub source_code: Option<String>,
    #[serde(default)]
    pub build_params: Option<serde_json::Value>,
    #[serde(default)]
    pub compiler_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchVerifyRequest {
    pub contracts: Vec<BatchVerifyItem>,
}

/// Sorting options for contracts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
pub enum SortBy {
    #[serde(rename = "created_at", alias = "createdat")]
    CreatedAt,
    #[serde(rename = "updated_at", alias = "updatedat")]
    UpdatedAt,
    VerifiedAt,
    LastAccessedAt,
    Popularity,
    #[serde(rename = "deployments")]
    Deployments,
    // Kept for backwards/UX compatibility: the frontend supports "downloads".
    #[serde(rename = "interactions", alias = "downloads")]
    Interactions,
    #[serde(rename = "relevance")]
    Relevance,
}

/// Sorting order
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Search/filter parameters for contracts
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ContractSearchParams {
    pub query: Option<String>,
    pub network: Option<Network>,
    /// Multiple networks filter (e.g. ?networks=mainnet&networks=testnet)
    pub networks: Option<Vec<Network>>,
    pub verified_only: Option<bool>,
    pub category: Option<String>,
    /// Multiple categories filter (e.g. ?categories=DeFi&categories=NFT)
    pub categories: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub maturity: Option<MaturityLevel>,
    pub page: Option<i64>,
    #[serde(alias = "page_size")]
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub sort_by: Option<SortBy>,
    pub sort_order: Option<SortOrder>,
    pub cursor: Option<String>,
    pub created_from: Option<DateTime<Utc>>,
    pub created_to: Option<DateTime<Utc>>,
    pub updated_from: Option<DateTime<Utc>>,
    pub updated_to: Option<DateTime<Utc>>,
    pub verified_from: Option<DateTime<Utc>>,
    pub verified_to: Option<DateTime<Utc>>,
    pub last_accessed_from: Option<DateTime<Utc>>,
    pub last_accessed_to: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SearchSuggestion {
    pub text: String,
    pub kind: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SearchSuggestionsResponse {
    pub items: Vec<SearchSuggestion>,
}

/// Pagination params for contract versions (limit/offset style)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VersionPaginationParams {
    #[serde(default = "default_version_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_version_limit() -> i64 {
    20
}

/// Paginated version response (limit/offset style per issue #32 spec)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PaginatedVersionResponse {
    pub items: Vec<ContractVersion>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

/// Paginated response
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

impl<T> PaginatedResponse<T> {
    pub fn new(items: Vec<T>, total: i64, page: i64, limit: i64) -> Self {
        let total_pages = if limit > 0 {
            (total as f64 / limit as f64).ceil() as i64
        } else {
            0
        };
        Self {
            items,
            total,
            page,
            page_size: limit,
            total_pages,
            next_cursor: None,
            prev_cursor: None,
        }
    }

    pub fn with_cursors(mut self, next: Option<String>, prev: Option<String>) -> Self {
        self.next_cursor = next;
        self.prev_cursor = prev;
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTRACT INTERACTION HISTORY (Issue #46)
// ═══════════════════════════════════════════════════════════════════════════

/// One contract invocation row (DB)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractInteraction {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub user_address: Option<String>,
    pub interaction_type: String,
    pub transaction_hash: Option<String>,
    pub method: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub return_value: Option<serde_json::Value>,
    pub interaction_timestamp: Option<DateTime<Utc>>,
    pub interaction_count: Option<i64>,
    pub network: Option<Network>,
    pub created_at: DateTime<Utc>,
}

/// Response item for GET /api/contracts/:id/interactions
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractInteractionResponse {
    pub id: Uuid,
    pub account: Option<String>,
    pub method: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub return_value: Option<serde_json::Value>,
    pub transaction_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct InteractionsQueryParams {
    #[serde(default = "default_interactions_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub account: Option<String>,
    pub method: Option<String>,
    pub from_timestamp: Option<String>,
    pub to_timestamp: Option<String>,
    pub days: Option<i64>,
    pub interaction_type: Option<String>,
    pub network: Option<Network>,
    pub cursor: Option<String>,
}

fn default_interactions_limit() -> i64 {
    50
}

/// Request body for POST /api/contracts/:id/interactions (single)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateInteractionRequest {
    pub account: Option<String>,
    pub interaction_type: Option<String>,
    pub method: Option<String>,
    pub transaction_hash: Option<String>,
    pub target_contract_id: Option<String>,
    pub parameters: Option<serde_json::Value>,
    pub return_value: Option<serde_json::Value>,
    pub timestamp: Option<DateTime<Utc>>,
    pub network: Option<Network>,
}

/// Request body for POST /api/contracts/:id/interactions/batch
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateInteractionBatchRequest {
    pub interactions: Vec<CreateInteractionRequest>,
}

/// Paginated interactions response
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InteractionsListResponse {
    pub items: Vec<ContractInteractionResponse>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InteractionTimeSeriesPoint {
    pub date: chrono::NaiveDate,
    pub interaction_type: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InteractionTimeSeriesResponse {
    pub contract_id: Uuid,
    pub days: i64,
    pub interactions_this_week: i64,
    pub interactions_last_week: i64,
    pub is_trending: bool,
    pub series: Vec<InteractionTimeSeriesPoint>,
}

/// Migration status
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, utoipa::ToSchema)]
#[sqlx(type_name = "migration_status", rename_all = "snake_case")]
pub enum MigrationStatus {
    Pending,
    Success,
    Failed,
    RolledBack,
}

/// Represents a contract state migration
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct Migration {
    pub id: Uuid,
    pub contract_id: String,
    pub status: MigrationStatus,
    pub wasm_hash: String,
    pub log_output: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new migration record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMigrationRequest {
    pub contract_id: String,
    pub wasm_hash: String,
}

/// Request to update a migration's status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMigrationStatusRequest {
    pub status: MigrationStatus,
    pub log_output: Option<String>,
}

// ────────────────────────────────────────────────────────────────────────────
// Contributor models
// ────────────────────────────────────────────────────────────────────────────

/// A contract creator profile with optional verification badge
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Contributor {
    pub id: Uuid,
    pub stellar_address: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub links: serde_json::Value,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Contributor profile with aggregated contract stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorWithStats {
    pub id: Uuid,
    pub stellar_address: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub links: serde_json::Value,
    pub is_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub contract_count: i64,
}

impl ContributorWithStats {
    pub fn from_contributor(c: Contributor, contract_count: i64) -> Self {
        Self {
            id: c.id,
            stellar_address: c.stellar_address,
            name: c.name,
            avatar_url: c.avatar_url,
            bio: c.bio,
            links: c.links,
            is_verified: c.is_verified,
            created_at: c.created_at,
            updated_at: c.updated_at,
            contract_count,
        }
    }
}

/// Request to create a contributor profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateContributorRequest {
    pub stellar_address: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub links: Option<serde_json::Value>,
}

/// Request to update a contributor profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateContributorRequest {
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub links: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, utoipa::ToSchema)]
#[sqlx(type_name = "deployment_environment", rename_all = "lowercase")]
pub enum DeploymentEnvironment {
    Blue,
    Green,
}

impl std::fmt::Display for DeploymentEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeploymentEnvironment::Blue => write!(f, "blue"),
            DeploymentEnvironment::Green => write!(f, "green"),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, utoipa::ToSchema)]
#[sqlx(type_name = "deployment_status", rename_all = "lowercase")]
pub enum DeploymentStatus {
    Active,
    Inactive,
    Testing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractDeployment {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub environment: DeploymentEnvironment,
    pub status: DeploymentStatus,
    pub wasm_hash: String,
    pub deployed_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub health_checks_passed: i32,
    pub health_checks_failed: i32,
    pub last_health_check_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct DeploymentSwitch {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub from_environment: DeploymentEnvironment,
    pub to_environment: DeploymentEnvironment,
    pub switched_at: DateTime<Utc>,
    pub switched_by: Option<String>,
    pub rollback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "canary_status", rename_all = "snake_case")]
pub enum CanaryStatus {
    Pending,
    Active,
    Paused,
    Completed,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "rollout_stage", rename_all = "snake_case")]
pub enum RolloutStage {
    Stage1,
    Stage2,
    Stage3,
    Stage4,
    Complete,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct CanaryRelease {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub from_deployment_id: Option<Uuid>,
    pub to_deployment_id: Uuid,
    pub status: CanaryStatus,
    pub current_stage: RolloutStage,
    pub current_percentage: i32,
    pub target_percentage: i32,
    pub error_rate_threshold: Decimal,
    pub current_error_rate: Option<Decimal>,
    pub total_requests: i32,
    pub error_count: i32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct CanaryMetric {
    pub id: Uuid,
    pub canary_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub requests: i32,
    pub errors: i32,
    pub error_rate: rust_decimal::Decimal,
    pub avg_response_time_ms: Option<Decimal>,
    pub p95_response_time_ms: Option<Decimal>,
    pub p99_response_time_ms: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct CanaryUserAssignment {
    pub id: Uuid,
    pub canary_id: Uuid,
    pub user_address: String,
    pub assigned_at: DateTime<Utc>,
    pub notified: bool,
    pub notified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateCanaryRequest {
    pub contract_id: String,
    pub to_deployment_id: String,
    pub error_rate_threshold: Option<f64>,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AdvanceCanaryRequest {
    pub canary_id: String,
    pub target_percentage: Option<i32>,
    pub advanced_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RecordCanaryMetricRequest {
    pub canary_id: String,
    pub requests: i32,
    pub errors: i32,
    pub avg_response_time_ms: Option<f64>,
    pub p95_response_time_ms: Option<f64>,
    pub p99_response_time_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "ab_test_status", rename_all = "snake_case")]
pub enum AbTestStatus {
    Draft,
    Running,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "variant_type", rename_all = "snake_case")]
pub enum VariantType {
    Control,
    Treatment,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AbTest {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub status: AbTestStatus,
    pub traffic_split: Decimal,
    pub variant_a_deployment_id: Uuid,
    pub variant_b_deployment_id: Uuid,
    pub primary_metric: String,
    pub hypothesis: Option<String>,
    pub significance_threshold: Decimal,
    pub min_sample_size: i32,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AbTestVariant {
    pub id: Uuid,
    pub test_id: Uuid,
    pub variant_type: VariantType,
    pub deployment_id: Uuid,
    pub traffic_percentage: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AbTestAssignment {
    pub id: Uuid,
    pub test_id: Uuid,
    pub user_address: String,
    pub variant_type: VariantType,
    pub assigned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AbTestMetric {
    pub id: Uuid,
    pub test_id: Uuid,
    pub variant_type: VariantType,
    pub metric_name: String,
    pub metric_value: Decimal,
    pub user_address: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AbTestResult {
    pub id: Uuid,
    pub test_id: Uuid,
    pub variant_type: VariantType,
    pub sample_size: i32,
    pub mean_value: Option<Decimal>,
    pub std_deviation: Option<Decimal>,
    pub confidence_interval_lower: Option<Decimal>,
    pub confidence_interval_upper: Option<Decimal>,
    pub p_value: Option<Decimal>,
    pub statistical_significance: Option<Decimal>,
    pub is_winner: bool,
    pub calculated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateAbTestRequest {
    pub contract_id: String,
    pub name: String,
    pub description: Option<String>,
    pub traffic_split: Option<f64>,
    pub variant_a_deployment_id: String,
    pub variant_b_deployment_id: String,
    pub primary_metric: String,
    pub hypothesis: Option<String>,
    pub significance_threshold: Option<f64>,
    pub min_sample_size: Option<i32>,
    pub created_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RecordAbTestMetricRequest {
    pub test_id: String,
    pub user_address: Option<String>,
    pub metric_name: String,
    pub metric_value: f64,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GetVariantRequest {
    pub test_id: String,
    pub user_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "metric_type", rename_all = "snake_case")]
pub enum MetricType {
    ExecutionTime,
    MemoryUsage,
    StorageIo,
    GasConsumption,
    ErrorRate,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "alert_severity", rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct PerformanceMetric {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub metric_type: MetricType,
    pub function_name: Option<String>,
    pub value: Decimal,
    pub p50: Option<Decimal>,
    pub p95: Option<Decimal>,
    pub p99: Option<Decimal>,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct PerformanceAnomaly {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub metric_type: MetricType,
    pub function_name: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub baseline_value: Option<Decimal>,
    pub current_value: Option<Decimal>,
    pub deviation_percent: Option<Decimal>,
    pub severity: AlertSeverity,
    pub resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct PerformanceAlert {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub metric_type: MetricType,
    pub threshold_type: String,
    pub threshold_value: Decimal,
    pub current_value: Decimal,
    pub severity: AlertSeverity,
    pub triggered_at: DateTime<Utc>,
    pub acknowledged: bool,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<String>,
    pub resolved: bool,
    pub resolved_at: Option<DateTime<Utc>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct PerformanceTrend {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub function_name: Option<String>,
    pub metric_type: MetricType,
    pub timeframe_start: DateTime<Utc>,
    pub timeframe_end: DateTime<Utc>,
    pub avg_value: Option<Decimal>,
    pub min_value: Option<Decimal>,
    pub max_value: Option<Decimal>,
    pub p50_value: Option<Decimal>,
    pub p95_value: Option<Decimal>,
    pub p99_value: Option<Decimal>,
    pub sample_count: i32,
    pub trend_direction: Option<String>,
    pub change_percent: Option<Decimal>,
    pub calculated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct PerformanceAlertConfig {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub metric_type: MetricType,
    pub threshold_type: String,
    pub threshold_value: Decimal,
    pub severity: AlertSeverity,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RecordPerformanceMetricRequest {
    pub contract_id: String,
    pub metric_type: MetricType,
    pub function_name: Option<String>,
    pub value: f64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateAlertConfigRequest {
    pub contract_id: String,
    pub metric_type: MetricType,
    pub threshold_type: String,
    pub threshold_value: f64,
    pub severity: Option<AlertSeverity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "similarity_match_type", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SimilarityMatchType {
    ExactClone,
    NearDuplicate,
    Similar,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "similarity_review_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SimilarityReviewStatus {
    None,
    Pending,
    Reviewed,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractSimilaritySignature {
    pub contract_id: Uuid,
    pub representation_type: String,
    pub exact_hash: String,
    pub simhash: i64,
    pub token_count: i32,
    pub source_length: i32,
    pub wasm_hash: String,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractSimilarityReport {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub similar_contract_id: Uuid,
    pub similarity_score: Decimal,
    pub exact_clone: bool,
    pub match_type: SimilarityMatchType,
    pub suspicious: bool,
    pub flagged_for_review: bool,
    pub review_status: SimilarityReviewStatus,
    pub reasons: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractSimilarityResult {
    pub contract_id: Uuid,
    pub similar_contract_id: Uuid,
    pub similar_contract_name: String,
    pub similar_contract_address: String,
    pub similarity_score: f64,
    pub exact_clone: bool,
    pub match_type: SimilarityMatchType,
    pub suspicious: bool,
    pub flagged_for_review: bool,
    pub review_status: SimilarityReviewStatus,
    pub reasons: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractSimilarityResponse {
    pub contract_id: Uuid,
    pub total_matches: usize,
    pub suspicious_matches: usize,
    pub items: Vec<ContractSimilarityResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchSimilarityAnalysisRequest {
    #[serde(default)]
    pub contract_ids: Vec<String>,
    pub limit_per_contract: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchSimilarityAnalysisItem {
    pub contract_id: Uuid,
    pub analyzed_contracts: usize,
    pub suspicious_matches: usize,
    pub items: Vec<ContractSimilarityResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BatchSimilarityAnalysisResponse {
    pub analyzed_contracts: usize,
    pub total_flagged_for_review: usize,
    pub items: Vec<BatchSimilarityAnalysisItem>,
}

// ────────────────────────────────────────────────────────────────────────────
// Custom contract metrics (issue #89)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "custom_metric_type", rename_all = "snake_case")]
pub enum CustomMetricType {
    Counter,
    Gauge,
    Histogram,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct CustomMetric {
    pub id: Uuid,
    pub contract_id: String,
    pub metric_name: String,
    pub metric_type: CustomMetricType,
    pub value: Decimal,
    pub unit: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub ledger_sequence: Option<i64>,
    pub transaction_hash: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub network: Network,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RecordCustomMetricRequest {
    pub contract_id: String,
    pub metric_name: String,
    pub metric_type: CustomMetricType,
    pub value: f64,
    pub unit: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub ledger_sequence: Option<i64>,
    pub transaction_hash: Option<String>,
    pub timestamp: Option<DateTime<Utc>>,
    pub network: Option<Network>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct CustomMetricAggregate {
    pub contract_id: String,
    pub metric_name: String,
    pub metric_type: CustomMetricType,
    pub bucket_start: DateTime<Utc>,
    pub bucket_end: DateTime<Utc>,
    pub sample_count: i32,
    pub sum_value: Option<Decimal>,
    pub avg_value: Option<Decimal>,
    pub min_value: Option<Decimal>,
    pub max_value: Option<Decimal>,
    pub p50_value: Option<Decimal>,
    pub p95_value: Option<Decimal>,
    pub p99_value: Option<Decimal>,
}

// ────────────────────────────────────────────────────────────────────────────
// Analytics models
// ────────────────────────────────────────────────────────────────────────────

/// Types of analytics events tracked by the system
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, utoipa::ToSchema)]
#[sqlx(type_name = "analytics_event_type", rename_all = "snake_case")]
pub enum AnalyticsEventType {
    ContractPublished,
    ContractVerified,
    ContractDeployed,
    VersionCreated,
    ContractUpdated,
    PublisherCreated,
    SearchClick,
}

impl std::fmt::Display for AnalyticsEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContractPublished => write!(f, "contract_published"),
            Self::ContractVerified => write!(f, "contract_verified"),
            Self::ContractDeployed => write!(f, "contract_deployed"),
            Self::VersionCreated => write!(f, "version_created"),
            Self::ContractUpdated => write!(f, "contract_updated"),
            Self::PublisherCreated => write!(f, "publisher_created"),
            Self::SearchClick => write!(f, "search_click"),
        }
    }
}

/// A simplified entry for the activity feed API
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ActivityFeedEntry {
    pub id: Uuid,
    pub event_type: AnalyticsEventType,
    pub contract_id: Option<Uuid>,
    pub contract_name: Option<String>,
    pub contract_stellar_id: Option<String>,
    pub publisher_id: Option<Uuid>,
    pub publisher_name: Option<String>,
    pub network: Option<Network>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// A raw analytics event recorded when a contract lifecycle action occurs
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct AnalyticsEvent {
    pub id: Uuid,
    pub event_type: AnalyticsEventType,
    pub contract_id: Uuid,
    pub user_address: Option<String>,
    pub network: Option<Network>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Pre-computed daily aggregate for a single contract
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct DailyAggregate {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub date: chrono::NaiveDate,
    pub deployment_count: i32,
    pub unique_deployers: i32,
    pub verification_count: i32,
    pub publish_count: i32,
    pub version_count: i32,
    pub total_events: i32,
    pub unique_users: i32,
    pub network_breakdown: serde_json::Value,
    pub top_users: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ────────────────────────────────────────────────────────────────────────────
// Analytics API response DTOs
// ────────────────────────────────────────────────────────────────────────────

/// Top-level response for GET /api/contracts/:id/analytics
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractAnalyticsResponse {
    pub contract_id: Uuid,
    pub deployments: DeploymentStats,
    pub interactors: InteractorStats,
    pub timeline: Vec<TimelineEntry>,
}

/// Deployment statistics
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeploymentStats {
    pub count: i64,
    pub unique_users: i64,
    pub by_network: serde_json::Value,
}

/// Interactor / unique-user statistics
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct InteractorStats {
    pub unique_count: i64,
    pub top_users: Vec<TopUser>,
}

/// A user ranked by interaction count
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TopUser {
    pub address: String,
    pub count: i64,
}

/// One data-point in the 30-day timeline
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TimelineEntry {
    pub date: chrono::NaiveDate,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeployGreenRequest {
    pub contract_id: String,
    pub wasm_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SwitchDeploymentRequest {
    pub contract_id: String,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct HealthCheckRequest {
    pub contract_id: String,
    pub environment: DeploymentEnvironment,
    pub passed: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// POPULARITY / TRENDING
// ═══════════════════════════════════════════════════════════════════════════

/// Query parameters for the trending contracts endpoint
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct TrendingParams {
    /// Max results to return (default 10, max 50)
    pub limit: Option<i64>,
    /// Timeframe for trending calculation: "7d", "30d", "90d" (default "7d")
    pub timeframe: Option<String>,
}

/// Response DTO for a trending contract
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct TrendingContract {
    // Core contract fields
    pub id: Uuid,
    pub contract_id: String,
    pub name: String,
    pub description: Option<String>,
    pub network: Network,
    pub is_verified: bool,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    // Popularity metrics
    pub popularity_score: f64,
    pub deployment_count: i64,
    pub interaction_count: i64,
}

// MULTI-SIGNATURE DEPLOYMENT TYPES  (issue #47)
// ═══════════════════════════════════════════════════════════════════════════
// ════════════════════════════════════════════════════════════════════════════
// Audit Log & Version History types
// ════════════════════════════════════════════════════════════════════════════

/// The type of mutation that triggered an audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "audit_action_type", rename_all = "snake_case")]
pub enum AuditActionType {
    ContractPublished,
    MetadataUpdated,
    VerificationChanged,
    PublisherChanged,
    VersionCreated,
    Rollback,
}

impl std::fmt::Display for AuditActionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::ContractPublished => "contract_published",
            Self::MetadataUpdated => "metadata_updated",
            Self::VerificationChanged => "verification_changed",
            Self::PublisherChanged => "publisher_changed",
            Self::VersionCreated => "version_created",
            Self::Rollback => "rollback",
        };
        write!(f, "{}", s)
    }
}

/// One immutable row in `contract_audit_log`.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractAuditLog {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub action_type: AuditActionType,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub changed_by: String,
    pub timestamp: DateTime<Utc>,
    pub previous_hash: Option<String>,
    pub hash: Option<String>,
    pub signature: Option<String>,
}

/// Full contract state captured at each audited change in `contract_snapshots`.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractSnapshot {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub version_number: i32,
    pub snapshot_data: serde_json::Value,
    pub audit_log_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// A single field-level change between two snapshots.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FieldChange {
    pub field: String,
    pub from: serde_json::Value,
    pub to: serde_json::Value,
}

/// Response for GET /api/contracts/:id/versions/:v1/diff/:v2
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct VersionDiff {
    pub contract_id: Uuid,
    pub from_version: i32,
    pub to_version: i32,
    /// Fields present in v2 but not v1
    pub added: Vec<FieldChange>,
    /// Fields present in v1 but not v2
    pub removed: Vec<FieldChange>,
    /// Fields present in both but with different values
    pub modified: Vec<FieldChange>,
}

/// Request body for POST /api/contracts/:id/rollback/:snapshot_id
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RollbackRequest {
    /// Stellar address (or admin service ID) authorising the rollback
    pub changed_by: String,
}

// Multisig deployment types
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct MultisigPolicy {
    pub id: Uuid,
    pub name: String,
    pub threshold: i32,
    pub required_signatures: i32,
    pub signer_addresses: Vec<String>,
    pub expiry_seconds: i32,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct DeployProposal {
    pub id: Uuid,
    pub contract_name: String,
    pub contract_id: Uuid,
    pub wasm_hash: String,
    pub network: String,
    pub description: Option<String>,
    pub policy_id: Uuid,
    pub status: String,
    pub expires_at: DateTime<Utc>,
    pub proposer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ProposalSignature {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub signer_address: String,
}

/// Paginated response for audit log
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ProposalWithSignatures {
    pub proposal: DeployProposal,
    pub policy: MultisigPolicy,
    pub signatures: Vec<ProposalSignature>,
    pub signatures_needed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AuditLogPage {
    pub items: Vec<ContractAuditLog>,
    pub total: i64,
    pub page: i64,
    pub total_pages: i64,
}

// ────────────────────────────────────────────────────────────────────────────
// Cursor-based pagination  (issue #337)
// Used by activity-feed and any future cursor-paginated endpoints.
// PaginatedResponse (offset-based) above is left completely untouched.
// ────────────────────────────────────────────────────────────────────────────

/// Query parameters for the activity feed endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityFeedParams {
    /// ISO-8601 timestamp. Only events older than this are returned.
    /// Omit on the first request; use `next_cursor` from the previous
    /// response on subsequent requests.
    pub cursor: Option<DateTime<Utc>>,

    /// How many events to return (default 20, max 100).
    #[serde(default = "default_activity_limit")]
    pub limit: i64,

    /// Optionally filter by event type.
    pub event_type: Option<AnalyticsEventType>,
}

fn default_activity_limit() -> i64 {
    20
}

/// Response for cursor-paginated endpoints.
/// `next_cursor` is `None` when `has_more` is false (last page).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    /// Real total matching the applied filters — from COUNT(*).
    pub total: i64,
    /// True when more results exist beyond this page.
    pub has_more: bool,
    /// Pass this value as `cursor` to fetch the next page.
    /// `None` on the last page.
    pub next_cursor: Option<DateTime<Utc>>,
}

impl<T: Serialize> CursorPaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, limit: i64, next_cursor: Option<DateTime<Utc>>) -> Self {
        let has_more = data.len() as i64 == limit;
        Self {
            has_more,
            // Only emit a cursor when there really are more pages.
            next_cursor: if has_more { next_cursor } else { None },
            data,
            total,
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Config Management types
// ════════════════════════════════════════════════════════════════════════════

/// Represents a contract configuration version in the registry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
pub struct ContractConfig {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub environment: String,
    pub version: i32,
    pub config_data: serde_json::Value,
    pub secrets_data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

/// Request to create a new configuration version
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConfigCreateRequest {
    pub environment: String,
    pub config_data: serde_json::Value,
    pub secrets_data: Option<serde_json::Value>,
    pub created_by: String,
}

/// Request to rollback to an old configuration version
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConfigRollbackRequest {
    pub roll_back_to_version: i32,
    pub created_by: String,
}

/// Response object for returning configurations (without secrets_data when returning publicly)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractConfigResponse {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub environment: String,
    pub version: i32,
    pub config_data: serde_json::Value,
    pub has_secrets: bool, // Indicator instead of returning actual secrets
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

impl From<ContractConfig> for ContractConfigResponse {
    fn from(config: ContractConfig) -> Self {
        Self {
            id: config.id,
            contract_id: config.contract_id,
            environment: config.environment,
            version: config.version,
            config_data: config.config_data,
            has_secrets: config.secrets_data.is_some(),
            created_at: config.created_at,
            created_by: config.created_by,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DATA RESIDENCY CONTROLS  (issue #100)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema)]
#[sqlx(type_name = "residency_decision", rename_all = "lowercase")]
pub enum ResidencyDecision {
    Allowed,
    Denied,
}

impl std::fmt::Display for ResidencyDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allowed => write!(f, "allowed"),
            Self::Denied => write!(f, "denied"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResidencyPolicy {
    pub id: Uuid,
    pub contract_id: String,
    pub allowed_regions: Vec<String>,
    pub description: Option<String>,
    pub is_active: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResidencyAuditLog {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub contract_id: String,
    pub requested_region: String,
    pub decision: ResidencyDecision,
    pub action: String,
    pub requested_by: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ResidencyViolation {
    pub id: Uuid,
    pub policy_id: Uuid,
    pub contract_id: String,
    pub attempted_region: String,
    pub action: String,
    pub attempted_by: Option<String>,
    pub prevented_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateResidencyPolicyRequest {
    pub contract_id: String,
    pub allowed_regions: Vec<String>,
    pub description: Option<String>,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResidencyPolicyRequest {
    pub allowed_regions: Option<Vec<String>>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResidencyRequest {
    pub policy_id: Uuid,
    pub contract_id: String,
    pub requested_region: String,
    pub action: String,
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResidencyLogsParams {
    pub contract_id: Option<String>,
    pub limit: Option<i64>,
    pub page: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTRACT EVENT TYPES (issue #44)
// ═══════════════════════════════════════════════════════════════════════════

/// A contract event emitted during execution
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContractEvent {
    pub id: Uuid,
    pub contract_id: String,
    pub topic: String,
    pub data: Option<serde_json::Value>,
    pub ledger_sequence: i64,
    pub transaction_hash: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub network: Network,
    pub created_at: DateTime<Utc>,
}

/// Query parameters for searching events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventQueryParams {
    pub topic: Option<String>,
    pub data_pattern: Option<String>,
    pub from_timestamp: Option<DateTime<Utc>>,
    pub to_timestamp: Option<DateTime<Utc>>,
    pub from_ledger: Option<i64>,
    pub to_ledger: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Request to index a new event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEventRequest {
    pub contract_id: String,
    pub topic: String,
    pub data: Option<serde_json::Value>,
    pub ledger_sequence: i64,
    pub transaction_hash: Option<String>,
    pub network: Network,
}

/// Event statistics for a contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStats {
    pub contract_id: String,
    pub total_events: i64,
    pub unique_topics: i64,
    pub first_event: Option<DateTime<Utc>>,
    pub last_event: Option<DateTime<Utc>>,
    pub events_by_topic: serde_json::Value,
}

/// CSV export response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventExport {
    pub contract_id: String,
    pub events: Vec<ContractEvent>,
    pub exported_at: DateTime<Utc>,
    pub total_count: i64,
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTRACT PACKAGE SIGNING (Issue #67)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "signature_status", rename_all = "lowercase")]
pub enum SignatureStatus {
    Valid,
    Revoked,
    Expired,
}

impl std::fmt::Display for SignatureStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "valid"),
            Self::Revoked => write!(f, "revoked"),
            Self::Expired => write!(f, "expired"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "transparency_entry_type", rename_all = "snake_case")]
pub enum TransparencyEntryType {
    PackageSigned,
    SignatureVerified,
    SignatureRevoked,
    KeyRotated,
}

impl std::fmt::Display for TransparencyEntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PackageSigned => write!(f, "package_signed"),
            Self::SignatureVerified => write!(f, "signature_verified"),
            Self::SignatureRevoked => write!(f, "signature_revoked"),
            Self::KeyRotated => write!(f, "key_rotated"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PackageSignature {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub version: String,
    pub wasm_hash: String,
    pub signature: String,
    pub signing_address: String,
    pub public_key: String,
    pub algorithm: String,
    pub status: SignatureStatus,
    pub signed_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub revoked_by: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignPackageRequest {
    pub contract_id: String,
    pub version: String,
    pub wasm_hash: String,
    pub private_key: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySignatureRequest {
    pub contract_id: String,
    pub version: String,
    pub wasm_hash: String,
    pub signature: String,
    pub signing_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySignatureResponse {
    pub valid: bool,
    pub signature_id: Option<Uuid>,
    pub signing_address: String,
    pub signed_at: Option<DateTime<Utc>>,
    pub status: SignatureStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeSignatureRequest {
    pub signature_id: String,
    pub revoked_by: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SignatureRevocation {
    pub id: Uuid,
    pub signature_id: Uuid,
    pub revoked_by: String,
    pub reason: String,
    pub revoked_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SigningKey {
    pub id: Uuid,
    pub publisher_id: Uuid,
    pub public_key: String,
    pub key_fingerprint: String,
    pub algorithm: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSigningKeyRequest {
    pub publisher_id: String,
    pub public_key: String,
    pub algorithm: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TransparencyLogEntry {
    pub id: Uuid,
    pub entry_type: TransparencyEntryType,
    pub contract_id: Option<Uuid>,
    pub signature_id: Option<Uuid>,
    pub actor_address: String,
    pub previous_hash: Option<String>,
    pub entry_hash: String,
    pub payload: Option<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
    pub immutable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainOfCustodyEntry {
    pub action: String,
    pub actor: String,
    pub timestamp: DateTime<Utc>,
    pub signature_id: Option<Uuid>,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainOfCustodyResponse {
    pub contract_id: String,
    pub entries: Vec<ChainOfCustodyEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparencyLogQueryParams {
    pub contract_id: Option<String>,
    pub entry_type: Option<TransparencyEntryType>,
    pub actor_address: Option<String>,
    pub from_timestamp: Option<DateTime<Utc>>,
    pub to_timestamp: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Warning,
    Critical,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Warning => write!(f, "warning"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

impl std::str::FromStr for HealthStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "healthy" => Ok(Self::Healthy),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            other => Err(format!("unknown health status: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContractHealth {
    pub contract_id: Uuid,
    pub status: String,
    pub last_activity: DateTime<Utc>,
    pub security_score: i32,
    pub audit_date: Option<DateTime<Utc>>,
    pub total_score: i32,
    pub recommendations: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

impl ContractHealth {
    /// Returns the typed `HealthStatus` from the stored string.
    pub fn health_status(&self) -> Result<HealthStatus, String> {
        self.status.parse()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ADVANCED CONTRACT DEPENDENCIES (issue #417)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    pub contract_id: String,
    pub resolved_id: Option<Uuid>,
    pub name: Option<String>,
    pub call_volume: i32,
    pub status: String,
    pub is_circular: bool,
    pub dependencies: Vec<DependencyNode>,
    pub visualization_hints: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyResponse {
    pub root: DependencyNode,
    pub total_dependencies: usize,
    pub max_depth: usize,
    pub has_circular: bool,
}

// Backup and disaster recovery types
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ContractBackup {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub backup_date: chrono::NaiveDate,
    pub wasm_hash: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub state_snapshot: Option<serde_json::Value>,
    pub storage_size_bytes: Option<i64>,
    pub primary_region: Option<String>,
    pub backup_regions: Option<Vec<String>>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BackupRestoration {
    pub id: Uuid,
    pub backup_id: Uuid,
    pub restored_by: Uuid,
    pub restore_duration_ms: i32,
    pub success: bool,
    pub restored_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBackupRequest {
    pub include_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreBackupRequest {
    pub backup_date: String,
}

/// AUTOMATED REALEASE NOTE GENERATOR
/// Status of auto-generated release notes (draft allows editing before publish)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "release_notes_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ReleaseNotesStatus {
    Draft,
    Published,
}

impl std::fmt::Display for ReleaseNotesStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseNotesStatus::Draft => write!(f, "draft"),
            ReleaseNotesStatus::Published => write!(f, "published"),
        }
    }
}

/// A detected function change in a code diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionChange {
    pub name: String,
    pub change_type: String,
    pub old_signature: Option<String>,
    pub new_signature: Option<String>,
    pub is_breaking: bool,
}

/// Summary of a code diff between two contract versions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffSummary {
    pub files_changed: i32,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub function_changes: Vec<FunctionChange>,
    pub has_breaking_changes: bool,
    pub features_count: i32,
    pub fixes_count: i32,
    pub breaking_count: i32,
}

/// Stored release notes generation record
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ReleaseNotesGenerated {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub version: String,
    pub previous_version: Option<String>,
    pub diff_summary: serde_json::Value,
    pub changelog_entry: Option<String>,
    pub notes_text: String,
    pub status: ReleaseNotesStatus,
    pub generated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

/// Request to auto-generate release notes for a contract version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateReleaseNotesRequest {
    pub version: String,
    pub previous_version: Option<String>,
    pub source_url: Option<String>,
    pub changelog_content: Option<String>,
    pub contract_address: Option<String>,
}

/// Request to manually edit release notes before publishing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateReleaseNotesRequest {
    pub notes_text: String,
}

/// Request to publish (finalize) release notes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishReleaseNotesRequest {
    #[serde(default = "default_true")]
    pub update_version_record: bool,
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTRACT DEPLOYMENT SIMULATION (Issue #256)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulateDeployRequest {
    pub wasm_binary: String,
    pub contract_id: String,
    pub name: String,
    pub description: Option<String>,
    pub network: Network,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub publisher_address: String,
    #[serde(default)]
    pub dependencies: Vec<DependencyDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub valid: bool,
    pub errors: Vec<SimulationError>,
    pub warnings: Vec<SimulationWarning>,
    pub gas_estimate: GasEstimate,
    pub performance_metrics: PerformanceMetrics,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abi_preview: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_functions: Option<Vec<ContractFunctionInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationWarning {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasEstimate {
    pub total_cost_stroops: i64,
    pub total_cost_xlm: f64,
    pub wasm_size_kb: f64,
    pub complexity_factor: f64,
    pub deployment_cost_stroops: i64,
    pub storage_cost_stroops: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub estimated_execution_time_ms: u64,
    pub memory_estimate_kb: u64,
    pub function_count: u32,
    pub table_size_bytes: u32,
    pub data_section_bytes: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractFunctionInfo {
    pub name: String,
    pub param_count: u32,
    pub return_type: Option<String>,
    pub is_view: bool,
}

fn default_true() -> bool {
    true
}

/// Full response for generated release notes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseNotesResponse {
    pub id: Uuid,
    pub contract_id: Uuid,
    pub version: String,
    pub previous_version: Option<String>,
    pub diff_summary: DiffSummary,
    pub changelog_entry: Option<String>,
    pub notes_text: String,
    pub status: ReleaseNotesStatus,
    pub generated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
}

// ────────────────────────────────────────────────────────────────────────────
// Contract changelog (release history)
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractChangelogEntry {
    pub version: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_notes: Option<String>,
    pub breaking: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaking_changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractChangelogResponse {
    pub contract_id: Uuid,
    pub entries: Vec<ContractChangelogEntry>,
}

// ═══════════════════════════════════════════════════════════════════════════
// ANALYTICS DASHBOARD (issue #430)
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct CategoryCount {
    pub category: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct NetworkCount {
    pub network: Network,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeploymentTrend {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DashboardAnalyticsResponse {
    pub category_distribution: Vec<CategoryCount>,
    pub network_usage: Vec<NetworkCount>,
    pub deployment_trends: Vec<DeploymentTrend>,
    pub recent_additions: Vec<Contract>,
}

// ═══════════════════════════════════════════════════════════════════════════
// CONTRACT REVIEW SYSTEM (Issue: Review System Implementation)
// ═══════════════════════════════════════════════════════════════════════════

/// Review status for moderation workflow
/// New reviews start as "pending" and must be approved before becoming visible
#[derive(
    Debug, Clone, Serialize, Deserialize, sqlx::Type, utoipa::ToSchema, PartialEq, Default,
)]
#[sqlx(type_name = "review_status", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ReviewStatus {
    /// Review is pending approval (default for new reviews)
    #[default]
    Pending,
    /// Review has been approved and is visible to users
    Approved,
    /// Review has been rejected and is hidden
    Rejected,
}

/// Sort options for fetching reviews
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSortBy {
    /// Sort by helpful votes (highest first)
    MostHelpful,
    /// Sort by creation date (newest first)
    MostRecent,
    /// Sort by rating (highest first)
    HighestRated,
    /// Sort by rating (lowest first)
    LowestRated,
}

/// Request to create a new contract review
/// Rating must be between 1.0 and 5.0 (inclusive)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CreateReviewRequest {
    /// Rating from 1.0 to 5.0 (one decimal place allowed)
    #[schema(example = 4.5, minimum = 1.0, maximum = 5.0)]
    pub rating: f64,
    /// Optional review text
    #[schema(example = "Great contract! Very well optimized and easy to integrate.")]
    pub review_text: Option<String>,
    /// Contract version being reviewed (optional)
    #[schema(example = "1.0.0")]
    pub version: Option<String>,
}

/// Review response with full details
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ReviewResponse {
    pub id: i32,
    pub contract_id: Uuid,
    pub user_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[schema(example = 4.5, minimum = 1.0, maximum = 5.0)]
    pub rating: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_text: Option<String>,
    #[serde(default)]
    pub helpful_count: i32,
    #[serde(default)]
    pub is_flagged: bool,
    #[serde(default)]
    pub status: ReviewStatus,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Aggregated rating statistics for a contract
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ContractRatingStats {
    /// Average rating (0.0 if no reviews)
    #[schema(example = 4.3)]
    pub average_rating: f64,
    /// Total number of approved reviews
    #[schema(example = 42)]
    pub total_reviews: i64,
    /// Distribution of ratings (1-5 stars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating_distribution: Option<RatingDistribution>,
}

/// Distribution of ratings by star count
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RatingDistribution {
    /// Number of 1-star reviews
    #[schema(example = 2)]
    pub stars_1: i64,
    /// Number of 2-star reviews
    #[schema(example = 3)]
    pub stars_2: i64,
    /// Number of 3-star reviews
    #[schema(example = 5)]
    pub stars_3: i64,
    /// Number of 4-star reviews
    #[schema(example = 12)]
    pub stars_4: i64,
    /// Number of 5-star reviews
    #[schema(example = 20)]
    pub stars_5: i64,
}

/// Request to vote on review helpfulness
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReviewVoteRequest {
    /// true = helpful, false = unhelpful
    pub helpful: bool,
}

/// Request to flag a review for moderation
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FlagReviewRequest {
    /// Reason for flagging the review
    #[schema(example = "Spam or misleading content")]
    pub reason: String,
}

/// Request to approve or reject a review (moderation endpoint)
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ModerateReviewRequest {
    /// Action to take: "approve" or "reject"
    #[schema(example = "approve")]
    pub action: String,
}

/// Query parameters for GET /contracts/:id/reviews
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::IntoParams)]
pub struct GetReviewsQuery {
    /// Sort order: most_helpful, most_recent, highest_rated, lowest_rated
    #[serde(default = "default_review_sort")]
    pub sort_by: ReviewSortBy,
    /// Maximum number of reviews to return (default: 20, max: 100)
    #[serde(default = "default_review_limit")]
    pub limit: i64,
    /// Offset for pagination
    #[serde(default)]
    pub offset: i64,
}

fn default_review_sort() -> ReviewSortBy {
    ReviewSortBy::MostRecent
}

fn default_review_limit() -> i64 {
    20
}

/// Response for review voting endpoint
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReviewVoteResponse {
    pub review_id: i32,
    pub helpful_count: i32,
    pub vote_recorded: bool,
}
