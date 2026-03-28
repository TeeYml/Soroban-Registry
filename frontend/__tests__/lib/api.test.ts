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
