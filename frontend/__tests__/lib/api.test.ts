import { api } from '../../lib/api';
import type { PublishRequest } from '../../lib/api';
import fetchMock from 'jest-fetch-mock';
import { trackEvent } from '../../lib/analytics';
import { NetworkError } from '../../lib/errors';

jest.mock('../../lib/analytics', () => ({ trackEvent: jest.fn() }));

const API_URL = 'http://localhost:3001';

beforeEach(() => {
  process.env.NEXT_PUBLIC_USE_MOCKS = 'false';
  process.env.NEXT_PUBLIC_API_URL = API_URL;
  fetchMock.resetMocks();
});

test('getContracts: returns paginated results and calls correct URL', async () => {
  const mock = {
    items: [{ id: 'c1', contract_id: 'c1', wasm_hash: '', name: 'A', publisher_id: 'p1', network: 'mainnet', is_verified: false, tags: [], created_at: new Date().toISOString(), updated_at: new Date().toISOString() }],
    total: 1,
    page: 1,
    page_size: 20,
    total_pages: 1,
  };

  fetchMock.mockResponseOnce(JSON.stringify(mock), { status: 200 });

  const res = await api.getContracts({ query: 'A' });
  expect(res.items).toHaveLength(1);
  expect(fetchMock).toHaveBeenCalled();
  const called = fetchMock.mock.calls[0][0] as string;
  expect(called.startsWith(`${API_URL}/api/contracts`)).toBe(true);
});

test('semanticSearchContracts: applies inferred category and network intent', async () => {
  const mock = makeContractsResponse();
  fetchMock.mockResponseOnce(JSON.stringify(mock), { status: 200 });

  await api.semanticSearchContracts({ query: 'best defi on mainnet' });

  const url = getCalledUrl();
  expect(url.searchParams.getAll('categories')).toContain('DeFi');
  expect(url.searchParams.getAll('networks')).toContain('mainnet');
});

test('semanticSearchContracts: falls back to plain keyword query when semantic result is empty', async () => {
  fetchMock
    .mockResponseOnce(JSON.stringify(makeContractsResponse({ items: [], total: 0 })), {
      status: 200,
    })
    .mockResponseOnce(JSON.stringify(makeContractsResponse({ total: 1 })), { status: 200 });

  const result = await api.semanticSearchContracts({ query: 'governance vault' });

  expect(fetchMock).toHaveBeenCalledTimes(2);
  expect(result.semantic.fallback_used).toBe(true);
  expect(result.items.length).toBe(1);
});

test('semanticSearchContracts: returns semantic query suggestions', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  const result = await api.semanticSearchContracts({ query: 'token factory' });
  expect(result.semantic.query_suggestions.length).toBeGreaterThan(0);
});

test('getNetworks: returns network metadata from /networks', async () => {
  const mock = {
    cached_at: new Date().toISOString(),
    networks: [
      {
        id: 'mainnet',
        name: 'Stellar Mainnet',
        network_type: 'mainnet',
        status: 'online',
        endpoints: {
          rpc_url: 'https://rpc-mainnet.stellar.org',
          health_url: 'https://rpc-mainnet.stellar.org/health',
          explorer_url: 'https://stellar.expert/explorer/public',
        },
        last_checked_at: new Date().toISOString(),
        consecutive_failures: 0,
      },
    ],
  };

  fetchMock.mockResponseOnce(JSON.stringify(mock), { status: 200 });

  const res = await api.getNetworks();
  expect(res.networks).toHaveLength(1);
  expect(res.networks[0].endpoints.explorer_url).toContain('stellar.expert');
  expect(fetchMock.mock.calls[0][0]).toBe(`${API_URL}/networks`);
});

test('getContractSearchSuggestions: returns autocomplete results', async () => {
  const mock = {
    items: [
      { text: 'Token Factory', kind: 'contract', score: 1.0 },
      { text: 'DeFi', kind: 'category', score: 0.81 },
    ],
  };

  fetchMock.mockResponseOnce(JSON.stringify(mock), { status: 200 });

  const res = await api.getContractSearchSuggestions('tok');
  expect(res.items).toHaveLength(2);
  expect((fetchMock.mock.calls[0][0] as string)).toContain('/api/contracts/suggestions?q=tok');
});

test('getContract: success and 404 error handling', async () => {
  const contract = { id: 'c2', contract_id: 'c2', wasm_hash: '', name: 'B', publisher_id: 'p2', network: 'testnet', is_verified: true, tags: [], created_at: new Date().toISOString(), updated_at: new Date().toISOString() };
  fetchMock.mockResponseOnce(JSON.stringify(contract), { status: 200 });
  const res = await api.getContract('c2');
  expect(res.id).toBe('c2');

  // 404 case
  fetchMock.mockResponseOnce(JSON.stringify({ message: 'Not found' }), { status: 404 });
  await expect(api.getContract('missing')).rejects.toMatchObject({ statusCode: 404 });
});

test('publishContract: success calls trackEvent, failure tracks error', async () => {
  const track = trackEvent as jest.MockedFunction<typeof trackEvent>;
  const req: PublishRequest = { contract_id: 'p1', name: 'P', network: 'mainnet', tags: [], publisher_address: 'G...' };

  fetchMock.mockResponseOnce(JSON.stringify({ id: 'published-id', ...req, wasm_hash: '', created_at: new Date().toISOString(), updated_at: new Date().toISOString() }), { status: 201 });
  const published = await api.publishContract(req);
  expect(published.id).toBe('published-id');
  expect(track).toHaveBeenCalledWith('contract_published', expect.any(Object));

  // Failure case
  track.mockClear();
  fetchMock.mockResponseOnce(JSON.stringify({ message: 'Server error' }), { status: 500 });
  await expect(api.publishContract(req)).rejects.toBeTruthy();
  expect(track).toHaveBeenCalled();
});

test('handleApiCall: network timeout and network error mapping', async () => {
  // AbortError simulation
  const abortErr = new Error('The user aborted a request');
  abortErr.name = 'AbortError';
  fetchMock.mockRejectedValueOnce(abortErr);
  await expect(api.getContract('x-timeout')).rejects.toBeInstanceOf(NetworkError);

  // Generic network failure (TypeError) simulation
  const typeErr = new TypeError('Failed to fetch');
  fetchMock.mockRejectedValueOnce(typeErr);
  await expect(api.getContract('x-network-fail')).rejects.toBeInstanceOf(NetworkError);
});

// ── Filter tests ────────────────────────────────────────────────────────────

function makeContractsResponse(overrides = {}) {
  return {
    items: [{ id: 'c1', contract_id: 'c1', wasm_hash: '', name: 'A', publisher_id: 'p1', network: 'mainnet', is_verified: false, tags: [], created_at: new Date().toISOString(), updated_at: new Date().toISOString() }],
    total: 1,
    page: 1,
    page_size: 20,
    total_pages: 1,
    ...overrides,
  };
}

function getCalledUrl(): URL {
  const called = fetchMock.mock.calls[0][0] as string;
  return new URL(called);
}

test('getContracts: single network filter sends ?network=mainnet', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  await api.getContracts({ network: 'mainnet' });
  const url = getCalledUrl();
  expect(url.searchParams.get('network')).toBe('mainnet');
});

test('getContracts: multi-network filter sends ?networks= params', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  await api.getContracts({ networks: ['mainnet', 'testnet'] });
  const url = getCalledUrl();
  expect(url.searchParams.getAll('networks')).toEqual(['mainnet', 'testnet']);
  // should NOT use the singular 'network' key for array values
  expect(url.searchParams.get('network')).toBeNull();
});

test('getContracts: single category filter sends ?category=DeFi', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  await api.getContracts({ category: 'DeFi' });
  const url = getCalledUrl();
  expect(url.searchParams.get('category')).toBe('DeFi');
});

test('getContracts: multi-category filter sends ?categories= params', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  await api.getContracts({ categories: ['DeFi', 'NFT'] });
  const url = getCalledUrl();
  expect(url.searchParams.getAll('categories')).toEqual(['DeFi', 'NFT']);
  expect(url.searchParams.get('category')).toBeNull();
});

test('getContracts: network + category combined sends both params', async () => {
  fetchMock.mockResponseOnce(JSON.stringify(makeContractsResponse()), { status: 200 });
  await api.getContracts({ network: 'mainnet', category: 'DeFi' });
  const url = getCalledUrl();
  expect(url.searchParams.get('network')).toBe('mainnet');
  expect(url.searchParams.get('category')).toBe('DeFi');
});

test('getContracts: legacy "contracts" response key is normalized to items', async () => {
  const legacy = { contracts: [{ id: 'c1', contract_id: 'c1', wasm_hash: '', name: 'A', publisher_id: 'p1', network: 'mainnet', is_verified: false, tags: [], created_at: new Date().toISOString(), updated_at: new Date().toISOString() }], total: 1, page: 1, pages: 1 };
  fetchMock.mockResponseOnce(JSON.stringify(legacy), { status: 200 });
  const res = await api.getContracts({});
  expect(res.items).toHaveLength(1);
  expect(res.items[0].id).toBe('c1');
});

test('getContracts: legacy "pages" response key is normalized to total_pages', async () => {
  const legacy = { contracts: [], total: 0, page: 1, pages: 5 };
  fetchMock.mockResponseOnce(JSON.stringify(legacy), { status: 200 });
  const res = await api.getContracts({});
  expect(res.total_pages).toBe(5);
});

// ── End filter tests ─────────────────────────────────────────────────────────

test('getCompatibilityExportUrl returns expected URL', () => {
  const url = api.getCompatibilityExportUrl('c123', 'json');
  expect(url).toBe(`${API_URL}/api/contracts/c123/compatibility/export?format=json`);
});

test('getContractInteractions: success and failure', async () => {
  const interactions = { items: [], total: 0, limit: 10, offset: 0 };
  fetchMock.mockResponseOnce(JSON.stringify(interactions), { status: 200 });
  const res = await api.getContractInteractions('c1');
  expect(res).toMatchObject(interactions);

  fetchMock.mockResponseOnce('Bad', { status: 500 });
  await expect(api.getContractInteractions('c1')).rejects.toThrow();
});

// ── Comment / Discussion tests (Issue #516) ───────────────────────────────────

test('getComments: returns comment list from /api/contracts/:id/comments', async () => {
  // Force non-window environment so the real API path is taken
  const windowSpy = jest.spyOn(global, 'window', 'get').mockReturnValue(undefined as unknown as Window & typeof globalThis);
  const mock = {
    items: [
      {
        id: 'c1',
        contract_id: 'contract-1',
        parent_id: null,
        author: 'GABC...',
        body: 'Test comment',
        created_at: new Date().toISOString(),
        score: 0,
        flagged: false,
        flag_count: 0,
      },
    ],
    total: 1,
  };

  fetchMock.mockResponseOnce(JSON.stringify(mock), { status: 200 });
  const res = await api.getComments('contract-1');
  expect(res.items).toHaveLength(1);
  expect(res.total).toBe(1);
  const calledUrl = fetchMock.mock.calls[0][0] as string;
  expect(calledUrl).toContain('/api/contracts/contract-1/comments');
  windowSpy.mockRestore();
});

test('postComment: sends POST to /api/contracts/:id/comments with body', async () => {
  const windowSpy = jest.spyOn(global, 'window', 'get').mockReturnValue(undefined as unknown as Window & typeof globalThis);
  const newComment = {
    id: 'new-1',
    contract_id: 'contract-1',
    parent_id: null,
    author: 'GABC...',
    body: 'Hello',
    created_at: new Date().toISOString(),
    score: 0,
    flagged: false,
    flag_count: 0,
  };

  fetchMock.mockResponseOnce(JSON.stringify(newComment), { status: 201 });
  const res = await api.postComment('contract-1', 'Hello');
  expect(res.id).toBe('new-1');
  const [calledUrl, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
  expect(calledUrl).toContain('/api/contracts/contract-1/comments');
  expect(opts.method).toBe('POST');
  expect(JSON.parse(opts.body as string)).toMatchObject({ body: 'Hello' });
  windowSpy.mockRestore();
});

test('voteComment: sends POST to /api/comments/:id/vote with direction', async () => {
  const windowSpy = jest.spyOn(global, 'window', 'get').mockReturnValue(undefined as unknown as Window & typeof globalThis);
  const voteRes = { comment_id: 'cm1', direction: 'up' };

  fetchMock.mockResponseOnce(JSON.stringify(voteRes), { status: 200 });
  const res = await api.voteComment('cm1', 'contract-1', 'up');
  expect(res.direction).toBe('up');
  const [calledUrl, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
  expect(calledUrl).toContain('/api/comments/cm1/vote');
  expect(opts.method).toBe('POST');
  expect(JSON.parse(opts.body as string)).toMatchObject({ direction: 'up' });
  windowSpy.mockRestore();
});

test('flagComment: sends POST to /api/comments/:id/flag with reason', async () => {
  const windowSpy = jest.spyOn(global, 'window', 'get').mockReturnValue(undefined as unknown as Window & typeof globalThis);
  const flagRes = { comment_id: 'cm2', reason: 'spam' };

  fetchMock.mockResponseOnce(JSON.stringify(flagRes), { status: 200 });
  const res = await api.flagComment('cm2', 'contract-1', 'spam');
  expect(res.reason).toBe('spam');
  const [calledUrl, opts] = fetchMock.mock.calls[0] as [string, RequestInit];
  expect(calledUrl).toContain('/api/comments/cm2/flag');
  expect(opts.method).toBe('POST');
  expect(JSON.parse(opts.body as string)).toMatchObject({ reason: 'spam' });
  windowSpy.mockRestore();
});
