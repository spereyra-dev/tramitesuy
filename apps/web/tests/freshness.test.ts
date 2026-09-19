/**
 * Freshness + same-origin pinning tests (PR 1; extended in PR 2 when the
 * live check exposed that Node fetch cannot parse a relative URL server-side).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { headers } from 'next/headers';
import type { ReadonlyHeaders } from 'next/dist/server/web/spec-extension/adapters/headers';

import { apiFetch, type FetchInit } from '@/lib/api';

vi.mock('next/headers', () => ({
  headers: vi.fn(),
}));

const mockedHeaders = vi.mocked(headers);

function okResponse(): Response {
  return new Response(JSON.stringify({}), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  });
}

beforeEach(() => {
  // Inside a request scope: the incoming request's own web origin.
  mockedHeaders.mockImplementation(async () => {
    const store = new Map<string, string>([['host', 'localhost:3000']]);
    return {
      get: (name: string) => (store.has(name) ? store.get(name)! : null),
    } as unknown as ReadonlyHeaders;
  });
});

afterEach(() => {
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

  it('server-side fetch resolves the relative path to the same WEB origin, never the API origin', async () => {
    // Node fetch cannot parse a relative URL, so the wrapper resolves it
    // against the incoming request's own origin; the Next rewrite still
    // forwards to API_BASE_URL, so the request stays on the same-origin
    // /api/v1 proxy path.
    const fetchMock = vi.fn(async () => okResponse());
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/search?q=auto');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toBe('http://localhost:3000/api/v1/search?q=auto');
    expect(url).not.toMatch(/:8080/);
  });
});
