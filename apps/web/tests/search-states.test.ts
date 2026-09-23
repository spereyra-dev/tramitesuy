import { afterEach, describe, expect, it, vi } from 'vitest';
import { createElement } from 'react';
import { renderToReadableStream, renderToStaticMarkup } from 'react-dom/server';

import HomePage from '@/app/page';
import Loading from '@/app/loading';
import ErrorPage from '@/app/error';

vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));
vi.mock('next/headers', () => ({ headers: vi.fn(async () => { throw new Error('no request'); }) }));
vi.mock('next/link', () => ({ default: (props: { href: string; children: string }) => createElement('a', { href: props.href }, props.children) }));

afterEach(() => vi.unstubAllGlobals());

async function renderedSearch(query: string, status: number): Promise<string> {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status })));
  const page = await HomePage({ searchParams: Promise.resolve({ q: query }) });
  const stream = await renderToReadableStream(page);
  await stream.allReady;
  return new Response(stream).text();
}

describe('search pending and recoverable failures', () => {
  it('renders a distinct 400 response without throwing or claiming no results', async () => {
    const html = await renderedSearch('auto', 400);
    expect(html).toContain('La búsqueda no es válida');
    expect(html).not.toContain('No encontramos una coincidencia');
    expect(html).not.toContain('Explorá por categoría');
  });

  it('rejects an oversized URL query before requesting the API', async () => {
    const html = await renderedSearch('x'.repeat(513), 200);
    expect(html).toContain('La búsqueda no es válida');
    expect(fetch).not.toHaveBeenCalled();
  });

  it('passes 512 astral scalars (2048 UTF-8 bytes) to the API, but rejects 513', async () => {
    await renderedSearch('😀'.repeat(512), 400);
    expect(fetch).toHaveBeenCalledOnce();
    await renderedSearch('😀'.repeat(513), 200);
    expect(fetch).not.toHaveBeenCalled();
  });

  it('shows a service outage with a same-query retry rather than an empty search', async () => {
    const html = await renderedSearch('auto usado', 503);
    expect(html).toContain('No pudimos consultar los trámites');
    expect(html).toContain('name="q"');
    expect(html).toContain('value="auto usado"');
    expect(html).toContain('Reintentar búsqueda');
    expect(html).not.toContain('No encontramos una coincidencia');
  });

  it('offers route-level pending and error fallbacks with recovery', () => {
    expect(renderToStaticMarkup(createElement(Loading))).toContain('Buscando trámites');
    const html = renderToStaticMarkup(createElement(ErrorPage, { error: new Error('down'), reset: vi.fn() }));
    expect(html).toContain('Reintentar');
    expect(html).not.toContain('down');
  });
});
