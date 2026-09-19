/**
 * Event-page tests (task 17, RED first — strict TDD).
 *
 * These render the /events/[slug] server component with renderToStaticMarkup
 * (react-dom, already a dependency — no testing-library, no browser) over the
 * fixture payloads recorded from the shipped handlers (design §5): the full
 * event page, the empty-procedures event, and the null-official_url card.
 * No network, no DB: fetch is stubbed and the request stays relative
 * /api/v1/... (same-origin proxy requirement).
 */
import { readFileSync } from 'node:fs';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { createElement } from 'react';

import EventPage from '@/app/events/[slug]/page';
import {
  EMPTY_PROCEDURES_COPY,
  SOURCE_LINK_UNAVAILABLE_COPY,
} from '@/lib/display';

import eventPage from './fixtures/event-page.json';
import eventPageEmpty from './fixtures/event-page-empty.json';
import eventPageNullUrl from './fixtures/event-page-null-url.json';

vi.mock('next/headers', () => ({
  headers: vi.fn(async () => {
    throw new Error('headers was called outside a request scope');
  }),
}));

beforeEach(() => {
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

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

function renderEvent(slug: string, body: unknown): string {
  vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse(body)));
  const element = EventPage({
    params: Promise.resolve({ slug }),
  }) as Promise<React.ReactElement>;
  return renderToStaticMarkup(element);
}

describe('event page: full payload (event-page.json)', () => {
  const html = renderEvent('comprar-vehiculo', eventPage);

  it('renders the event name, description, and category', () => {
    expect(html).toContain('Comprar un vehículo');
    expect(html).toContain(
      'Requisitos y trámites para comprar un vehículo nuevo o usado en Uruguay.',
    );
    expect(html).toContain('vehiculos');
  });

  it('renders the procedure cards in API order, not array order', () => {
    const shuffled = {
      ...eventPage,
      procedures: [
        eventPage.procedures[2],
        eventPage.procedures[0],
        eventPage.procedures[1],
      ],
    };
    const reordered = renderEvent('comprar-vehiculo', shuffled);
    const first = reordered.indexOf('Solicitud de empadronamientos');
    const second = reordered.indexOf('Alta de vehículos ante la Dirección Nacional de Transporte (DNT)');
    const third = reordered.indexOf('Registro de Automotoras o Gestoría para Empadronamiento de Vehículos');
    expect(first).toBeGreaterThanOrEqual(0);
    expect(second).toBeGreaterThan(first);
    expect(third).toBeGreaterThan(second);
  });

  it('shows the required flag on every card', () => {
    expect(html).toContain('Obligatorio');
    expect(html).toContain('Opcional');
  });

  it('keeps the per-card attribution block and verbatim cost_display', () => {
    expect(html).toContain('Sin costo informado');
    expect(html).toContain('Fuente oficial');
    expect(html).toContain('Actualizado:');
  });
});

describe('event page: pinned empty state (event-page-empty.json)', () => {
  it('renders exactly the pinned copy instead of an empty card list', () => {
    const html = renderEvent('fixture-sin-tramites', eventPageEmpty);
    expect(html).toContain(EMPTY_PROCEDURES_COPY);
    expect(html).not.toContain('procedure-card');
  });
});

describe('event page: null official_url (event-page-null-url.json)', () => {
  const html = renderEvent('fixture-sin-url', eventPageNullUrl);

  it('renders no link element at all', () => {
    expect(html).not.toContain('<a ');
  });

  it('shows the explicit source-link-unavailable state', () => {
    expect(html).toContain(SOURCE_LINK_UNAVAILABLE_COPY);
  });

  it('keeps the attribution block intact', () => {
    expect(html).toContain('Fuente oficial');
    expect(html).toContain('Catálogo de trámites y servicios del Estado — AGESIC');
    expect(html).toContain('Actualizado:');
  });
});

describe('event page: unknown slug renders not-found (spec scenario 3)', () => {
  it('the page maps getEvent null to Next notFound()', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => fixtureResponse({}, 404)));
    const call = EventPage({
      params: Promise.resolve({ slug: 'no-existe' }),
    }) as Promise<unknown>;
    await expect(call).rejects.toMatchObject({
      digest: 'NEXT_HTTP_ERROR_FALLBACK;404',
    });
  });
});

describe('event page: Spanish hyphen slug passed through untouched (TX-4)', () => {
  it('requests the slug exactly as emitted, with no transliteration', async () => {
    const fetchMock = vi.fn(async () => fixtureResponse(eventPage));
    vi.stubGlobal('fetch', fetchMock);
    await EventPage({ params: Promise.resolve({ slug: 'comprar-vehiculo' }) });
    const [url] = fetchMock.mock.calls[0] as unknown as [string];
    expect(url).toBe('/api/v1/events/comprar-vehiculo');
  });
});
