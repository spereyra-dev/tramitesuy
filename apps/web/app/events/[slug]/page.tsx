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
 */
import { notFound } from 'next/navigation';

import { ProcedureCard } from '@/components/ProcedureCard';
import { getEvent } from '@/lib/api';
import { EMPTY_PROCEDURES_COPY } from '@/lib/display';

export default async function EventPage({
  params,
}: {
  params: Promise<{ slug: string }>;
}) {
  const { slug } = await params;
  const event = await getEvent(slug);
  if (event === null) notFound();

  // Cards render in API `order`, never raw array position.
  const procedures = [...event.procedures].sort((a, b) => a.order - b.order);

  return (
    <article>
      <h1>{event.name}</h1>
      {event.description !== null && <p>{event.description}</p>}
      <p className="event-category">Categoría: {event.category}</p>
      {procedures.length === 0 ? (
        <p>{EMPTY_PROCEDURES_COPY}</p>
      ) : (
        procedures.map((procedure) => (
          <ProcedureCard key={procedure.external_id} procedure={procedure} />
        ))
      )}
    </article>
  );
}
