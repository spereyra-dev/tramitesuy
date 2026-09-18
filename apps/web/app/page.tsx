/**
 * Home (task 15 GREEN): the search box plus the three-mode inline rendering
 * on /?q= (spec "Three-mode search rendering" ×3, proposal R5). A server
 * component: it reads searchParams.q, calls search() through the same-origin
 * proxy, and renders the matching arm of the response here — no redirect to
 * the event page, no client-side data fetching.
 */
import Link from 'next/link';

import { ProcedureCard } from '@/components/ProcedureCard';
import { SearchForm } from '@/components/SearchForm';
import { search } from '@/lib/api';
import {
  EMPTY_PROCEDURES_COPY,
} from '@/lib/display';
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
    <div>
      <h1>¿Qué trámite necesitás hacer?</h1>
      <p>
        Contanos tu situación (por ejemplo: “compré un auto usado”) y te
        mostramos los trámites oficiales que aplican.
      </p>
      <SearchForm initialQuery={query ?? ''} />
      {query ? <SearchResults query={query} /> : null}
    </div>
  );
}

async function SearchResults({ query }: { query: string }) {
  const view = searchView(await search(query));

  if (view.kind === 'open') return <OpenResult view={view} />;
  if (view.kind === 'disambiguation') return <Disambiguation view={view} />;
  return <Categories view={view} />;
}

function OpenResult({ view }: { view: Extract<SearchView, { kind: 'open' }> }) {
  return (
    <section aria-label="Resultado">
      <h2>
        <Link href={view.eventHref}>{view.eventName}</Link>
      </h2>
      <p className="confidence">
        Coincidencia: {Math.round(view.confidence * 100)}%
      </p>
      {view.procedures.length === 0 ? (
        <p>{EMPTY_PROCEDURES_COPY}</p>
      ) : (
        view.procedures.map((procedure) => (
          <ProcedureCard key={procedure.external_id} procedure={procedure} />
        ))
      )}
    </section>
  );
}

function Disambiguation({
  view,
}: {
  view: Extract<SearchView, { kind: 'disambiguation' }>;
}) {
  return (
    <section aria-label="Opciones">
      <h2>{view.heading}</h2>
      <ul className="option-list">
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
  view,
}: {
  view: Extract<SearchView, { kind: 'categories' }>;
}) {
  return (
    <section aria-label="Categorías">
      <h2>Explorá por categoría</h2>
      <ul className="option-list">
        {view.categories.map((category) => (
          <li key={category.href}>
            <Link href={category.href}>{category.name}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
