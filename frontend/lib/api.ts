// Mock data: conditionally imported only in development/test.
// In production (NEXT_PUBLIC_USE_MOCKS !== "true"), these are empty stubs
// that never get reached (gated behind USE_MOCKS checks below).
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let MOCK_CONTRACTS: any[] = [];
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let MOCK_EXAMPLES: Record<string, any[]> = {};
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let MOCK_VERSIONS: Record<string, any[]> = {};
if (process.env.NEXT_PUBLIC_USE_MOCKS === "true") {
  // Dynamic require ensures Next.js tree-shakes mock-data from production bundles
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const mocks = require("./mock-data");
  MOCK_CONTRACTS = mocks.MOCK_CONTRACTS;
  MOCK_EXAMPLES = mocks.MOCK_EXAMPLES;
  MOCK_VERSIONS = mocks.MOCK_VERSIONS;
}
import { trackEvent } from "./analytics";
import {
  ApiError,
  NetworkError,
  extractErrorData,
  createApiError,
} from "./errors";

export type Network = "mainnet" | "testnet" | "futurenet";

export type NetworkStatus = "online" | "offline" | "degraded";

export interface NetworkEndpoints {
  rpc_url: string;
  health_url: string;
  explorer_url: string;
  friendbot_url?: string;
}

export interface NetworkInfo {
  id: string;
  name: string;
  network_type: Network;
  status: NetworkStatus;
  endpoints: NetworkEndpoints;
  last_checked_at: string;
  last_indexed_ledger_height?: number;
  last_indexed_at?: string;
  consecutive_failures: number;
  status_message?: string;
}

export interface NetworkListResponse {
  networks: NetworkInfo[];
  cached_at: string;
}

/** Per-network config (Issue #43) */
export interface NetworkConfig {
  contract_id: string;
  is_verified: boolean;
  min_version?: string;
  max_version?: string;
}

export interface Contract {
  id: string;
  contract_id: string;
  wasm_hash: string;
  name: string;
  description?: string;
  publisher_id: string;
  network: Network;
  is_verified: boolean;
  category?: string;
  tags: string[];
  popularity_score?: number;
  downloads?: number;
  // Image fields for contract logo/icon
  logo_url?: string;
  created_at: string;
  updated_at: string;
  verified_at?: string;
  last_accessed_at?: string;
  is_maintenance?: boolean;
  /** Logical contract grouping (Issue #43) */
  logical_id?: string;
  /** Per-network configs: { mainnet: {...}, testnet: {...} } */
  network_configs?: Record<Network, NetworkConfig>;
}

/** GET /contracts/:id response when ?network= is used (Issue #43) */
export interface ContractGetResponse extends Contract {
  current_network?: Network;
  network_config?: NetworkConfig;
}

export interface ContractHealth {
  contract_id: string;
  status: "healthy" | "warning" | "critical";
  last_activity: string;
  security_score: number;
  audit_date?: string;
  total_score: number;
  recommendations: string[];
  updated_at: string;
}

export interface ContractInteractionResponse {
  id: string;
  account: string | null;
  method: string | null;
  parameters: unknown;
  return_value: unknown;
  transaction_hash: string | null;
  created_at: string;
}

export interface InteractionsQueryParams {
  limit?: number;
  offset?: number;
  account?: string;
  method?: string;
  from_timestamp?: string;
  to_timestamp?: string;
}

export interface InteractionsListResponse {
  items: ContractInteractionResponse[];
  total: number;
  limit: number;
  offset: number;
}

/** Analytics timeline entry (one day) */
export interface TimelineEntry {
  date: string;
  count: number;
}

export interface TopUser {
  address: string;
  count: number;
}

export interface InteractorStats {
  unique_count: number;
  top_users: TopUser[];
}

export interface DeploymentStats {
  count: number;
  unique_users: number;
  by_network: Record<string, number>;
}

export interface ContractAnalyticsResponse {
  contract_id: string;
  deployments: DeploymentStats;
  interactors: InteractorStats;
  timeline: TimelineEntry[];
}

export interface ContractVersion {
  id: string;
  contract_id: string;
  version: string;
  wasm_hash: string;
  source_url?: string;
  commit_hash?: string;
  release_notes?: string;
  created_at: string;
}

export interface Publisher {
  id: string;
  stellar_address: string;
  username?: string;
  email?: string;
  github_url?: string;
  website?: string;
  // Image fields for publisher avatar
  avatar_url?: string;
  created_at: string;
}

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
  total_pages: number;
}

export interface DependencyTreeNode {
  contract_id: string;
  name: string;
  current_version: string;
  constraint_to_parent: string;
  dependencies: DependencyTreeNode[];
}

export interface MaintenanceWindow {
  message: string;
  scheduled_end_at?: string;
}

export type MaturityLevel = 'alpha' | 'beta' | 'stable' | 'mature' | 'legacy';

export interface ContractSearchParams {
  query?: string;
  network?: "mainnet" | "testnet" | "futurenet";
  networks?: Array<"mainnet" | "testnet" | "futurenet">;
  verified_only?: boolean;
  category?: string;
  categories?: string[];
  language?: string;
  languages?: string[];
  author?: string;
  tags?: string[];
  maturity?: 'alpha' | 'beta' | 'stable' | 'mature' | 'legacy';
  page?: number;
  page_size?: number;
  sort_by?: 'name' | 'created_at' | 'updated_at' | 'popularity' | 'deployments' | 'interactions' | 'relevance' | 'downloads';
  sort_order?: 'asc' | 'desc';
}

export interface PublishRequest {
  contract_id: string;
  name: string;
  description?: string;
  network: "mainnet" | "testnet" | "futurenet";
  category?: string;
  tags: string[];
  source_url?: string;
  publisher_address: string;
}

export type CustomMetricType = 'counter' | 'gauge' | 'histogram';

export interface MetricCatalogEntry {
  metric_name: string;
  metric_type: CustomMetricType;
  last_seen: string;
  sample_count: number;
}

export interface MetricSeriesPoint {
  bucket_start: string;
  bucket_end: string;
  sample_count: number;
  sum_value?: number;
  avg_value?: number;
  min_value?: number;
  max_value?: number;
  p50_value?: number;
  p95_value?: number;
  p99_value?: number;
}

export interface MetricSample {
  timestamp: string;
  value: number;
  unit?: string;
  metadata?: Record<string, unknown> | null;
}

export interface MetricSeriesResponse {
  contract_id: string;
  metric_name: string;
  metric_type: CustomMetricType | null;
  resolution: 'hour' | 'day' | 'raw';
  points?: MetricSeriesPoint[];
  samples?: MetricSample[];
}

export type DeprecationStatus = 'active' | 'deprecated' | 'retired';

export type ReleaseNotesStatus = 'draft' | 'published';

export interface FunctionChange {
  name: string;
  change_type: 'added' | 'removed' | 'modified';
  old_signature?: string;
  new_signature?: string;
  is_breaking: boolean;
}

export interface DiffSummary {
  files_changed: number;
  lines_added: number;
  lines_removed: number;
  function_changes: FunctionChange[];
  has_breaking_changes: boolean;
  features_count: number;
  fixes_count: number;
  breaking_count: number;
}

export interface ReleaseNotesResponse {
  id: string;
  contract_id: string;
  version: string;
  previous_version?: string;
  diff_summary: DiffSummary;
  changelog_entry?: string;
  notes_text: string;
  status: ReleaseNotesStatus;
  generated_by: string;
  created_at: string;
  updated_at: string;
  published_at?: string;
}

export interface GenerateReleaseNotesRequest {
  version: string;
  previous_version?: string;
  source_url?: string;
  changelog_content?: string;
  contract_address?: string;
}

export interface UpdateReleaseNotesRequest {
  notes_text: string;
}

export interface PublishReleaseNotesRequest {
  update_version_record?: boolean;
}

export interface DeprecationInfo {
  contract_id: string;
  status: DeprecationStatus;
  deprecated_at?: string | null;
  retirement_at?: string | null;
  replacement_contract_id?: string | null;
  migration_guide_url?: string | null;
  notes?: string | null;
  days_remaining?: number | null;
  dependents_notified: number;
}

const API_URL = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3001";
const USE_MOCKS = process.env.NEXT_PUBLIC_USE_MOCKS === "true";

/**
 * Wrapper for API calls with consistent error handling
 */
async function handleApiCall<T>(
  apiCall: () => Promise<Response>,
  endpoint: string
): Promise<T> {
  try {
    const response = await apiCall();
    
    if (!response.ok) {
      const errorData = await extractErrorData(response);
      throw createApiError(response.status, errorData, endpoint);
    }
    
    try {
      return await response.json();
    } catch (parseError) {
      throw new ApiError(
        'Failed to parse server response',
        response.status,
        parseError,
        endpoint
      );
    }
  } catch (error) {
    // Re-throw if already an ApiError
    if (error instanceof ApiError) {
      throw error;
    }
    
    // Handle network errors
    if (error instanceof TypeError) {
      const message = error.message.toLowerCase();
      if (message.includes('fetch') || message.includes('network') || message.includes('failed to fetch')) {
        throw new NetworkError(
          'Unable to connect to the server. Please check your internet connection.',
          endpoint
        );
      }
    }
    
    // Handle timeout errors
    if (error instanceof Error && error.name === 'AbortError') {
      throw new NetworkError('The request timed out. Please try again.', endpoint);
    }
    
    // Unknown error
    throw new ApiError('An unexpected error occurred', undefined, error, endpoint);
  }
}

export const api = {
  async getNetworks(): Promise<NetworkListResponse> {
    if (USE_MOCKS) {
      const now = new Date().toISOString();
      return {
        cached_at: now,
        networks: [
          {
            id: "mainnet",
            name: "Stellar Mainnet",
            network_type: "mainnet",
            status: "online",
            endpoints: {
              rpc_url: "https://rpc-mainnet.stellar.org",
              health_url: "https://rpc-mainnet.stellar.org/health",
              explorer_url: "https://stellar.expert/explorer/public",
            },
            last_checked_at: now,
            consecutive_failures: 0,
          },
          {
            id: "testnet",
            name: "Stellar Testnet",
            network_type: "testnet",
            status: "online",
            endpoints: {
              rpc_url: "https://rpc-testnet.stellar.org",
              health_url: "https://rpc-testnet.stellar.org/health",
              explorer_url: "https://stellar.expert/explorer/testnet",
              friendbot_url: "https://friendbot.stellar.org",
            },
            last_checked_at: now,
            consecutive_failures: 0,
          },
          {
            id: "futurenet",
            name: "Stellar Futurenet",
            network_type: "futurenet",
            status: "online",
            endpoints: {
              rpc_url: "https://rpc-futurenet.stellar.org",
              health_url: "https://rpc-futurenet.stellar.org/health",
              explorer_url: "https://stellar.expert/explorer/futurenet",
              friendbot_url: "https://friendbot-futurenet.stellar.org",
            },
            last_checked_at: now,
            consecutive_failures: 0,
          },
        ],
      };
    }

    return handleApiCall<NetworkListResponse>(
      () => fetch(`${API_URL}/networks`),
      "/networks",
    );
  },

  // Contract endpoints
  async getContracts(
    params?: ContractSearchParams,
  ): Promise<PaginatedResponse<Contract>> {
    if (USE_MOCKS) {
      return new Promise((resolve) => {
        setTimeout(() => {
          let filtered = [...MOCK_CONTRACTS];

          if (params?.query) {
            const q = params.query.toLowerCase();
            filtered = filtered.filter(
              (c) =>
                c.name.toLowerCase().includes(q) ||
                (c.description && c.description.toLowerCase().includes(q)) ||
                c.tags.some((tag: string) => tag.toLowerCase().includes(q)),
            );
          }

          const categories = params?.categories?.length
            ? params.categories
            : params?.category
              ? [params.category]
              : [];
          if (categories.length > 0) {
            filtered = filtered.filter(
              (c) => c.category && categories.includes(c.category),
            );
          }

          const networks = params?.networks?.length
            ? params.networks
            : params?.network
              ? [params.network]
              : [];
          if (networks.length > 0) {
            filtered = filtered.filter((c) => networks.includes(c.network));
          }

          const languages = params?.languages?.length
            ? params.languages
            : params?.language
              ? [params.language]
              : [];
          if (languages.length > 0) {
            const normalized = languages.map((language) => language.toLowerCase());
            filtered = filtered.filter((c) =>
              c.tags.some((tag: string) => normalized.includes(tag.toLowerCase())),
            );
          }

          if (params?.author) {
            const author = params.author.toLowerCase();
            filtered = filtered.filter((c) =>
              c.publisher_id.toLowerCase().includes(author),
            );
          }

          if (params?.verified_only) {
            filtered = filtered.filter((c) => c.is_verified);
          }

          const sortBy = params?.sort_by || "created_at";
          const sortOrder = params?.sort_order || "desc";
          filtered.sort((a, b) => {
            if (sortBy === "name") {
              return a.name.localeCompare(b.name);
            }
            if (sortBy === "popularity") {
              const aPopularity = a.popularity_score ?? 0;
              const bPopularity = b.popularity_score ?? 0;
              return aPopularity - bPopularity;
            }
            if (sortBy === "downloads") {
              const aDownloads = a.downloads ?? 0;
              const bDownloads = b.downloads ?? 0;
              return aDownloads - bDownloads;
            }
            return (
              new Date(a.created_at).getTime() - new Date(b.created_at).getTime()
            );
          });
          if (sortOrder === "desc") {
            filtered.reverse();
          }

          const page = params?.page || 1;
          const pageSize = params?.page_size || 20;
          const start = (page - 1) * pageSize;
          const end = start + pageSize;
          const items = filtered.slice(start, end);

          resolve({
            items,
            total: filtered.length,
            page,
            page_size: pageSize,
            total_pages: Math.max(1, Math.ceil(filtered.length / pageSize)),
          });
        }, 500);
      });
    }

    const queryParams = new URLSearchParams();
    if (params?.query) queryParams.append("query", params.query);
    if (params?.network) queryParams.append("network", params.network);
    params?.networks?.forEach((network) => queryParams.append("network", network));
    if (params?.verified_only !== undefined)
      queryParams.append("verified_only", String(params.verified_only));
    if (params?.category) queryParams.append("category", params.category);
    params?.categories?.forEach((category) =>
      queryParams.append("category", category),
    );
    if (params?.language) queryParams.append("language", params.language);
    params?.languages?.forEach((language) =>
      queryParams.append("language", language),
    );
    if (params?.author) queryParams.append("author", params.author);
    params?.tags?.forEach((tag) => queryParams.append("tag", tag));
    // Backend expects sort_by without underscores: createdat, updatedat, popularity, deployments, interactions, relevance
    if (params?.sort_by) {
      const backendSortBy =
        params.sort_by === 'created_at' ? 'createdat'
        : params.sort_by === 'updated_at' ? 'updatedat'
        : params.sort_by === 'name' ? 'name'
        : params.sort_by === 'downloads' ? 'interactions'
        : params.sort_by;
      queryParams.append("sort_by", backendSortBy);
    }
    if (params?.sort_order) queryParams.append("sort_order", params.sort_order);
    if (params?.page) queryParams.append("page", String(params.page));
    if (params?.page_size)
      queryParams.append("page_size", String(params.page_size));

    const data = await handleApiCall<PaginatedResponse<Contract>>(
      () => fetch(`${API_URL}/api/contracts?${queryParams}`),
      '/api/contracts'
    );
    // Backend may return "contracts" instead of "items" — normalize for PaginatedResponse
    const raw = data as unknown as Record<string, unknown>;
    if (Array.isArray(raw.contracts) && data.items === undefined) {
      return { ...data, items: raw.contracts as Contract[] };
    }
    return data;
  },

  async getContract(id: string, network?: Network): Promise<ContractGetResponse> {
    if (USE_MOCKS) {
      return new Promise((resolve, reject) => {
        setTimeout(() => {
          const contract = MOCK_CONTRACTS.find(
            (c) => c.id === id || c.contract_id === id,
          );
          if (contract) {
            resolve(contract as ContractGetResponse);
          } else {
            reject(new Error("Contract not found"));
          }
        }, 300);
      });
    }


    return handleApiCall<Contract>(
      () => {
        const url = new URL(`${API_URL}/api/contracts/${id}`);
        if (network != null) url.searchParams.set("network", String(network));
        return fetch(url.toString());
      },
      `/api/contracts/${id}`
    );
  },

  async getContractExamples(id: string): Promise<ContractExample[]> {
    if (USE_MOCKS) {
      return new Promise((resolve) => {
        setTimeout(() => {
          resolve(MOCK_EXAMPLES[id] || []);
        }, 500);
      });
    }

    return handleApiCall<ContractExample[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/examples`),
      `/api/contracts/${id}/examples`
    );
  },

  async rateExample(
    id: string,
    userAddress: string,
    rating: number,
  ): Promise<ExampleRating> {
    if (USE_MOCKS) {
      return new Promise((resolve) => {
        setTimeout(() => {
          resolve({
            id: "mock-rating-id",
            example_id: id,
            user_address: userAddress,
            rating: rating,
            created_at: new Date().toISOString(),
          });
        }, 300);
      });
    }

    return handleApiCall<ExampleRating>(
      () => fetch(`${API_URL}/api/examples/${id}/rate`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ user_address: userAddress, rating }),
      }),
      `/api/examples/${id}/rate`
    );
  },

  async getContractVersions(id: string): Promise<ContractVersion[]> {
    if (USE_MOCKS) {
      return new Promise((resolve) => {
        setTimeout(() => {
          resolve(MOCK_VERSIONS[id] || []);
        }, 300);
      });
    }

    return handleApiCall<ContractVersion[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/versions`),
      `/api/contracts/${id}/versions`
    );
  },

  async getContractDependencies(id: string): Promise<DependencyTreeNode[]> {
    return handleApiCall<DependencyTreeNode[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/dependencies`),
      `/api/contracts/${id}/dependencies`
    );
  },

  async getContractInteractions(
    id: string,
    params?: InteractionsQueryParams,
  ): Promise<InteractionsListResponse> {
    const search = new URLSearchParams();
    if (params?.limit != null) search.set("limit", String(params.limit));
    if (params?.offset != null) search.set("offset", String(params.offset));
    if (params?.account) search.set("account", params.account);
    if (params?.method) search.set("method", params.method);
    if (params?.from_timestamp) search.set("from_timestamp", params.from_timestamp);
    if (params?.to_timestamp) search.set("to_timestamp", params.to_timestamp);
    const qs = search.toString();
    const response = await fetch(
      `${API_URL}/api/contracts/${id}/interactions${qs ? `?${qs}` : ""}`,
    );
    if (!response.ok) throw new Error("Failed to fetch contract interactions");
    return response.json();
  },

  async getContractAnalytics(id: string): Promise<ContractAnalyticsResponse> {
    const response = await fetch(`${API_URL}/api/contracts/${id}/analytics`);
    if (!response.ok) throw new Error("Failed to fetch contract analytics");
    return response.json();
  },

  async publishContract(data: PublishRequest): Promise<Contract> {
    if (USE_MOCKS) {
      if (typeof window !== "undefined") {
        trackEvent("contract_publish_failed", {
          network: data.network,
          name: data.name,
          reason: "mock_mode_not_supported",
        });
      }
      throw new Error("Publishing is not supported in mock mode");
    }

    try {
      const response = await fetch(`${API_URL}/api/contracts`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      });
      if (!response.ok) throw new Error("Failed to publish contract");

      const published = await response.json();
      if (typeof window !== "undefined") {
        trackEvent("contract_published", {
          contract_id: data.contract_id,
          name: data.name,
          network: data.network,
          category: data.category,
        });
      }

      return published;
    } catch (error) {
      if (typeof window !== "undefined") {
        trackEvent("contract_publish_failed", {
          contract_id: data.contract_id,
          name: data.name,
          network: data.network,
        });
      }
      throw error;
    }
  },

  async getContractHealth(id: string): Promise<ContractHealth> {
    return handleApiCall<ContractHealth>(
      () => fetch(`${API_URL}/api/contracts/${id}/health`),
      `/api/contracts/${id}/health`
    );
  },

  async getDeprecationInfo(id: string): Promise<DeprecationInfo> {
    if (USE_MOCKS) {
      return Promise.resolve({
        contract_id: id,
        status: 'deprecated',
        deprecated_at: new Date(Date.now() - 86400000 * 7).toISOString(),
        retirement_at: new Date(Date.now() + 86400000 * 30).toISOString(),
        replacement_contract_id: 'c2',
        migration_guide_url: 'https://example.com/migration',
        notes: 'This contract is being retired. Migrate to the new liquidity pool contract.',
        days_remaining: 30,
        dependents_notified: 4,
      });
    }

    return handleApiCall<DeprecationInfo>(
      () => fetch(`${API_URL}/api/contracts/${id}/deprecation-info`),
      `/api/contracts/${id}/deprecation-info`
    );
  },

  async getFormalVerificationResults(id: string): Promise<FormalVerificationReport[]> {
    if (USE_MOCKS) {
      return Promise.resolve([]);
    }
    return handleApiCall<FormalVerificationReport[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/formal-verification`),
      `/api/contracts/${id}/formal-verification`
    );
  },

  async runFormalVerification(id: string, data: RunVerificationRequest): Promise<FormalVerificationReport> {
    if (USE_MOCKS) {
      throw new Error('Formal verification is not supported in mock mode');
    }
    return handleApiCall<FormalVerificationReport>(
      () => fetch(`${API_URL}/api/contracts/${id}/formal-verification`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      }),
      `/api/contracts/${id}/formal-verification`
    );
  },

  async getCustomMetricCatalog(id: string): Promise<MetricCatalogEntry[]> {
    if (USE_MOCKS) {
      return Promise.resolve([
        {
          metric_name: 'custom_trades_volume',
          metric_type: 'counter',
          last_seen: new Date().toISOString(),
          sample_count: 128,
        },
        {
          metric_name: 'custom_liquidity_depth',
          metric_type: 'gauge',
          last_seen: new Date().toISOString(),
          sample_count: 72,
        },
      ]);
    }

    const response = await fetch(`${API_URL}/api/contracts/${id}/metrics/catalog`);
    if (!response.ok) throw new Error('Failed to fetch metrics catalog');
    return response.json();
  },

  async getCustomMetricSeries(
    id: string,
    metric: string,
    options?: { resolution?: 'hour' | 'day' | 'raw'; from?: string; to?: string; limit?: number },
  ): Promise<MetricSeriesResponse> {
    if (USE_MOCKS) {
      const now = Date.now();
      const points = Array.from({ length: 24 }).map((_, idx) => {
        const bucketStart = new Date(now - (23 - idx) * 3600_000).toISOString();
        const bucketEnd = new Date(now - (22 - idx) * 3600_000).toISOString();
        return {
          bucket_start: bucketStart,
          bucket_end: bucketEnd,
          sample_count: 12,
          avg_value: Math.random() * 1000,
          p95_value: Math.random() * 1200,
          max_value: Math.random() * 1500,
          sum_value: Math.random() * 5000,
        } satisfies MetricSeriesPoint;
      });

      return Promise.resolve({
        contract_id: id,
        metric_name: metric,
        metric_type: 'counter',
        resolution: options?.resolution ?? 'hour',
        points,
      });
    }

    const queryParams = new URLSearchParams();
    queryParams.append('metric', metric);
    if (options?.resolution) queryParams.append('resolution', options.resolution);
    if (options?.from) queryParams.append('from', options.from);
    if (options?.to) queryParams.append('to', options.to);
    if (options?.limit) queryParams.append('limit', String(options.limit));

    const response = await fetch(
      `${API_URL}/api/contracts/${id}/metrics?${queryParams.toString()}`,
    );
    if (!response.ok) throw new Error('Failed to fetch metric series');
    return response.json();
  },

  // Publisher endpoints
  async getPublisher(id: string): Promise<Publisher> {
    if (USE_MOCKS) {
      return Promise.resolve({
        id: id,
        stellar_address: "G...",
        username: "Mock Publisher",
        created_at: new Date().toISOString(),
      });
    }

    return handleApiCall<Publisher>(
      () => fetch(`${API_URL}/api/publishers/${id}`),
      `/api/publishers/${id}`
    );
  },

  async getPublisherContracts(id: string): Promise<Contract[]> {
    if (USE_MOCKS) {
      return Promise.resolve(
        MOCK_CONTRACTS.filter((c) => c.publisher_id === id),
      );
    }

    return handleApiCall<Contract[]>(
      () => fetch(`${API_URL}/api/publishers/${id}/contracts`),
      `/api/publishers/${id}/contracts`
    );
  },

  async getStats(): Promise<{
    total_contracts: number;
    verified_contracts: number;
    total_publishers: number;
  }> {
    if (USE_MOCKS) {
      return Promise.resolve({
        total_contracts: MOCK_CONTRACTS.length,
        verified_contracts: MOCK_CONTRACTS.filter((c) => c.is_verified).length,
        total_publishers: 5,
      });
    }

    return handleApiCall<{
      total_contracts: number;
      verified_contracts: number;
      total_publishers: number;
    }>(
      () => fetch(`${API_URL}/api/stats`),
      '/api/stats'
    );
  },

  // Compatibility endpoints
  async getCompatibility(id: string): Promise<CompatibilityMatrix> {
    return handleApiCall<CompatibilityMatrix>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility`),
      `/api/contracts/${id}/compatibility`
    );
  },

  async addCompatibility(id: string, data: AddCompatibilityRequest): Promise<unknown> {
    return handleApiCall<unknown>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      }),
      `/api/contracts/${id}/compatibility`
    );
  },

  getCompatibilityExportUrl(id: string, format: 'csv' | 'json'): string {
    return `${API_URL}/api/contracts/${id}/compatibility/export?format=${format}`;
  },

  // Graph endpoint (backend may return { graph: {} } or { nodes, edges }; normalize to GraphResponse)
  async getContractGraph(network?: string): Promise<GraphResponse> {
    const queryParams = new URLSearchParams();
    if (network) queryParams.append("network", network);
    const qs = queryParams.toString();
    return handleApiCall<GraphResponse>(
      () => fetch(`${API_URL}/api/contracts/graph${qs ? `?${qs}` : ""}`),
      '/api/contracts/graph'
    );
  },

  async getTemplates(): Promise<Template[]> {
    if (USE_MOCKS) {
      return Promise.resolve([]);
    }
    return handleApiCall<Template[]>(
      () => fetch(`${API_URL}/api/templates`),
      '/api/templates'
    );
  },

  // SDK / Wasm / Network Compatibility Testing (Issue #261)
  async getCompatibilityMatrix(id: string): Promise<CompatibilityTestMatrixResponse> {
    return handleApiCall<CompatibilityTestMatrixResponse>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility-matrix`),
      `/api/contracts/${id}/compatibility-matrix`
    );
  },

  async runCompatibilityTest(id: string, data: RunCompatibilityTestRequest): Promise<CompatibilityTestEntry> {
    return handleApiCall<CompatibilityTestEntry>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility-matrix/test`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      }),
      `/api/contracts/${id}/compatibility-matrix/test`
    );
  },

  async getCompatibilityHistory(id: string, limit?: number, offset?: number): Promise<CompatibilityHistoryResponse> {
    const params = new URLSearchParams();
    if (limit != null) params.set('limit', String(limit));
    if (offset != null) params.set('offset', String(offset));
    const qs = params.toString();
    return handleApiCall<CompatibilityHistoryResponse>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility-matrix/history${qs ? `?${qs}` : ''}`),
      `/api/contracts/${id}/compatibility-matrix/history`
    );
  },

  async getCompatibilityNotifications(id: string): Promise<CompatibilityNotification[]> {
    return handleApiCall<CompatibilityNotification[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility-matrix/notifications`),
      `/api/contracts/${id}/compatibility-matrix/notifications`
    );
  },

  async markCompatibilityNotificationsRead(id: string): Promise<unknown> {
    return handleApiCall<unknown>(
      () => fetch(`${API_URL}/api/contracts/${id}/compatibility-matrix/notifications/read`, {
        method: 'POST',
      }),
      `/api/contracts/${id}/compatibility-matrix/notifications/read`
    );
  },

  async getCompatibilityDashboard(): Promise<CompatibilityDashboardResponse> {
    return handleApiCall<CompatibilityDashboardResponse>(
      () => fetch(`${API_URL}/api/compatibility-dashboard`),
      '/api/compatibility-dashboard'
    );
  },

  // ── Release Notes Generation ────────────────────────────────────────────

  async listReleaseNotes(id: string): Promise<ReleaseNotesResponse[]> {
    return handleApiCall<ReleaseNotesResponse[]>(
      () => fetch(`${API_URL}/api/contracts/${id}/release-notes`),
      `/api/contracts/${id}/release-notes`
    );
  },

  async getReleaseNotes(id: string, version: string): Promise<ReleaseNotesResponse> {
    return handleApiCall<ReleaseNotesResponse>(
      () => fetch(`${API_URL}/api/contracts/${id}/release-notes/${version}`),
      `/api/contracts/${id}/release-notes/${version}`
    );
  },

  async generateReleaseNotes(
    id: string,
    req: GenerateReleaseNotesRequest
  ): Promise<ReleaseNotesResponse> {
    return handleApiCall<ReleaseNotesResponse>(
      () =>
        fetch(`${API_URL}/api/contracts/${id}/release-notes/generate`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req),
        }),
      `/api/contracts/${id}/release-notes/generate`
    );
  },

  async updateReleaseNotes(
    id: string,
    version: string,
    req: UpdateReleaseNotesRequest
  ): Promise<ReleaseNotesResponse> {
    return handleApiCall<ReleaseNotesResponse>(
      () =>
        fetch(`${API_URL}/api/contracts/${id}/release-notes/${version}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req),
        }),
      `/api/contracts/${id}/release-notes/${version}`
    );
  },

  async publishReleaseNotes(
    id: string,
    version: string,
    req?: PublishReleaseNotesRequest
  ): Promise<ReleaseNotesResponse> {
    return handleApiCall<ReleaseNotesResponse>(
      () =>
        fetch(`${API_URL}/api/contracts/${id}/release-notes/${version}/publish`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req ?? { update_version_record: true }),
        }),
      `/api/contracts/${id}/release-notes/${version}/publish`
    );
  },

  // Database Migration Versioning (Issue #252)
  async getMigrationStatus(): Promise<MigrationStatusResponse> {
    return handleApiCall<MigrationStatusResponse>(
      () => fetch(`${API_URL}/api/admin/migrations/status`),
      '/api/admin/migrations/status'
    );
  },

  async registerMigration(data: RegisterMigrationRequest): Promise<RegisterMigrationResponse> {
    return handleApiCall<RegisterMigrationResponse>(
      () => fetch(`${API_URL}/api/admin/migrations/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      }),
      '/api/admin/migrations/register'
    );
  },

  async validateMigrations(): Promise<MigrationValidationResponse> {
    return handleApiCall<MigrationValidationResponse>(
      () => fetch(`${API_URL}/api/admin/migrations/validate`),
      '/api/admin/migrations/validate'
    );
  },

  async getMigrationLockStatus(): Promise<LockStatusResponse> {
    return handleApiCall<LockStatusResponse>(
      () => fetch(`${API_URL}/api/admin/migrations/lock`),
      '/api/admin/migrations/lock'
    );
  },

  async getMigrationVersion(version: number): Promise<SchemaVersion> {
    return handleApiCall<SchemaVersion>(
      () => fetch(`${API_URL}/api/admin/migrations/${version}`),
      `/api/admin/migrations/${version}`
    );
  },

  async rollbackMigration(version: number): Promise<RollbackResponse> {
    return handleApiCall<RollbackResponse>(
      () => fetch(`${API_URL}/api/admin/migrations/${version}/rollback`, {
        method: 'POST',
      }),
      `/api/admin/migrations/${version}/rollback`
    );
  },
};

export interface Template {
  id: string;
  slug: string;
  name: string;
  description?: string;
  category: string;
  version: string;
  install_count: number;
  // Image fields for template icon/thumbnail
  thumbnail_url?: string;
  parameters: {
    name: string;
    type: string;
    default?: string;
    description?: string;
  }[];
  created_at: string;
}

export interface GraphNode {
  id: string;
  contract_id: string;
  name: string;
  network: "mainnet" | "testnet" | "futurenet";
  is_verified: boolean;
  category?: string;
  tags: string[];
}

export interface GraphEdge {
  source: string;
  target: string;
  dependency_type: string;
}

export interface GraphResponse {
  nodes: GraphNode[];
  edges: GraphEdge[];
}


export interface ContractExample {
  id: string;
  contract_id: string;
  title: string;
  description?: string;
  code_rust?: string;
  code_js?: string;
  category?: "basic" | "advanced" | "integration";
  rating_up: number;
  rating_down: number;
  created_at: string;
  updated_at: string;
}

export interface ExampleRating {
  id: string;
  example_id: string;
  user_address: string;
  rating: number;
  created_at: string;
}

// ─── Compatibility Matrix ────────────────────────────────────────────────────

export interface CompatibilityEntry {
  target_contract_id: string;
  target_contract_stellar_id: string;
  target_contract_name: string;
  target_version: string;
  stellar_version?: string;
  is_compatible: boolean;
}

/** Shape returned by GET /api/contracts/:id/compatibility */
export interface CompatibilityMatrix {
  contract_id: string;
  /** Keyed by source version string */
  versions: Record<string, CompatibilityEntry[]>;
  warnings: string[];
  total_entries: number;
}

export interface AddCompatibilityRequest {
  source_version: string;
  target_contract_id: string;
  target_version: string;
  stellar_version?: string;
  is_compatible: boolean;
}

// ─── SDK / Wasm / Network Compatibility Testing (Issue #261) ─────────────────

export type CompatibilityTestStatus = 'compatible' | 'warning' | 'incompatible';

export interface CompatibilityTestEntry {
  sdk_version: string;
  wasm_runtime: string;
  network: string;
  status: CompatibilityTestStatus;
  tested_at: string;
  test_duration_ms?: number;
  error_message?: string;
}

export interface CompatibilityTestSummary {
  total_tests: number;
  compatible_count: number;
  warning_count: number;
  incompatible_count: number;
}

export interface CompatibilityTestMatrixResponse {
  contract_id: string;
  sdk_versions: string[];
  wasm_runtimes: string[];
  networks: string[];
  entries: CompatibilityTestEntry[];
  summary: CompatibilityTestSummary;
  last_tested?: string;
}

export interface RunCompatibilityTestRequest {
  sdk_version: string;
  wasm_runtime: string;
  network: string;
}

export interface CompatibilityHistoryEntry {
  id: string;
  contract_id: string;
  sdk_version: string;
  wasm_runtime: string;
  network: string;
  previous_status?: CompatibilityTestStatus;
  new_status: CompatibilityTestStatus;
  changed_at: string;
  change_reason?: string;
}

export interface CompatibilityHistoryResponse {
  contract_id: string;
  changes: CompatibilityHistoryEntry[];
  total: number;
}

export interface CompatibilityNotification {
  id: string;
  contract_id: string;
  sdk_version: string;
  message: string;
  is_read: boolean;
  created_at: string;
}

export interface CompatibilityDashboardResponse {
  total_contracts_tested: number;
  overall_compatible: number;
  overall_warning: number;
  overall_incompatible: number;
  sdk_versions: string[];
  recent_changes: CompatibilityHistoryEntry[];
}

// ─── Database Migration Versioning (Issue #252) ──────────────────────────────

export interface SchemaVersion {
  id: number;
  version: number;
  description: string;
  filename: string;
  checksum: string;
  applied_at: string;
  applied_by: string;
  execution_time_ms?: number;
  rolled_back_at?: string;
  rollback_by?: string;
}

export interface MigrationStatusResponse {
  current_version?: number;
  total_applied: number;
  total_rolled_back: number;
  pending_count: number;
  versions: SchemaVersion[];
  has_lock: boolean;
  healthy: boolean;
  warnings: string[];
}

export interface ChecksumMismatch {
  version: number;
  filename: string;
  expected_checksum: string;
  actual_checksum: string;
}

export interface MigrationValidationResponse {
  valid: boolean;
  mismatches: ChecksumMismatch[];
  missing: number[];
}

export interface RegisterMigrationRequest {
  version: number;
  description: string;
  filename: string;
  sql_content: string;
  down_sql?: string;
}

export interface RegisterMigrationResponse {
  version: number;
  checksum: string;
  message: string;
}

export interface RollbackResponse {
  version: number;
  rolled_back_at: string;
  message: string;
}

export interface LockStatusResponse {
  locked: boolean;
  locked_by?: string;
  locked_at?: string;
}

// ─── Formal Verification ─────────────────────────────────────────────────────

export type VerificationStatus = 'Proved' | 'Violated' | 'Unknown' | 'Skipped';

export interface FormalVerificationSession {
  id: string;
  contract_id: string;
  version: string;
  verifier_version: string;
  created_at: string;
  updated_at: string;
}

export interface FormalVerificationProperty {
  id: string;
  session_id: string;
  property_id: string;
  description?: string;
  invariant: string;
  severity: string;
}

export interface FormalVerificationResult {
  id: string;
  property_id: string;
  status: VerificationStatus;
  counterexample?: string;
  details?: string;
}

export interface FormalVerificationPropertyResult {
  property: FormalVerificationProperty;
  result: FormalVerificationResult;
}

export interface FormalVerificationReport {
  session: FormalVerificationSession;

  properties: FormalVerificationPropertyResult[];
}

export interface RunVerificationRequest {
  properties_file: string;
  verifier_version?: string;
}
