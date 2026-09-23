/**
 * Home category-tile tests (task 5, RED first — strict TDD).
 *
 * The no-query home must show large citizen-first category tiles fetched
 * server-side, never fail with a 500 when the API is down, and never change
 * the search-first hero. Rendered with renderToStaticMarkup over a fixture;
 * internal links go through a next/link stub emitting plain anchors (same
 * pattern as categories.test.ts).
 */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToReadableStream } from 'react-dom/server';
import { createElement, type ReactElement, type ReactNode } from 'react';

import HomePage from '@/app/page';

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

async function renderHome(body: unknown, status = 200): Promise<string> {
  vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(body, status)));
  const element = await HomePage({ searchParams: Promise.resolve({}) });
  const stream = await renderToReadableStream(element as ReactElement);
  await stream.allReady;
  return new Response(stream).text();
}

const shuffledCategories = {
  categories: [
    { slug: 'salud', name: 'Salud', order_index: 2 },
    { slug: 'trabajo', name: 'Trabajo', order_index: 3 },
    { slug: 'vehiculos', name: 'Vehículos', order_index: 1 },
  ],
};

describe('home category tiles (no query)', () => {
  it('renders a labelled home-categories section with a "Trámites por tema" heading and Spanish intro copy', async () => {
    const html = await renderHome(shuffledCategories);

    expect(html).toContain('class="home-categories"');
    expect(html).toContain('aria-labelledby');
    expect(html).toContain('Trámites por tema');
    expect(html).toContain(
      'Si no sabés qué buscar, elegí un tema y mirá los trámites oficiales.',
    );
  });

  it('renders every category as a link to its category page, in order_index ascending', async () => {
    const html = await renderHome(shuffledCategories);

    expect(html).toContain('href="/categories/vehiculos"');
    expect(html).toContain('href="/categories/salud"');
    expect(html).toContain('href="/categories/trabajo"');

    const vehiculos = html.indexOf('/categories/vehiculos');
    const salud = html.indexOf('/categories/salud');
    const trabajo = html.indexOf('/categories/trabajo');
    expect(vehiculos).toBeGreaterThanOrEqual(0);
    expect(salud).toBeGreaterThan(vehiculos);
    expect(trabajo).toBeGreaterThan(salud);
  });

  it('gives each tile a short Spanish subline instead of icons or emoji', async () => {
    const html = await renderHome(shuffledCategories);
    expect(html).toContain('Ver trámites');
  });

  it('falls back to the simple explore prompt instead of 500ing when the categories API fails', async () => {
    const html = await renderHome({ error: 'boom' }, 500);

    expect(html).toContain('browse-prompt');
    expect(html).toContain('href="/categories"');
    // The search-first hero stays intact regardless of API health.
    expect(html).toContain('¿Qué trámite necesitás hacer?');
    expect(html).toContain('No ingreses datos personales (cédulas, teléfonos ni correos electrónicos).');
    expect(html.match(/class="browse-prompt"/g)).toHaveLength(1);
  });
});
