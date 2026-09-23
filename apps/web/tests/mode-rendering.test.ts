/**
 * Mode-rendering tests (task 10, RED first — strict TDD).
 *
 * These exercise the pure render-decision logic the home page uses for the
 * three search modes (spec: "Three-mode search rendering" ×3), over the
 * fixture payloads recorded from the shipped handlers (design §5). No
 * browser, no DOM, no network.
 */
import { readFileSync } from 'node:fs';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderToReadableStream } from 'react-dom/server';
import { createElement, type ReactNode } from 'react';

import HomePage from '@/app/page';

import type { SearchResponse } from '@/lib/api';
import { searchView } from '@/lib/search-view';

import searchOpen from './fixtures/search-open.json';
import searchDisambiguation from './fixtures/search-disambiguation.json';
import searchCategories from './fixtures/search-categories.json';

type OpenResponse = Extract<SearchResponse, { mode: 'open' }>;
type DisambiguationResponse = Extract<SearchResponse, { mode: 'disambiguation' }>;
type CategoriesResponse = Extract<SearchResponse, { mode: 'categories' }>;

const open = searchOpen as unknown as OpenResponse;
const disambiguation = searchDisambiguation as unknown as DisambiguationResponse;
const categories = searchCategories as unknown as CategoriesResponse;

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

function fixtureResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'content-type': 'application/json' },
  });
}

async function renderSearchMode(body: SearchResponse): Promise<string> {
  vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(body)));
  const element = await HomePage({ searchParams: Promise.resolve({ q: body.query }) });
  const stream = await renderToReadableStream(element);
  await stream.allReady;
  return new Response(stream).text();
}

describe('home search result rendering', () => {
  it('compacts only the searched hero and identifies results as an announced region', async () => {
    const searched = await renderSearchMode(open);
    expect(searched).toContain('class="home-hero home-hero--searched"');
    expect(searched).toMatch(/<section[^>]*aria-labelledby="search-results-heading"[^>]*aria-live="polite"/);
    expect(searched.indexOf('search-results--direct')).toBeGreaterThan(searched.indexOf('home-hero--searched'));
    expect(searched).toContain('tabindex="-1"');
    const css = readFileSync(new URL('../app/globals.css', import.meta.url), 'utf8');
    expect(css).toMatch(/\.home-hero--searched\s*\{[^}]*padding:\s*1rem/s);
    expect(css).toMatch(/\.home-hero--searched \.home-intro,[^}]*display:\s*none/s);
    vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse({ categories: [] })));
    const home = await HomePage({ searchParams: Promise.resolve({}) });
    const stream = await renderToReadableStream(home);
    await stream.allReady;
    const homeHtml = await new Response(stream).text();
    expect(homeHtml).toContain('class="home-hero"');
    expect(homeHtml).not.toContain('home-hero--searched');
  });
  it('gives each returned search mode a named, visually targetable result region while retaining official attribution', async () => {
    const openHtml = await renderSearchMode(open);
    const disambiguationHtml = await renderSearchMode(disambiguation);
    const categoriesHtml = await renderSearchMode(categories);

    expect(openHtml).toContain('class="search-results search-results--direct"');
    expect(openHtml).toContain('Fuente oficial');
    expect(disambiguationHtml).toContain('class="search-results search-results--disambiguation"');
    expect(disambiguationHtml).toContain('¿Te referías a...?');
    expect(categoriesHtml).toContain('class="search-results search-results--categories"');
    expect(categoriesHtml).toContain('Explorá por categoría');
  });
});

describe('searchView: open mode', () => {
  it('renders the result event name with a link to its event page and no redirect', () => {
    const view = searchView(open);
    expect(view.kind).toBe('open');
    if (view.kind !== 'open') throw new Error('expected open branch');
    expect(view.eventName).toBe('Comprar un vehículo');
    expect(view.eventHref).toBe('/events/comprar-vehiculo');
    // Open mode renders inline on /?q= — there is no redirect to make.
    expect(view).not.toHaveProperty('redirect');
  });

  it('exposes the response confidence', () => {
    const view = searchView(open);
    if (view.kind !== 'open') throw new Error('expected open branch');
    expect(view.confidence).toBe(open.confidence);
  });

  it('returns procedure cards in API order (sorted by the card order field)', () => {
    const view = searchView(open);
    if (view.kind !== 'open') throw new Error('expected open branch');
    expect(view.procedures.map((card) => card.name)).toEqual([
      'Solicitud de empadronamientos',
      'Alta de vehículos ante la Dirección Nacional de Transporte (DNT)',
      'Registro de Automotoras o Gestoría para Empadronamiento de Vehículos',
    ]);
  });
});

describe('searchView: disambiguation mode', () => {
  it('heads the options with the exact copy ¿Te referías a...?', () => {
    const view = searchView(disambiguation);
    expect(view.kind).toBe('disambiguation');
    if (view.kind !== 'disambiguation') throw new Error('expected disambiguation branch');
    expect(view.heading).toBe('¿Te referías a...?');
  });

  it('links every option to its event page with none presented as the answer', () => {
    const view = searchView(disambiguation);
    if (view.kind !== 'disambiguation') throw new Error('expected disambiguation branch');
    expect(view.options).toHaveLength(disambiguation.options.length);
    expect(view.options.map((o) => o.href)).toEqual([
      '/events/perder-libreta',
      '/events/cambiar-matricula',
    ]);
    expect(view.options.map((o) => o.name)).toEqual([
      'Perder la libreta de conducir',
      'Cambiar la matrícula',
    ]);
    // No option is presented as the selected answer: the view carries no
    // selected/chosen marker at all.
    expect(view).not.toHaveProperty('selected');
    expect(view).not.toHaveProperty('answer');
  });
});

describe('searchView: categories mode', () => {
  it('renders the returned categories as links to their category pages', () => {
    const view = searchView(categories);
    expect(view.kind).toBe('categories');
    if (view.kind !== 'categories') throw new Error('expected categories branch');
    expect(view.categories.map((c) => c.name)).toEqual(['Vehículos']);
    expect(view.categories.map((c) => c.href)).toEqual(['/categories/vehiculos']);
  });
});

describe('searchView: open branch triangulation (task 16)', () => {
  it('renders cards in API order even when the array arrives shuffled', () => {
    const shuffled: OpenResponse = {
      ...open,
      results: [
        {
          ...open.results[0],
          // Same cards, different array positions than their order values.
          procedures: [
            open.results[0].procedures[2],
            open.results[0].procedures[0],
            open.results[0].procedures[1],
          ],
        },
      ],
    };
    const view = searchView(shuffled);
    if (view.kind !== 'open') throw new Error('expected open branch');
    expect(view.procedures.map((card) => card.order)).toEqual([1, 2, 3]);
    expect(view.procedures.map((card) => card.name)).toEqual([
      'Solicitud de empadronamientos',
      'Alta de vehículos ante la Dirección Nacional de Transporte (DNT)',
      'Registro de Automotoras o Gestoría para Empadronamiento de Vehículos',
    ]);
  });

  it('keeps the SearchResponse union narrowed: reading options on an open response is a compile error', () => {
    if (open.mode === 'open') {
      // @ts-expect-error options does not exist on the open arm
      void open.options;
    }
  });
});
