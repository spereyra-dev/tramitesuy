/**
 * Search-first home: it reads searchParams.q, calls search() through the
 * same-origin proxy, and renders the matching response on this page. It does
 * not redirect, fetch in the browser, or alter the API view-model contract.
 */
import Link from 'next/link';

import { ProcedureCard } from '@/components/ProcedureCard';
import { SearchForm, SearchResultsHeading } from '@/components/SearchForm';
import { getCategories, search, SearchApiError } from '@/lib/api';
import { EMPTY_PROCEDURES_COPY, SEARCH_QUERY_MAX_CHARS, searchQueryTooLong } from '@/lib/display';
import { searchView } from '@/lib/search-view';
import type { SearchView } from '@/lib/search-view';

export default async function HomePage({
  searchParams,
}: {
  searchParams: Promise<{ q?: string | string[] }>;
}) {
  const params = await searchParams;
  const rawQuery = Array.isArray(params.q) ? params.q[0] : params.q;
  const query = rawQuery?.trim();

  return (
    <div className="home-page">
      <header className={query ? 'home-hero home-hero--searched' : 'home-hero'}>
        <p className="home-eyebrow">Orientación para trámites del Estado</p>
        <h1>¿Qué trámite necesitás hacer?</h1>
        <p className="home-intro">
          Contanos tu situación y te mostramos los trámites oficiales que pueden
          aplicar.
        </p>
        <SearchForm initialQuery={query ?? ''} />
        <p className="home-reassurance">
          Buscá con tus palabras. Siempre vas a ver la fuente oficial de cada
          trámite.
        </p>
      </header>
      {query ? <SearchResults query={query} /> : <CategoryTiles />}
    </div>
  );
}

/**
 * Citizen-first category tiles for the no-query home. getCategories() throws
 * on API failure, so the fetch is wrapped: the home must never 500, it falls
 * back to the simple explore prompt instead.
 */
async function CategoryTiles() {
  let categories: Awaited<ReturnType<typeof getCategories>>['categories'] = [];
  let failed = false;
  try {
    categories = (await getCategories()).categories;
  } catch {
    failed = true;
  }
  if (failed || categories.length === 0) return <BrowseCategoriesPrompt />;

  const tiles = [...categories].sort((a, b) => a.order_index - b.order_index);

  return (
    <section className="home-categories" aria-labelledby="home-categories-heading">
      <p className="section-kicker">También podés explorar</p>
      <h2 id="home-categories-heading">Trámites por tema</h2>
      <p>
        Si no sabés qué buscar, elegí un tema y mirá los trámites oficiales.
      </p>
      <ul className="home-categories__grid">
        {tiles.map((category) => (
          <li key={category.slug}>
            <Link className="home-categories__tile" href={`/categories/${category.slug}`}>
              <span className="home-categories__name">{category.name}</span>
              <span className="home-categories__hint">Ver trámites</span>
            </Link>
          </li>
        ))}
      </ul>
      <p className="home-categories__all">
        <Link className="text-link" href="/categories">
          Ver todos los temas
        </Link>
      </p>
    </section>
  );
}

function BrowseCategoriesPrompt() {
  return (
    <section className="browse-prompt" aria-labelledby="browse-heading">
      <p className="section-kicker">También podés explorar</p>
      <h2 id="browse-heading">Empezá por una categoría</h2>
      <p>
        Si todavía no sabés qué buscar, recorré los trámites agrupados por tema.
      </p>
      <Link className="text-link" href="/categories">
        Explorar categorías
      </Link>
    </section>
  );
}

async function SearchResults({ query }: { query: string }) {
  if (searchQueryTooLong(query)) return <SearchFailure query={query} invalid />;
  let response: Awaited<ReturnType<typeof search>>;
  try {
    response = await search(query);
  } catch (error) {
    if (error instanceof SearchApiError) {
      return <SearchFailure query={query} invalid={error.kind === 'bad-request'} />;
    }
    throw error;
  }
  const view = searchView(response);

  if (view.kind === 'open') return <OpenResult query={query} view={view} />;
  if (view.kind === 'disambiguation') {
    return <Disambiguation query={query} view={view} />;
  }
  return <Categories query={query} view={view} />;
}

function SearchFailure({ query, invalid }: { query: string; invalid: boolean }) {
  return (
    <section className="search-results search-results--error" aria-labelledby="search-results-heading" role="region" aria-live="polite">
      <SearchResultsHeading key={query} query={query}>
        {invalid ? 'La búsqueda no es válida' : 'No pudimos consultar los trámites'}
      </SearchResultsHeading>
      <p>{invalid ? `Probá con una consulta de hasta ${SEARCH_QUERY_MAX_CHARS} caracteres.` : 'El servicio no está disponible por ahora. Tu búsqueda se conserva.'}</p>
      {!invalid && (
        <form method="get" action="/">
          <input type="hidden" name="q" value={query} />
          <button className="retry-button" type="submit">Reintentar búsqueda</button>
        </form>
      )}
    </section>
  );
}

function OpenResult({
  query,
  view,
}: {
  query: string;
  view: Extract<SearchView, { kind: 'open' }>;
}) {
  return (
    <section
      className="search-results search-results--direct"
      aria-labelledby="search-results-heading" role="region" aria-live="polite"
    >
      <div className="result-heading">
        <p className="section-kicker">Resultado para “{query}”</p>
        <SearchResultsHeading key={query} query={query}>Encontramos una situación relacionada</SearchResultsHeading>
        <p>
          <Link href={view.eventHref}>{view.eventName}</Link>
        </p>
        <p className="confidence">
          Coincidencia: {Math.round(view.confidence * 100)}%
        </p>
      </div>
      <div className="procedure-results" aria-label="Trámites oficiales sugeridos">
        {view.procedures.length === 0 ? (
          <p className="empty-results">{EMPTY_PROCEDURES_COPY}</p>
        ) : (
          view.procedures.map((procedure) => (
            <ProcedureCard key={procedure.external_id} procedure={procedure} />
          ))
        )}
      </div>
    </section>
  );
}

function Disambiguation({
  query,
  view,
}: {
  query: string;
  view: Extract<SearchView, { kind: 'disambiguation' }>;
}) {
  return (
    <section
      className="search-results search-results--disambiguation"
      aria-labelledby="search-results-heading" role="region" aria-live="polite"
    >
      <p className="section-kicker">Resultados para “{query}”</p>
      <SearchResultsHeading key={query} query={query}>{view.heading}</SearchResultsHeading>
      <p className="result-description">
        Elegí la situación que mejor describa lo que necesitás resolver.
      </p>
      <ul className="option-list option-list--choices">
        {view.options.map((option) => (
          <li key={option.href}>
            <Link href={option.href}>{option.name}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Categories({
  query,
  view,
}: {
  query: string;
  view: Extract<SearchView, { kind: 'categories' }>;
}) {
  return (
    <section
      className="search-results search-results--categories"
      aria-labelledby="search-results-heading" role="region" aria-live="polite"
    >
      <p className="section-kicker">No encontramos una coincidencia directa para “{query}”</p>
      <SearchResultsHeading key={query} query={query}>Explorá por categoría</SearchResultsHeading>
      <p className="result-description">
        Elegí un tema para seguir descubriendo trámites oficiales.
      </p>
      <ul className="option-list option-list--categories">
        {view.categories.map((category) => (
          <li key={category.href}>
            <Link href={category.href}>{category.name}</Link>
          </li>
        ))}
      </ul>
      <p className="search-results__all">
        <Link className="text-link" href="/categories">
          Ver todos los temas
        </Link>
      </p>
    </section>
  );
}
