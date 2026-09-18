import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  apiFetch,
  getCategories,
  getCategoryEvents,
  getEvent,
  search,
} from '@/lib/api';

import searchOpen from './fixtures/search-open.json';
import searchDisambiguation from './fixtures/search-disambiguation.json';
import searchCategories from './fixtures/search-categories.json';
import eventPage from './fixtures/event-page.json';
import eventPageNullUrl from './fixtures/event-page-null-url.json';
import categories from './fixtures/categories.json';
import categoryEvents from './fixtures/category-events.json';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function fixtureResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('typed API client: contract shapes from recorded fixtures', () => {
  it('search() returns the open-mode payload with ordered cards', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(searchOpen)),
    );

    const response = await search('compré un auto usado');
    expect(response.mode).toBe('open');
    expect(response.query).toBe(searchOpen.query);
    expect(response.confidence).toBe(searchOpen.confidence);
    if (response.mode !== 'open') {
      throw new Error('expected the open arm of the SearchResponse union');
    }
    const result = response.results[0];
    expect(result.event.slug).toBe('comprar-vehiculo');
    expect(result.procedures.length).toBeGreaterThan(0);

    // Compile-time guard: the union forbids reading options on an open
    // response. If the union ever stops discriminating, this line stops
    // being an error and tsc --noEmit fails the build.
    if (response.mode === 'open') {
      // @ts-expect-error options does not exist on the open arm
      void response.options;
    }
  });

  it('search() returns disambiguation options', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(searchDisambiguation)),
    );

    const response = await search('quiero sacar la licencia');
    expect(response.mode).toBe('disambiguation');
    if (response.mode !== 'disambiguation') {
      throw new Error('expected the disambiguation arm of the union');
    }
    expect(response.options.length).toBeGreaterThan(0);
    for (const option of response.options) {
      expect(typeof option.slug).toBe('string');
      expect(typeof option.score).toBe('number');
    }
  });

  it('search() returns the categories fallback', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(searchCategories)),
    );

    const response = await search('hola que tal');
    expect(response.mode).toBe('categories');
    if (response.mode !== 'categories') {
      throw new Error('expected the categories arm of the union');
    }
    expect(response.categories[0].slug).toBe(searchCategories.categories[0].slug);
  });

  it('getEvent() returns the event page with attribution shapes', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(eventPage)),
    );

    const page = await getEvent('comprar-vehiculo');
    expect(page?.slug).toBe('comprar-vehiculo');
    expect(page?.category).toBe('vehiculos');
    const card = page?.procedures[0];
    expect(card).toMatchObject({
      external_id: expect.any(String),
      order: expect.any(Number),
      required: expect.any(Boolean),
      official_url: expect.anything(),
      cost_display: expect.any(String),
    });
    expect(card?.source).toMatchObject({
      official: expect.any(Boolean),
      name: expect.any(String),
      license: expect.any(String),
    });
  });

  it('getEvent() maps a 404 response to null (caller renders notFound)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse({}, 404)),
    );

    const page = await getEvent('no-existe');
    expect(page).toBeNull();
  });

  it('getEvent() surfaces the null-URL card shape from the fixture', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(eventPageNullUrl)),
    );

    const card = (await getEvent('fixture-sin-url'))?.procedures[0];
    expect(card?.source.official_url).toBeNull();
    expect(card?.official_url).toBeNull();
    expect(card?.cost).toBeNull();
    expect(card?.cost_display).toBe('Sin costo informado');
  });

  it('getCategories() returns the ordered category list', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(categories)),
    );

    const page = await getCategories();
    expect(page?.categories[0]).toMatchObject({
      slug: 'vehiculos',
      name: expect.any(String),
      order_index: expect.any(Number),
    });
  });

  it('getCategoryEvents() returns the event list and 404 → null', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse(categoryEvents)),
    );

    const page = await getCategoryEvents('vehiculos');
    expect(page?.category).toBe('vehiculos');
    expect(page?.events.length).toBeGreaterThan(0);

    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse({}, 404)),
    );
    expect(await getCategoryEvents('no-existe')).toBeNull();
  });
});

describe('typed API client: exclusion contract (proposal P2)', () => {
  it('no client function exists for /search/debug, /search/feedback, /procedures/{id}', async () => {
    const module = (await import('@/lib/api')) as unknown as Record<
      string,
      unknown
    >;
    const keys = Object.keys(module);
    for (const key of keys) {
      expect(key.toLowerCase()).not.toContain('debug');
      expect(key.toLowerCase()).not.toContain('feedback');
      expect(key.toLowerCase()).not.toContain('procedure');
      expect(key.toLowerCase()).not.toContain('procedures');
    }
  });

  it('search() requests the relative search path with the q parameter', async () => {
    const fetchMock = vi.fn(async () => fixtureResponse(searchOpen));
    vi.stubGlobal('fetch', fetchMock);

    await search('compré un auto usado');

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/v1/search?q=' + encodeURIComponent('compré un auto usado'));
  });

  it('apiFetch remains the single wrapper (freshness pin re-exported here)', async () => {
    const fetchMock = vi.fn(async () => fixtureResponse({}));
    vi.stubGlobal('fetch', fetchMock);
    await apiFetch('/api/v1/categories');
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/categories', {
      cache: 'no-store',
      revalidate: 0,
    });
  });
});
