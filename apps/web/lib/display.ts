/**
 * Pure display helpers (task 12, GREEN — design §2.5).
 *
 * English code, Spanish domain copy. These helpers never touch the network
 * and never compose, format, estimate, or default a cost value: the card
 * prints `cost_display` verbatim, which needs no helper at all (spec:
 * "Procedure card attribution display").
 */

export const SOURCE_LINK_UNAVAILABLE_COPY = 'Enlace a la fuente no disponible';
export const LAST_SYNCED_UNAVAILABLE_COPY = 'Fecha de actualización no disponible';
export const EMPTY_PROCEDURES_COPY = 'Aún no hay trámites vinculados a este evento';

/** The `required`-flag copy shown on every procedure card. */
export function requiredFlagCopy(required: boolean): string {
  return required ? 'Obligatorio' : 'Opcional';
}

/**
 * The source-link decision: render the official_url as a link, or the
 * explicit unavailable state — never a broken or fabricated link (API-4,
 * spec scenario 2).
 */
export type SourceLinkState =
  | { kind: 'linked'; href: string }
  | { kind: 'unavailable'; copy: string };

export function sourceLinkState(officialUrl: string | null): SourceLinkState {
  if (officialUrl === null) {
    return { kind: 'unavailable', copy: SOURCE_LINK_UNAVAILABLE_COPY };
  }
  return { kind: 'linked', href: officialUrl };
}

/** The `last_synced_at` presentation (API-4 freshness contract). */
export function formatLastSyncedAt(lastSyncedAt: string | null): string {
  if (lastSyncedAt === null) return LAST_SYNCED_UNAVAILABLE_COPY;
  const date = new Date(lastSyncedAt);
  if (Number.isNaN(date.getTime())) return LAST_SYNCED_UNAVAILABLE_COPY;
  const day = String(date.getUTCDate()).padStart(2, '0');
  const month = String(date.getUTCMonth() + 1).padStart(2, '0');
  return `Actualizado: ${day}/${month}/${date.getUTCFullYear()}`;
}
