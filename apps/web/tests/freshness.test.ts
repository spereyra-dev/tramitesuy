import { afterEach, describe, expect, it, vi } from 'vitest';

import { apiFetch, type FetchInit } from '@/lib/api';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('freshness: the shared fetch wrapper never caches', () => {
  it('every call sets cache: no-store AND revalidate: 0', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/categories');
    await apiFetch('/api/v1/events/comprar-vehiculo');

    expect(fetchMock).toHaveBeenCalledTimes(2);
    for (const call of fetchMock.mock.calls) {
      const [url, init] = call as unknown as [string, FetchInit];
      expect(init.cache).toBe('no-store');
      expect(init.revalidate).toBe(0);
    }
  });

  it('requested URLs are relative /api/v1/... paths (same-origin proxy)', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({}), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    vi.stubGlobal('fetch', fetchMock);

    await apiFetch('/api/v1/search?q=auto');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, FetchInit];
    expect(url).toMatch(/^\/api\/v1\//);
    expect(url).not.toMatch(/^https?:\/\//);
  });
});
