/**
 * The per-card attribution block (task 14 GREEN, design §5; API-4; spec
 * "Procedure card attribution display" ×3). Per-card, never demoted to a
 * page-level footer: official marker, source name, the official_url link or
 * the explicit unavailable state, and last_synced_at.
 */
import type { SourceAttribution } from '@/lib/api';
import {
  formatLastSyncedAt,
  sourceLinkState,
} from '@/lib/display';

export function Attribution({ source }: { source: SourceAttribution }) {
  const link = sourceLinkState(source.official_url);
  return (
    <p className="attribution">
      {source.official ? 'Fuente oficial' : 'Fuente'}: {source.name} ·{' '}
      {link.kind === 'linked' ? (
        <a href={link.href}>Ver fuente oficial</a>
      ) : (
        link.copy
      )}{' '}
      · {formatLastSyncedAt(source.last_synced_at)}
    </p>
  );
}
