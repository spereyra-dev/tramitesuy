/**
 * Category event list page (task 20 GREEN, spec "Route inventory"
 * scenarios 3–4). A server component: the [slug] param (a Spanish hyphen
 * slug) passes through to getCategoryEvents untouched (proposal P1/TX-4),
 * a null page (API 404) renders the shared not-found state via
 * notFound(), and every event links to its /events/[slug] page — giving
 * the categories search mode a real destination.
 */
import Link from 'next/link';
import { notFound } from 'next/navigation';

import { getCategoryEvents } from '@/lib/api';

export default async function CategoryEventsPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const page = await getCategoryEvents(slug);
  if (page === null) notFound();

  return (
    <section>
      <h1>Eventos: {page.category}</h1>
      <ul className="option-list">
        {page.events.map((event) => (
          <li key={event.slug}>
            <Link href={`/events/${event.slug}`}>{event.name}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
