/**
 * Event page (task 18 GREEN, design §1.1/§2.2; spec "Event page and
 * empty-procedures state" ×2, "Route inventory" scenario 3).
 *
 * A server component: the [slug] param (a Spanish hyphen slug) passes
 * through to getEvent untouched (no transliteration — proposal P1/TX-4),
 * a null event (API 404) renders the shared not-found state via
 * notFound(), and the procedures render as ordered cards reusing the
 * ProcedureCard/Attribution components from PR 2 (per-card attribution,
 * verbatim cost_display). An empty procedures list renders the pinned
 * empty-state copy instead of an empty list.
 *
 * Structural navigation (T7/T8): a top breadcrumb (Inicio › human
 * category name) before the h1, a bottom back link to the category, and
 * a per-page document title via generateMetadata. The category name is
 * resolved through getCategories() and any resolution failure falls
 * back to neutral copy — the page must never break because of it.
 */
import type { Metadata } from 'next';
import Link from 'next/link';
import { notFound } from 'next/navigation';

import { ProcedureCard } from '@/components/ProcedureCard';
import { getCategories, getEvent } from '@/lib/api';
import { EMPTY_PROCEDURES_COPY } from '@/lib/display';

/** Neutral copy used when the human category name cannot be resolved. */
const CATEGORY_FALLBACK_COPY = 'la categoría';

/**
 * Resolve the human category name for a category slug. getCategories()
 * throws on API failure, so this is wrapped: a failure degrades to null
 * and callers fall back to neutral copy — never a broken page.
 */
async function resolveCategoryName(
  slug: string,
): Promise<{ name: string } | null> {
  try {
    const { categories } = await getCategories();
    const match = categories.find((category) => category.slug === slug);
    return match ? { name: match.name } : null;
  } catch {
    return null;
  }
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ slug: string }>;
}): Promise<Metadata> {
  const { slug } = await params;
  let title = `${slug} — TrámitesUY`;
  try {
    const event = await getEvent(slug);
    if (event !== null) title = `${event.name} — TrámitesUY`;
  } catch {
    // Metadata must never throw the page down; keep the slug fallback.
  }
  return { title };
}

export default async function EventPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const event = await getEvent(slug);
  if (event === null) notFound();

  const resolvedCategory = await resolveCategoryName(event.category);
  const categoryName = resolvedCategory?.name ?? CATEGORY_FALLBACK_COPY;

  // Cards render in API `order`, never raw array position.
  const procedures = [...event.procedures].sort((a, b) => a.order - b.order);

  return (
    <article className="event-page">
      <nav className="breadcrumb" aria-label="Ruta de navegación">
        <Link href="/">Inicio</Link>
        <span className="breadcrumb__sep" aria-hidden="true">
          ›
        </span>
        <Link href={`/categories/${event.category}`}>{categoryName}</Link>
      </nav>
      <header className="event-page__header">
        <p className="section-kicker">Situación de vida</p>
        <h1>{event.name}</h1>
        {event.description !== null && <p className="event-page__intro">{event.description}</p>}
      </header>
      <section className="event-procedures" aria-labelledby="event-procedures-heading">
        <h2 id="event-procedures-heading">Trámites oficiales relacionados</h2>
        {procedures.length === 0 ? (
          <p className="empty-results">{EMPTY_PROCEDURES_COPY}</p>
        ) : (
          procedures.map((procedure) => (
            <ProcedureCard key={procedure.external_id} procedure={procedure} />
          ))
        )}
      </section>
      <p className="back-link">
        <Link href={`/categories/${event.category}`}>← Volver a {categoryName}</Link>
      </p>
    </article>
  );
}
