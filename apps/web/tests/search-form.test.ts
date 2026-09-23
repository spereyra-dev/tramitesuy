import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { createElement } from 'react';

import { SearchForm, SearchResultsHeading } from '@/components/SearchForm';

const hooks = vi.hoisted(() => {
  let state = '';
  let initialized = false;
  let pending = false;
  let previousDependency: string | undefined;
  let pendingEffect: (() => void) | undefined;
  let focusRef = { current: false };
  const push = vi.fn();
  return {
    push,
    reset() { state = ''; initialized = false; pending = false; previousDependency = undefined; pendingEffect = undefined; focusRef = { current: false }; push.mockClear(); },
    setPending(value: boolean) { pending = value; },
    state() { return state; },
    useState(initial: string) {
      if (!initialized) { state = initial; initialized = true; }
      return [state, (value: string) => { state = value; }] as const;
    },
    useTransition() { return [pending, (callback: () => void) => callback()] as const; },
    useRef() { return focusRef; },
    useCallback<T extends (...args: never[]) => void>(callback: T) { return callback; },
    useEffect(effect: () => void, dependencies: string[]) {
      if (previousDependency !== dependencies[0]) pendingEffect = effect;
      previousDependency = dependencies[0];
    },
    flush() { pendingEffect?.(); pendingEffect = undefined; },
  };
});

vi.mock('react', async (importOriginal) => ({
  ...await importOriginal<typeof import('react')>(),
  useState: hooks.useState,
  useEffect: hooks.useEffect,
  useTransition: hooks.useTransition,
  useRef: hooks.useRef,
  useCallback: hooks.useCallback,
}));
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: hooks.push }),
}));

beforeEach(() => hooks.reset());
afterEach(() => vi.unstubAllGlobals());

describe('search form', () => {
  it('tracks Back and Forward while leaving unsubmitted typing intact', () => {
    const input = (query: string) => {
      const form = SearchForm({ initialQuery: query });
      const controls = (form.props.children as React.ReactElement[])[2] as React.ReactElement<{ children: React.ReactElement[] }>;
      return (controls.props.children as React.ReactElement[])[0] as React.ReactElement<{
        value: string; onChange: (event: { target: { value: string } }) => void;
      }>;
    };
    expect(input('primera').props.value).toBe('primera');
    input('primera').props.onChange({ target: { value: 'borrador' } });
    expect(input('primera').props.value).toBe('borrador');
    input('segunda'); hooks.flush();
    expect(input('segunda').props.value).toBe('segunda');
    input('primera'); hooks.flush(); // Back
    expect(input('primera').props.value).toBe('primera');
    input('segunda'); hooks.flush(); // Forward
    expect(input('segunda').props.value).toBe('segunda');
  });

  it('focuses the heading when rendered results mount, not on an unrelated re-render', () => {
    const focus = vi.fn();
    SearchForm({ initialQuery: 'auto' }); hooks.flush();
    expect(focus).not.toHaveBeenCalled(); // The async region has not rendered yet.
    const heading = SearchResultsHeading({ query: 'auto', children: 'Encontramos una situación' });
    const node = { focus };
    heading.props.ref(node); // Results now exist.
    expect(focus).toHaveBeenCalledOnce();
    SearchResultsHeading({ query: 'auto', children: 'Encontramos una situación' }).props.ref(node);
    expect(focus).toHaveBeenCalledOnce();
    hooks.reset(); // A distinct, keyed query mounts a new heading.
    SearchResultsHeading({ query: 'otro', children: 'Explorá por categoría' }).props.ref(node);
    expect(focus).toHaveBeenCalledTimes(2);
  });

  it('announces pending navigation and disables duplicate submission', () => {
    hooks.setPending(true);
    const html = renderToStaticMarkup(createElement(SearchForm, { initialQuery: 'auto' }));
    expect(html).toContain('role="status"');
    expect(html).toContain('Buscando trámites');
    expect(html).toContain('disabled=""');
  });

  it('submits 512 Unicode scalars even when UTF-16 length exceeds 512', () => {
    const query = 'a'.repeat(511) + '😀';
    const form = SearchForm({ initialQuery: query });
    form.props.onSubmit({ preventDefault: vi.fn() });
    expect(hooks.push).toHaveBeenCalledWith(`/?q=${encodeURIComponent(query)}`);
    const html = renderToStaticMarkup(createElement(SearchForm, { initialQuery: query }));
    expect(html).not.toContain('maxlength='); // Native maxLength counts UTF-16, not API scalars.
    expect(html).not.toContain('aria-invalid="true"');
  });

  it('shows the shared 512-character limit and refuses an overlong submission', () => {
    const form = SearchForm({ initialQuery: 'x'.repeat(513) });
    const event = { preventDefault: vi.fn() };
    form.props.onSubmit(event);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(hooks.push).not.toHaveBeenCalled();
    const html = renderToStaticMarkup(createElement(SearchForm, { initialQuery: 'x'.repeat(513) }));
    expect(html).toContain('512');
    expect(html).toContain('aria-invalid="true"');
  });
  it('provides a visible Spanish label, instructional help, and a submit button', () => {
    const html = renderToStaticMarkup(createElement(SearchForm));

    expect(html).toContain('<label');
    expect(html).toContain('Describí tu situación');
    expect(html).toContain('id="search-guidance"');
    expect(html).toContain('aria-describedby="search-guidance search-limit"');
    expect(html).toContain('required=""');
    expect(html).toContain('type="submit"');
    expect(html).toContain('Buscar trámites');
  });
});
