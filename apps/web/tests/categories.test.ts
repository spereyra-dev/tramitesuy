/**
 * Categories-page tests (task 19, RED first — strict TDD).
 *
 * These render the /categories and /categories/[slug] server components with
 * renderToStaticMarkup over the fixture payloads recorded from the shipped
 * handlers (design §5): the ordered category list (vehiculos first) and the
 * per-category event list. No network, no DB; internal links are rendered
 * through a next/link stub that emits plain anchors (no testing-library,
 * no browser).
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { createElement, type ReactElement, type ReactNode } from 'react';

import CategoriesPage from '@/app/categories/page';
import CategoryEventsPage from '@/app/categories/[slug]/page';

import categories from './fixtures/categories.json';
import categoryEvents from './fixtures/category-events.json';

vi.mock('next/link', () => ({
  default: function FakeLink(props: { href: string; children: ReactNode }) {
    return createElement('a', { href: props.href }, props.children);
  },
}));

vi.mock('next/headers', () => ({
  headers: vi.fn(async () => {
    throw new Error('headers was called outside a request scope');
  }),
}));

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

async function renderCategoriesPage(body: unknown): Promise<string> {
  vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(body)));
  const element = (await CategoriesPage()) as ReactElement;
  return renderToStaticMarkup(element);
}

async function renderCategoryEvents(
  slug: string,
  body: unknown,
): Promise<string> {
  vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(body)));
  const element = (await CategoryEventsPage({
    params: Promise.resolve({ slug }),
  })) as ReactElement;
  return renderToStaticMarkup(element);
}

describe('categories list page (/categories)', () => {
  it('renders the categories in order_index ascending, vehiculos first', async () => {
    // Triangulated ordering: the array arrives with vehiculos second, but
    // its order_index (1) puts it first on the page.
    const shuffled = {
      categories: [
        { slug: 'trabajo', name: 'Trabajo', order_index: 2 },
        ...categories.categories,
      ],
    };
    const html = await renderCategoriesPage(shuffled);
    const vehiculos = html.indexOf('/categories/vehiculos');
    const trabajo = html.indexOf('/categories/trabajo');
    expect(vehiculos).toBeGreaterThanOrEqual(0);
    expect(trabajo).toBeGreaterThan(vehiculos);
  });

  it('renders every category as a link to its category page', async () => {
    const html = await renderCategoriesPage(categories);
    expect(html).toContain('Vehículos');
    expect(html).toContain('href="/categories/vehiculos"');
  });
});

describe('category events page (/categories/[slug])', () => {
  it('renders the category event list with links to the event pages', async () => {
    const html = await renderCategoryEvents('vehiculos', categoryEvents);
    expect(html).toContain('vehiculos');
    expect(html).toContain('href="/events/comprar-vehiculo"');
    expect(html).toContain('Comprar un vehículo');
    expect(html).toContain('href="/events/vender-vehiculo"');
    expect(html).toContain('Vender un vehículo');
  });

  it('renders every returned event as a link (the categories search mode gets a real destination)', async () => {
    const html = await renderCategoryEvents('vehiculos', categoryEvents);
    for (const event of categoryEvents.events) {
      expect(html).toContain(`href="/events/${event.slug}"`);
    }
  });

  it('the page maps getCategoryEvents null to Next notFound()', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse({}, 404)));
    const call = CategoryEventsPage({
      params: Promise.resolve({ slug: 'no-existe' }),
    }) as Promise<unknown>;
    await expect(call).rejects.toMatchObject({
      digest: 'NEXT_HTTP_ERROR_FALLBACK;404',
    });
  });

  it('passes the Spanish hyphen slug through untouched (TX-4)', async () => {
    const fetchMock = vi.fn(async () => fixtureResponse(categoryEvents));
    vi.stubGlobal('fetch', fetchMock);
    await CategoryEventsPage({
      params: Promise.resolve({ slug: 'vehiculos' }),
    });
    const [url] = fetchMock.mock.calls[0] as unknown as [string];
    expect(url).toBe('/api/v1/categories/vehiculos/events');
  });
});
