// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createElement } from 'react';
import { renderToReadableStream } from 'react-dom/server';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';

import HomePage from '@/app/page';
import ErrorPage from '@/app/error';
import { SearchForm, SearchResultsHeading } from '@/components/SearchForm';

// Next's router and request headers need a running Next request. All React
// state, refs, effects, form events, and DOM focus remain real in these tests.
const navigation = vi.hoisted(() => ({ push: vi.fn() }));
vi.mock('next/navigation', () => ({ useRouter: () => navigation }));
vi.mock('next/headers', () => ({ headers: vi.fn(async () => { throw new Error('no request'); }) }));
vi.mock('next/link', () => ({ default: (props: { href: string; children: string }) => createElement('a', { href: props.href }, props.children) }));

afterEach(() => {
  cleanup();
  navigation.push.mockClear();
  vi.unstubAllGlobals();
});

function searchInput(): HTMLInputElement {
  return screen.getByRole('searchbox', { name: 'Describí tu situación' }) as HTMLInputElement;
}

describe('real DOM search interactions', () => {
  it('follows Back and Forward query props in the mounted input', async () => {
    const { rerender } = render(createElement(SearchForm, { initialQuery: 'primera' }));
    expect(searchInput().value).toBe('primera');
    rerender(createElement(SearchForm, { initialQuery: 'segunda' }));
    await waitFor(() => expect(searchInput().value).toBe('segunda'));
    rerender(createElement(SearchForm, { initialQuery: 'primera' }));
    await waitFor(() => expect(searchInput().value).toBe('primera'));
    rerender(createElement(SearchForm, { initialQuery: 'segunda' }));
    await waitFor(() => expect(searchInput().value).toBe('segunda'));
  });

  it('preserves unsubmitted typing, focus, and caret on an unchanged query prop', () => {
    const { rerender } = render(createElement(SearchForm, { initialQuery: 'auto' }));
    const input = searchInput();
    input.focus();
    fireEvent.change(input, { target: { value: 'auto usado' } });
    input.setSelectionRange(4, 4);
    fireEvent.change(input, { target: { value: 'autoX usado', selectionStart: 5, selectionEnd: 5 } });
    expect(input.value).toBe('autoX usado');
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(5);
    rerender(createElement(SearchForm, { initialQuery: 'auto' }));
    expect(searchInput()).toBe(input);
    expect(input.value).toBe('autoX usado');
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(5);
  });

  it('focuses newly mounted results but never steals focus on an unchanged result rerender', () => {
    const results = (query: string) => createElement('section', { 'aria-label': 'Resultados' },
      createElement(SearchResultsHeading, { key: query, query, children: 'Encontramos una situación relacionada' }));
    const { rerender } = render(createElement('div', null, createElement(SearchForm, { initialQuery: 'auto' })));
    const input = searchInput();
    input.focus();
    expect(document.activeElement).toBe(input);
    fireEvent.submit(screen.getByRole('search'));
    expect(navigation.push).toHaveBeenCalledWith('/?q=auto');
    rerender(createElement('div', null, createElement(SearchForm, { initialQuery: 'auto' }), results('auto')));
    const heading = screen.getByRole('heading', { name: 'Encontramos una situación relacionada' });
    expect(document.activeElement).toBe(heading);
    input.focus();
    rerender(createElement('div', null, createElement(SearchForm, { initialQuery: 'auto' }), results('auto')));
    expect(document.activeElement).toBe(input);
    rerender(createElement('div', null, createElement(SearchForm, { initialQuery: 'otro' }), results('otro')));
    expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Encontramos una situación relacionada' }));
  });

  it('submits the same query from the outage retry button', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('{}', { status: 503 })));
    const page = await HomePage({ searchParams: Promise.resolve({ q: 'auto usado' }) });
    const stream = await renderToReadableStream(page);
    await stream.allReady;
    const container = document.createElement('div');
    container.innerHTML = await new Response(stream).text();
    document.body.append(container);
    try {
      const button = screen.getByRole('button', { name: 'Reintentar búsqueda' });
      const form = button.closest('form');
      expect(form).not.toBeNull();
      const submit = vi.fn((event: Event) => event.preventDefault());
      form!.addEventListener('submit', submit);
      fireEvent.click(button);
      expect(submit).toHaveBeenCalledOnce();
      expect(form!.method).toBe('get');
      expect(form!.getAttribute('action')).toBe('/');
      expect(new FormData(form!).get('q')).toBe('auto usado');
    } finally {
      container.remove();
    }
  });

  it('calls the route error reset exactly once when retry is clicked', () => {
    const reset = vi.fn();
    render(createElement(ErrorPage, { error: new Error('down'), reset }));
    fireEvent.click(screen.getByRole('button', { name: 'Reintentar' }));
    expect(reset).toHaveBeenCalledOnce();
  });
});
