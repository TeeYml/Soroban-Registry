# API Advanced Features

## Overview

This document covers advanced features of the Soroban Registry API including batch operations, advanced filtering, sorting, pagination, search, aggregations, and nested relationships. These features enable efficient data retrieval and bulk operations.

## Table of Contents

1. [Batch Operations](#batch-operations)
2. [Advanced Filtering](#advanced-filtering)
3. [Sorting](#sorting)
4. [Pagination](#pagination)
5. [Full-Text Search](#full-text-search)
6. [Aggregations](#aggregations)
7. [Nested Resources & Includes](#nested-resources--includes)
8. [Performance Characteristics](#performance-characteristics)
9. [Use Cases & Recipes](#use-cases--recipes)

---

## Planned Endpoints (Return 501)

The following endpoints are routed but intentionally return `501 Not Implemented` until full functionality ships:

- `GET /api/contracts/:id/state/:key`
- `PUT /api/contracts/:id/state/:key`
- `GET /api/contracts/:id/trust-score`
- `GET /api/contracts/:id/deployment-status`
- `POST /api/contracts/:id/deploy-green`

Response shape:

```json
{
  "error": "not_implemented",
  "message": "This endpoint is planned but not yet functional"
}
```

---

## Network Metadata

Use the networks endpoint to discover supported environments and populate client-side network selectors.

### GET /networks

Returns all supported deployment networks with RPC URLs, explorer links, and current status indicators.

**Response:**
```json
{
  "cached_at": "2026-03-25T10:15:00Z",
  "networks": [
    {
      "id": "mainnet",
      "name": "Stellar Mainnet",
      "network_type": "mainnet",
      "status": "online",
      "endpoints": {
        "rpc_url": "https://rpc-mainnet.stellar.org",
        "health_url": "https://rpc-mainnet.stellar.org/health",
        "explorer_url": "https://stellar.expert/explorer/public"
      },
      "last_checked_at": "2026-03-25T10:15:00Z",
      "last_indexed_ledger_height": 52345678,
      "consecutive_failures": 0
    }
  ]
}
```

**Status Values:**
- `online` - RPC health check is passing and the indexer has no known issues
- `degraded` - RPC is reachable but indexing is stale or reporting failures
- `offline` - RPC health checks are failing or the network is in a sustained failure state

**Use Cases:**
- Populate frontend network selectors dynamically
- Show environment-specific explorer links
- Gate features when a network is degraded or offline

---

## Batch Operations

Batch operations allow you to perform multiple actions in a single API call, reducing network overhead and improving performance.

### POST /api/contracts/batch-verify

Verify multiple contracts at once.

**Request:**
```http
POST /api/contracts/batch-verify
Content-Type: application/json

{
  "contracts": [
    {
      "contract_id": "CDLZFC3...",
      "source_code": "base64_encoded_source",
      "compiler_version": "21.0.0"
    },
    {
      "contract_id": "CAFX2Y7...",
      "source_code": "base64_encoded_source",
      "compiler_version": "21.0.0"
    }
  ],
  "options": {
    "fail_fast": false,
    "parallel": true
  }
}
```

**Request Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `contracts` | array | Array of contracts to verify (max 100) |
| `options.fail_fast` | boolean | Stop on first failure (default: false) |
| `options.parallel` | boolean | Execute verifications in parallel (default: true) |

**Response:**
```json
{
  "batch_id": "batch_abc123",
  "total": 2,
  "successful": 1,
  "failed": 1,
  "results": [
    {
      "contract_id": "CDLZFC3...",
      "status": "verified",
      "verification_id": "ver_xyz789"
    },
    {
      "contract_id": "CAFX2Y7...",
      "status": "failed",
      "error": {
        "code": "BYTECODE_MISMATCH",
        "message": "Compiled bytecode does not match"
      }
    }
  ]
}
```

**Limits:**
- Maximum 100 contracts per batch
- Each verification follows normal rate limits
- Timeout: 10 minutes for entire batch

**Use Case:** Verify multiple contract versions after deployment.

---

### POST /api/contracts/batch-fetch

Fetch multiple contracts in a single request.

**Request:**
```http
POST /api/contracts/batch-fetch
Content-Type: application/json

{
  "contract_ids": [
    "CDLZFC3...",
    "CAFX2Y7...",
    "CBGTX43..."
  ],
  "include": ["publisher", "verification_status"]
}
```

**Response:**
```json
{
  "contracts": [
    {
      "contract_id": "CDLZFC3...",
      "name": "Token Contract",
      "verification_status": "verified",
      "publisher": {
        "id": "pub_abc123",
        "name": "Acme Corp"
      }
    },
    {
      "contract_id": "CAFX2Y7...",
      "name": "DEX Contract",
      "verification_status": "pending",
      "publisher": {
        "id": "pub_def456",
        "name": "DeFi Labs"
      }
    }
  ],
  "not_found": ["CBGTX43..."]
}
```

**Limits:**
- Maximum 50 contracts per batch
- Non-existent contracts returned in `not_found` array

---

### Using the CLI for Batch Operations

**Batch Manifest (YAML):**

```yaml
version: "1.0"
batch:
  - contract: "CDLZFC3..."
    operation: verify
    params:
      source: "./contracts/token/src"
      compiler_version: "21.0.0"

  - contract: "CAFX2Y7..."
    operation: verify
    params:
      source: "./contracts/dex/src"
      compiler_version: "21.0.0"

  - contract: "CBGTX43..."
    operation: update-metadata
    params:
      name: "Updated Token Contract"
      description: "A better description"
```

**Execute Batch:**

```bash
# Dry run (validate without executing)
soroban-batch run --manifest batch.yaml --dry-run

# Execute batch
soroban-batch run --manifest batch.yaml

# With JSON output
soroban-batch run --manifest batch.yaml --format json > report.json
```

**Batch Report:**
```json
{
  "batch_id": "batch_abc123",
  "total": 3,
  "successful": 2,
  "failed": 1,
  "results": [
    {
      "contract": "CDLZFC3...",
      "operation": "verify",
      "status": "success"
    },
    {
      "contract": "CAFX2Y7...",
      "operation": "verify",
      "status": "failed",
      "error": "Compilation failed"
    },
    {
      "contract": "CBGTX43...",
      "operation": "update-metadata",
      "status": "success"
    }
  ]
}
```

**Supported Operations:**
- `publish` - Publish contract
- `verify` - Verify source code
- `update-metadata` - Update contract metadata
- `set-network` - Change network designation
- `retire` - Mark contract as retired

---

## Advanced Filtering

Apply complex filters to narrow down results.

### Basic Filtering

```http
GET /api/contracts?network=mainnet&verified=true
```

### Multi-Field Filtering

```http
GET /api/contracts?network=mainnet&verified=true&publisher=pub_abc123&category=defi
```

### Range Filters

```http
# Contracts created between dates
GET /api/contracts?created_from=2026-01-01T00:00:00Z&created_to=2026-02-01T00:00:00Z

# Contracts modified in the last 7 days
GET /api/contracts?updated_from=2026-03-18T00:00:00Z

# Contracts verified during a date window
GET /api/contracts?verified_from=2026-03-01T00:00:00Z&verified_to=2026-03-25T23:59:59Z

# Contracts with interaction count in range
GET /api/contracts?interactions_min=1000&interactions_max=10000
```

### Operators

Use operators for more complex queries:

**Available Operators:**

| Operator | Syntax | Description | Example |
|----------|--------|-------------|---------|
| Equals | `field=value` | Exact match | `network=mainnet` |
| Not equals | `field_ne=value` | Exclude value | `network_ne=testnet` |
| Greater than | `field_gt=value` | Numeric comparison | `interactions_gt=1000` |
| Less than | `field_lt=value` | Numeric comparison | `interactions_lt=10000` |
| In | `field_in=val1,val2` | Match any value | `category_in=defi,nft` |
| Contains | `field_contains=value` | String contains | `name_contains=token` |
| Starts with | `field_starts=value` | String prefix | `name_starts=Stellar` |

**Examples:**

```http
# Contracts NOT on testnet
GET /api/contracts?network_ne=testnet

# Contracts with high interaction count
GET /api/contracts?interactions_gt=5000

# Contracts in multiple categories
GET /api/contracts?category_in=defi,token,nft

# Contracts with "token" in name
GET /api/contracts?name_contains=token
```

### Complex Filtering (Query DSL)

For very complex queries, use the query DSL:

```http
POST /api/contracts/search
Content-Type: application/json

{
  "query": {
    "bool": {
      "must": [
        { "term": { "network": "mainnet" } },
        { "term": { "verified": true } }
      ],
      "should": [
        { "range": { "interactions": { "gte": 1000 } } },
        { "term": { "featured": true } }
      ],
      "must_not": [
        { "term": { "deprecated": true } }
      ]
    }
  }
}
```

**Query Types:**

- `term` - Exact match
- `range` - Numeric/date range
- `match` - Full-text search
- `wildcard` - Pattern matching
- `bool` - Boolean logic (must, should, must_not)

---

## Sorting

Order results by one or more fields.

### Single Field Sort

```http
# Sort by creation date (newest first)
GET /api/contracts?sort_by=createdat&sort_order=desc

# Sort by most recently updated
GET /api/contracts?sort_by=updatedat&sort_order=desc

# Sort by latest verification timestamp
GET /api/contracts?sort_by=verifiedat&sort_order=desc

# Sort by most recent contract view
GET /api/contracts?sort_by=lastaccessedat&sort_order=desc
```

**Sort Order:**
- `asc` - Ascending (A-Z, 0-9, oldest-newest)
- `desc` - Descending (Z-A, 9-0, newest-oldest)

### Multi-Field Sort

```http
# Sort by verification status, then by interactions
GET /api/contracts?sort_by=verified,interactions&sort_order=desc,desc
```

**Available Sort Fields:**

| Field | Type | Description |
|-------|------|-------------|
| `createdat` | datetime | Contract creation timestamp |
| `updatedat` | datetime | Last update timestamp |
| `verifiedat` | datetime | Most recent successful verification timestamp |
| `lastaccessedat` | datetime | Most recent contract detail view timestamp |
| `interactions` | integer | Total interaction count |
| `popularity` | integer | Interaction-driven popularity ordering |
| `deployments` | integer | Version/deployment count ordering |
| `relevance` | integer | Search relevance ordering when `query` is present |

### Relevance Sorting (Search)

When using full-text search, results are sorted by relevance by default:

```http
POST /api/contracts/search
Content-Type: application/json

{
  "query": "token transfer",
  "sort": {
    "by": "_score",
    "order": "desc"
  }
}
```

---

## Pagination

Efficiently retrieve large datasets using pagination.

### Offset-Based Pagination

Traditional pagination using `limit` and `offset`:

```http
# First page (20 items)
GET /api/contracts?limit=20&offset=0

# Second page
GET /api/contracts?limit=20&offset=20

# Third page
GET /api/contracts?limit=20&offset=40
```

**Response:**
```json
{
  "contracts": [...],
  "pagination": {
    "limit": 20,
    "offset": 40,
    "total": 1543,
    "has_more": true
  }
}
```

**Limits:**
- Max `limit`: 1000 (default: 50)
- Max `offset`: 10,000

**Performance:** Slower for large offsets (offset > 1000).

**Best for:** Small datasets, random access to pages.

---

### Cursor-Based Pagination (Recommended)

More efficient for large datasets:

```http
# First page
GET /api/contracts?limit=50

# Next page (use cursor from previous response)
GET /api/contracts?limit=50&cursor=eyJpZCI6MTIzNDU...
```

**Response:**
```json
{
  "contracts": [...],
  "pagination": {
    "limit": 50,
    "next_cursor": "eyJpZCI6MTI4OTA...",
    "has_more": true
  }
}
```

**Advantages:**
- Constant performance regardless of position
- Handles real-time updates correctly
- No maximum offset limitation

**Disadvantages:**
- Cannot jump to arbitrary page
- Forward-only iteration

**Best for:** Large datasets, streaming, real-time updates.

---

### Pagination Example (Python)

```python
import requests

def fetch_all_contracts(api_url, limit=100):
    """Fetch all contracts using cursor-based pagination"""
    contracts = []
    cursor = None

    while True:
        params = {'limit': limit}
        if cursor:
            params['cursor'] = cursor

        response = requests.get(f'{api_url}/api/contracts', params=params)
        data = response.json()

        contracts.extend(data['contracts'])

        if not data['pagination']['has_more']:
            break

        cursor = data['pagination']['next_cursor']

    return contracts

# Usage
all_contracts = fetch_all_contracts('https://registry.soroban.example')
print(f'Fetched {len(all_contracts)} contracts')
```

---

## Full-Text Search

Contract search is powered by PostgreSQL full-text indexes over `name`, `category`, and `description`.

### Basic Search

```http
GET /api/contracts?query=token transfer
```

### Advanced Search Syntax

The `query` parameter supports operator-aware parsing via PostgreSQL web-style search:

```http
# Exact phrase
GET /api/contracts?query="token factory"

# Required and excluded terms
GET /api/contracts?query=token -deprecated

# Phrase with additional required term
GET /api/contracts?query="liquidity pool" soroban
```

**Supported Patterns:**

| Pattern | Description | Example |
|---------|-------------|---------|
| Phrase | Exact word sequence | `"token contract"` |
| Required terms | Terms must appear | `stellar token` |
| Excluded terms | Terms must not appear | `token -legacy` |
| Prefix term | Prefix match for a token | `token*` |

### Relevance Ranking

When `sort_by=relevance` or when `query` is present without an explicit sort, matches are ranked by:

- Weighted full-text score across `name`, `category`, and `description`
- Exact contract name matches boosted above partial matches
- Prefix name matches boosted above generic body matches

### Search Suggestions

Use the suggestions endpoint to power autocomplete UIs:

```http
GET /api/contracts/suggestions?q=tok&limit=8
```

**Response:**
```json
{
  "items": [
    { "text": "Token Factory", "kind": "contract", "score": 1.0 },
    { "text": "Token", "kind": "category", "score": 0.82 }
  ]
}
```

### Performance and Monitoring

- Search uses generated `tsvector` columns plus GIN indexes for low-latency ranking
- Autocomplete uses prefix and trigram indexes on contract names and categories
- Search requests slower than 200ms are logged and counted in Prometheus metrics
- Suggestion responses are cached briefly to keep selector/autocomplete latency low

---

## Aggregations

Compute statistics and group data.

### GET /api/contracts/stats

Get aggregate statistics:

```http
GET /api/contracts/stats?network=mainnet
```

**Response:**
```json
{
  "total_contracts": 1543,
  "verified_contracts": 987,
  "total_interactions": 1234567,
  "by_network": {
    "mainnet": 1200,
    "testnet": 300,
    "futurenet": 43
  },
  "by_category": {
    "defi": 450,
    "nft": 320,
    "token": 280,
    "other": 493
  },
  "by_publisher": [
    { "publisher": "Acme Corp", "count": 23 },
    { "publisher": "DeFi Labs", "count": 18 },
    { "publisher": "Token Inc", "count": 15 }
  ]
}
```

### POST /api/contracts/aggregate

Custom aggregations:

```http
POST /api/contracts/aggregate
Content-Type: application/json

{
  "aggregations": {
    "by_month": {
      "type": "date_histogram",
      "field": "published_at",
      "interval": "month"
    },
    "avg_interactions": {
      "type": "avg",
      "field": "interactions"
    },
    "top_publishers": {
      "type": "terms",
      "field": "publisher",
      "size": 10
    }
  },
  "filters": {
    "network": "mainnet",
    "verified": true
  }
}
```

**Response:**
```json
{
  "aggregations": {
    "by_month": {
      "buckets": [
        { "key": "2026-01", "count": 123 },
        { "key": "2026-02", "count": 156 }
      ]
    },
    "avg_interactions": {
      "value": 456.7
    },
    "top_publishers": {
      "buckets": [
        { "key": "pub_abc123", "name": "Acme Corp", "count": 23 }
      ]
    }
  }
}
```

**Aggregation Types:**

| Type | Description | Use Case |
|------|-------------|----------|
| `count` | Count matching documents | Total contracts |
| `sum` | Sum numeric field | Total interactions |
| `avg` | Average value | Average popularity |
| `min/max` | Min/max value | Range of values |
| `terms` | Group by field | Top publishers |
| `date_histogram` | Time-based buckets | Contracts per month |
| `range` | Numeric ranges | Interaction tiers |

---

## Nested Resources & Includes

Efficiently load related resources.

### Default Response (Minimal)

```http
GET /api/contracts/CDLZFC3...
```

**Response:**
```json
{
  "contract_id": "CDLZFC3...",
  "name": "Token Contract",
  "publisher_id": "pub_abc123",
  "verified": true
}
```

### With Includes

```http
GET /api/contracts/CDLZFC3...?include=publisher,verification,interactions
```

**Response:**
```json
{
  "contract_id": "CDLZFC3...",
  "name": "Token Contract",
  "publisher": {
    "id": "pub_abc123",
    "name": "Acme Corp",
    "verified_publisher": true,
    "total_contracts": 12
  },
  "verification": {
    "verified": true,
    "verified_at": "2026-02-15T10:30:00Z",
    "compiler_version": "21.0.0",
    "source_url": "https://registry.../source/..."
  },
  "interactions": {
    "total": 12345,
    "last_24h": 234,
    "last_7d": 1890,
    "last_30d": 7654
  }
}
```

**Available Includes:**

| Include | Description | Performance Impact |
|---------|-------------|--------------------|
| `publisher` | Publisher details | Low |
| `verification` | Verification info | Low |
| `interactions` | Interaction stats | Medium |
| `dependencies` | Contract dependencies | Medium |
| `versions` | Version history | High |
| `similar` | Similar contracts | High |

**Performance Tips:**
- Only include what you need
- Multiple includes incur additive cost
- Consider caching responses with includes

---

## Performance Characteristics

Understanding performance helps you use the API efficiently.

### Operation Performance

| Operation | Average Latency | Throughput | Rate Limit |
|-----------|-----------------|------------|------------|
| GET single contract | 10-30ms | High | 100/min |
| GET list (50 items) | 50-100ms | Medium | 100/min |
| POST search | 30-200ms | Medium | 100/min |
| POST batch-verify (10) | 10-60s | Low | 20/min |
| POST aggregate | 100-500ms | Low | 100/min |

### Query Complexity

**Fast Queries (< 50ms):**
- Simple filters: `?network=mainnet&verified=true`
- Single contract fetch
- Basic pagination (offset < 1000)

**Medium Queries (50-200ms):**
- Full-text search
- Multiple filters with sorting
- Includes (1-2 relationships)
- Cursor pagination
- Aggregations (simple)

**Slow Queries (> 200ms):**
- Complex boolean search
- Multiple includes (3+)
- Large aggregations
- Deep pagination (offset > 5000)

### Optimization Tips

1. **Use cursor pagination** for large datasets
2. **Limit includes** to only what you need
3. **Cache responses** client-side (with TTL)
4. **Use batch endpoints** instead of multiple requests
5. **Filter early** to reduce result set size
6. **Index frequently-queried fields** (contact support)
7. **Avoid deep pagination** (use cursor instead)

### Caching Recommendations

| Endpoint | Cache TTL | Rationale |
|----------|-----------|-----------|
| GET /contracts/{id} | 5 minutes | Contract metadata rarely changes |
| GET /contracts (list) | 1 minute | List changes frequently |
| POST /search | 30 seconds | Real-time search expected |
| GET /stats | 1 hour | Aggregates update slowly |

---

## Use Cases & Recipes

### Recipe 1: Find Trending Contracts

Get contracts with high recent activity:

```http
POST /api/contracts/search
Content-Type: application/json

{
  "filters": {
    "network": "mainnet",
    "verified": true
  },
  "sort": {
    "by": "interactions_7d",
    "order": "desc"
  },
  "limit": 20,
  "include": ["publisher", "interactions"]
}
```

### Recipe 2: Monitor Contract for Updates

Poll for changes to a specific contract:

```python
import requests
import time

def monitor_contract(contract_id, callback, poll_interval=60):
    """Monitor a contract for updates"""
    last_updated = None

    while True:
        response = requests.get(f'https://api.example/contracts/{contract_id}')
        contract = response.json()

        if last_updated and contract['updated_at'] != last_updated:
            callback(contract)

        last_updated = contract['updated_at']
        time.sleep(poll_interval)

# Usage
def on_update(contract):
    print(f"Contract {contract['name']} updated!")

monitor_contract('CDLZFC3...', on_update)
```

### Recipe 3: Bulk Contract Publishing

Publish multiple contracts efficiently:

```yaml
# publish-batch.yaml
version: "1.0"
batch:
  - contract: "token-v1"
    operation: publish
    params:
      source: "./contracts/token/v1"
      name: "Token Contract v1"
      category: "token"

  - contract: "token-v2"
    operation: publish
    params:
      source: "./contracts/token/v2"
      name: "Token Contract v2"
      category: "token"
```

```bash
soroban-batch run --manifest publish-batch.yaml
```

### Recipe 4: Export All Contracts (Backup)

```python
import requests
import json

def export_all_contracts(output_file):
    """Export all contracts to JSON"""
    contracts = []
    cursor = None

    while True:
        params = {'limit': 100, 'include': 'publisher,verification'}
        if cursor:
            params['cursor'] = cursor

        response = requests.get('https://api.example/contracts', params=params)
        data = response.json()

        contracts.extend(data['contracts'])

        if not data['pagination']['has_more']:
            break

        cursor = data['pagination']['next_cursor']
        print(f"Fetched {len(contracts)} contracts...")

    with open(output_file, 'w') as f:
        json.dump(contracts, f, indent=2)

    print(f"Exported {len(contracts)} contracts to {output_file}")

# Usage
export_all_contracts('contracts_backup.json')
```

### Recipe 5: Contract Health Dashboard

```javascript
async function getContractHealth(contractId) {
  const response = await fetch(
    `https://api.example/contracts/${contractId}?include=interactions,health_score`
  );
  const contract = await response.json();

  return {
    name: contract.name,
    health_score: contract.health_score,
    verified: contract.verified,
    last_interaction: contract.interactions.last_interaction_at,
    interactions_7d: contract.interactions.last_7d,
    status: getStatus(contract)
  };
}

function getStatus(contract) {
  if (contract.health_score >= 80) return 'healthy';
  if (contract.health_score >= 60) return 'warning';
  return 'critical';
}
```

---

## Limits and Constraints

| Limit | Value | Upgrade Path |
|-------|-------|--------------|
| Max batch size | 100 items | Contact support |
| Max query results | 10,000 | Use cursor pagination |
| Max includes | 5 | N/A |
| Max search query length | 1,000 chars | N/A |
| Max aggregation buckets | 1,000 | Contact support |
| Rate limit (standard) | 100 req/min | Upgrade to enterprise |
| Batch timeout | 10 minutes | Contact support |

---

## Related Documentation

- [API Rate Limiting](./API_RATE_LIMITING.md) - Rate limits and quotas
- [Error Codes Reference](./ERROR_CODES.md) - Error handling
- [Observability](./OBSERVABILITY.md) - Monitoring API performance

---

## Support

For advanced API features:
- GitHub Issues: https://github.com/ALIPHATICHYD/Soroban-Registry/issues
- Tag with: `api`, `advanced-features`
