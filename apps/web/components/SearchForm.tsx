'use client';

import { useRouter } from 'next/navigation';
import { useState } from 'react';

/**
 * The single client-interactive element in the app (task 13 GREEN, design
 * §2.4): a search box whose submit navigates the server-rendered home page
 * to /?q={query}. It performs NO data fetching — every API call belongs to
 * the server component that renders the results.
 */
export function SearchForm({ initialQuery = '' }: { initialQuery?: string }) {
  const router = useRouter();
  const [query, setQuery] = useState(initialQuery);

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = query.trim();
    if (trimmed.length === 0) return; // the UI never submits an empty query
    router.push(`/?q=${encodeURIComponent(trimmed)}`);
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
          aria-describedby="search-guidance"
          placeholder="Ej.: compré un auto usado"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <button type="submit">Buscar trámites</button>
      </div>
    </form>
  );
}
