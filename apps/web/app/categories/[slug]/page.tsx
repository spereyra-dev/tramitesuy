/**
 * Category event list page (task 20 GREEN, spec "Route inventory"
 * scenarios 3–4). A server component: the [slug] param (a Spanish hyphen
 * slug) passes through to getCategoryEvents untouched (proposal P1/TX-4),
 * a null page (API 404) renders the shared not-found state via
 * notFound(), and every event links to its /events/[slug] page — giving
 * the categories search mode a real destination.
 *
 * Structural navigation (T7/T8): a bottom back link to /categories after
 * the event list (the pinned top link stays untouched) and a per-page
 * document title. The human category name is resolved through
 * getCategories() with a slug fallback — metadata never throws.
 */
import type { Metadata } from 'next';
import Link from 'next/link';
import { notFound } from 'next/navigation';

import { getCategories, getCategoryEvents } from '@/lib/api';

/**
 * Resolve the human category name for the heading/title; getCategories()
 * throws on API failure, so a failure degrades to the raw slug.
 */
async function resolveCategoryName(slug: string): Promise<string> {
  try {
    const { categories } = await getCategories();
    return (
      categories.find((category) => category.slug === slug)?.name ?? slug
    );
  } catch {
    return slug;
  }
}

export async function generateMetadata({
  params,
}: {
  params: Promise<{ slug: string }>;
}): Promise<Metadata> {
  const { slug } = await params;
  const name = await resolveCategoryName(slug);
  return { title: `${name} — TrámitesUY` };
}

export default async function CategoryEventsPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const page = await getCategoryEvents(slug);
  if (page === null) notFound();

  const categoryName = await resolveCategoryName(page.category);

  return (
    <section className="discovery-page" aria-labelledby="category-events-heading">
      <header className="discovery-page__header">
        <p className="section-kicker">Categoría</p>
        <h1 id="category-events-heading">Situaciones: {categoryName}</h1>
        <p>Elegí una situación para ver los trámites oficiales relacionados.</p>
        <Link className="text-link" href="/categories">
          Volver a categorías
        </Link>
      </header>
      <ul className="option-list option-list--events">
        {page.events.map((event) => (
          <li key={event.slug}>
            <Link href={`/events/${event.slug}`}>{event.name}</Link>
          </li>
        ))}
      </ul>
      <p className="back-link">
        <Link href="/categories">← Volver a categorías</Link>
      </p>
    </section>
  );
}
