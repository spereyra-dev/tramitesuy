/**
 * Freshness + same-origin pinning tests (PR 1; extended in PR 2 when the
 * live check exposed that Node fetch cannot parse a relative URL server-side;
 * extended in the audit-remediation work unit to pin the SSRF fix: the
 * server-side target origin comes from trusted configuration, never the
 * incoming request).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { headers } from 'next/headers';
import type { ReadonlyHeaders } from 'next/dist/server/web/spec-extension/adapters/headers';

import { apiFetch, type FetchInit } from '@/lib/api';

vi.mock('next/headers', () => ({
  headers: vi.fn(),
}));

const mockedHeaders = vi.mocked(headers);

const originalApiBaseUrl = process.env.API_BASE_URL;
const originalPort = process.env.PORT;

function okResponse(): Response {
  return new Response(JSON.stringify({}), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

/** Mocks the incoming request's `Host` header to a (possibly hostile) value. */
function mockIncomingHost(host: string): void {
  mockedHeaders.mockImplementation(async () => {
    const store = new Map<string, string>([['host', host]]);
    return {
      get: (name: string) => (store.has(name) ? store.get(name)! : null),
    } as unknown as ReadonlyHeaders;
  });
}

beforeEach(() => {
  // Inside a request scope: the incoming request's own web origin.
  mockIncomingHost('localhost:3000');
});

afterEach(() => {
  if (originalApiBaseUrl === undefined) delete process.env.API_BASE_URL;
  else process.env.API_BASE_URL = originalApiBaseUrl;
  if (originalPort === undefined) delete process.env.PORT;
  else process.env.PORT = originalPort;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('freshness: the shared fetch wrapper never caches', () => {
  it('every call sets cache: no-store AND revalidate: 0', async () => {
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/categories');
    await apiFetch('/api/v1/events/comprar-vehiculo');

    expect(fetchMock).toHaveBeenCalledTimes(2);
    for (const call of fetchMock.mock.calls) {
      const [, init] = call as unknown as [string, FetchInit];
      expect(init.cache).toBe('no-store');
      expect(init.revalidate).toBe(0);
    }
  });

  it('outside a request scope the relative /api/v1/... path passes through unchanged', async () => {
    mockedHeaders.mockImplementation(async () => {
      throw new Error('headers was called outside a request scope');
    });
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/search?q=auto');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toMatch(/^\/api\/v1\//);
    expect(url).not.toMatch(/^https?:\/\//);
  });

  it('server-side fetch resolves the relative path to the configured internal origin', async () => {
    // Node fetch cannot parse a relative URL, so the wrapper resolves it
    // against the trusted internal API origin (the same API_BASE_URL the
    // Next rewrite forwards to). The incoming request is never consulted.
    process.env.API_BASE_URL = 'http://internal-api.test:9999';
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/search?q=auto');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toBe('http://internal-api.test:9999/api/v1/search?q=auto');
  });
});

describe('SSRF: the server-side origin is trusted configuration, never the request', () => {
  it('a spoofed Host header cannot influence the resolved outgoing URL', async () => {
    mockIncomingHost('attacker.example.com');
    process.env.API_BASE_URL = 'http://internal-api.test:9999';
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/search?q=auto');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toBe('http://internal-api.test:9999/api/v1/search?q=auto');
    expect(url).not.toContain('attacker.example.com');
  });

  it('without API_BASE_URL it falls back to the web app loopback origin', async () => {
    mockIncomingHost('attacker.example.com');
    delete process.env.API_BASE_URL;
    process.env.PORT = '4321';
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/categories');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toBe('http://127.0.0.1:4321/api/v1/categories');
    expect(url).not.toContain('attacker.example.com');
  });
});
