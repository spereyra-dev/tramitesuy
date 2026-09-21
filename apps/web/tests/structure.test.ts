/**
 * Structural navigation tests (T7/T8/T9, RED first — strict TDD).
 *
 * These pin the human-oriented structure of the citizen pages: a top
 * breadcrumb with the human category name and a bottom back link on the
 * event page, a bottom back link on the category-events page, per-page
 * document titles via generateMetadata, and the "Ver todos los temas"
 * link on the search-results view. Rendered with react-dom/server over
 * the recorded fixtures (no network, no DB); next/link is stubbed to
 * emit plain anchors (same pattern as categories.test.ts).
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup, renderToReadableStream } from 'react-dom/server';
import { createElement, type ReactElement, type ReactNode } from 'react';

import EventPage, {
  generateMetadata as eventMetadata,
} from '@/app/events/[slug]/page';
import CategoryEventsPage, {
  generateMetadata as categoryEventsMetadata,
} from '@/app/categories/[slug]/page';
import CategoriesPage, {
  generateMetadata as categoriesMetadata,
} from '@/app/categories/page';
import HomePage from '@/app/page';

import eventPage from './fixtures/event-page.json';
import categoryEvents from './fixtures/category-events.json';
import categories from './fixtures/categories.json';
import searchCategories from './fixtures/search-categories.json';

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

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
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

type Route = { match: string; body: unknown; status?: number };

/**
 * Route the stubbed fetch by URL fragment so one render can hit both the
 * event and the categories endpoints (breadcrumb name resolution).
 */
function routeFetch(routes: Route[]): ReturnType<typeof vi.fn> {
  return vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input);
    const route = routes.find((candidate) => url.includes(candidate.match));
    if (!route) return fixtureResponse({}, 404);
    return fixtureResponse(route.body, route.status ?? 200);
  });
}

async function renderEventPage(routes: Route[]): Promise<string> {
  vi.stubGlobal('fetch', routeFetch(routes));
  const element = await EventPage({
    params: Promise.resolve({ slug: 'comprar-vehiculo' }),
  });
  return renderToStaticMarkup(element as ReactElement);
}

async function renderCategoryEventsPage(): Promise<string> {
  vi.stubGlobal(
    'fetch',
    routeFetch([
      { match: '/api/v1/categories/', body: categoryEvents },
      { match: '/api/v1/categories', body: categories },
    ]),
  );
  const element = await CategoryEventsPage({
    params: Promise.resolve({ slug: 'vehiculos' }),
  });
  return renderToStaticMarkup(element as ReactElement);
}

describe('event page structural navigation (T7)', () => {
  it('renders a top breadcrumb nav with Inicio and the human category name before the h1', async () => {
    const html = await renderEventPage([
      { match: '/api/v1/events/', body: eventPage },
      { match: '/api/v1/categories', body: categories },
    ]);

    expect(html).toContain('aria-label="Ruta de navegación"');
    expect(html).toContain('Vehículos');
    expect(html).toContain('href="/"');
    expect(html).toContain('href="/categories/vehiculos"');
    const nav = html.indexOf('Ruta de navegación');
    const h1 = html.indexOf('<h1');
    expect(nav).toBeGreaterThanOrEqual(0);
    expect(h1).toBeGreaterThan(nav);
  });

  it('renders a bottom back link to the category after the procedures section', async () => {
    const html = await renderEventPage([
      { match: '/api/v1/events/', body: eventPage },
      { match: '/api/v1/categories', body: categories },
    ]);

    expect(html).toContain('← Volver a Vehículos');
    const procedures = html.indexOf('event-procedures');
    const back = html.indexOf('Volver a Vehículos');
    expect(procedures).toBeGreaterThanOrEqual(0);
    expect(back).toBeGreaterThan(procedures);
  });

  it('falls back to neutral copy when the category name cannot be resolved, never breaking the page', async () => {
    const html = await renderEventPage([
      { match: '/api/v1/events/', body: eventPage },
      { match: '/api/v1/categories', body: {}, status: 500 },
    ]);

    expect(html).toContain('Comprar un vehículo');
    expect(html).toContain('aria-label="Ruta de navegación"');
    expect(html).toContain('href="/categories/vehiculos"');
    expect(html).toContain('← Volver a la categoría');
  });
});

describe('category events page bottom back link (T7)', () => {
  it('keeps the pinned top link and adds a bottom back link to /categories', async () => {
    const html = await renderCategoryEventsPage();

    expect(html).toContain('Volver a categorías');
    expect(html).toContain('← Volver a categorías');
    expect(html).toContain('href="/categories"');
    const list = html.indexOf('option-list--events');
    const bottom = html.lastIndexOf('Volver a categorías');
    expect(list).toBeGreaterThanOrEqual(0);
    expect(bottom).toBeGreaterThan(list);
  });
});

describe('per-page document titles (T8)', () => {
  it('titles the event page with its human name', async () => {
    vi.stubGlobal(
      'fetch',
      routeFetch([
        { match: '/api/v1/events/', body: eventPage },
        { match: '/api/v1/categories', body: categories },
      ]),
    );
    const meta = await eventMetadata({
      params: Promise.resolve({ slug: 'comprar-vehiculo' }),
    });
    expect(meta.title).toBe('Comprar un vehículo — TrámitesUY');
  });

  it('titles the category events page with the human category name', async () => {
    vi.stubGlobal(
      'fetch',
      routeFetch([{ match: '/api/v1/categories', body: categories }]),
    );
    const meta = await categoryEventsMetadata({
      params: Promise.resolve({ slug: 'vehiculos' }),
    });
    expect(meta.title).toBe('Vehículos — TrámitesUY');
  });

  it('titles the categories index page with a fixed Spanish title', async () => {
    const meta = await categoriesMetadata();
    expect(meta.title).toBe('Categorías — TrámitesUY');
  });

  it('metadata never throws: API failures fall back to slug-based titles', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => fixtureResponse({}, 500)),
    );
    const event = await eventMetadata({
      params: Promise.resolve({ slug: 'comprar-vehiculo' }),
    });
    expect(event.title).toBe('comprar-vehiculo — TrámitesUY');
    const category = await categoryEventsMetadata({
      params: Promise.resolve({ slug: 'vehiculos' }),
    });
    expect(category.title).toBe('vehiculos — TrámitesUY');
  });
});

describe('Ver todos los temas link on search results (T9)', () => {
  it('links the search-results view to /categories with the exact copy', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(searchCategories)));
    const element = await HomePage({
      searchParams: Promise.resolve({ q: 'vehiculo' }),
    });
    const stream = await renderToReadableStream(element as ReactElement);
    await stream.allReady;
    const html = await new Response(stream).text();

    expect(html).toContain('Ver todos los temas');
    expect(html).toContain('href="/categories"');
  });
});
