'use client';

import { useRouter } from 'next/navigation';
import { useCallback, useEffect, useRef, useState, useTransition } from 'react';

import { SEARCH_QUERY_MAX_CHARS, searchQueryTooLong } from '@/lib/display';

/**
 * The single client-interactive element in the app (task 13 GREEN, design
 * §2.4): a search box whose submit navigates the server-rendered home page
 * to /?q={query}. It performs NO data fetching — every API call belongs to
 * the server component that renders the results.
 */
export function SearchForm({ initialQuery = '' }: { initialQuery?: string }) {
  const router = useRouter();
  const [query, setQuery] = useState(initialQuery);
  const [pending, startTransition] = useTransition();
  useEffect(() => {
    setQuery(initialQuery);
  }, [initialQuery]);

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed.length === 0 || searchQueryTooLong(trimmed)) return;
    startTransition(() => router.push(`/?q=${encodeURIComponent(trimmed)}`));
  }

  return (
    <form className="search-form" role="search" onSubmit={handleSubmit}>
      <label className="search-label" htmlFor="situacion">
        Describí tu situación
      </label>
      <p className="search-guidance" id="search-guidance">
        Escribí qué pasó o qué necesitás hacer. Por ejemplo: “compré un auto usado”.
      </p>
      <div className="search-controls">
        <input
          id="situacion"
          name="q"
          type="search"
          required
          aria-describedby="search-guidance search-limit"
          aria-invalid={searchQueryTooLong(query.trim()) || undefined}
          placeholder="Ej.: compré un auto usado"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="submit" disabled={pending || searchQueryTooLong(query.trim())}>
          {pending ? 'Buscando trámites…' : 'Buscar trámites'}
        </button>
      </div>
      <p className="search-limit" id="search-limit">Máximo {SEARCH_QUERY_MAX_CHARS} caracteres.</p>
      {pending && <p role="status" aria-live="polite">Buscando trámites…</p>}
    </form>
  );
}

/** Mounted by each streamed results heading, never by the earlier search form. */
export function SearchResultsHeading({ query, children }: { query: string; children: React.ReactNode }) {
  const focused = useRef(false);
  const onMount = useCallback((node: HTMLHeadingElement | null) => {
    if (node && !focused.current) {
      focused.current = true;
      node.focus();
    }
  }, []);

  return <h2 id="search-results-heading" tabIndex={-1} ref={onMount} data-query={query}>{children}</h2>;
}
