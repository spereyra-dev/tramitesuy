/**
 * One procedure card (task 14 GREEN, design §2.5, §5; spec "Procedure card
 * attribution display" ×3): name, required flag, cost_display VERBATIM —
 * the web never invents, estimates, defaults, or reformats a cost — and the
 * per-card attribution block.
 */
import type { ProcedureCard as ProcedureCardData } from '@/lib/api';
import { requiredFlagCopy } from '@/lib/display';

import { Attribution } from './Attribution';

export function ProcedureCard({ procedure }: { procedure: ProcedureCardData }) {
  const headingId = `procedure-${procedure.external_id}`;

  return (
    <article className="procedure-card" aria-labelledby={headingId}>
      <h3 id={headingId} className="procedure-name">{procedure.name}</h3>
      <dl className="procedure-details">
        <div>
          <dt>Carácter</dt>
          <dd className="procedure-required">{requiredFlagCopy(procedure.required)}</dd>
        </div>
        <div>
          <dt>Costo</dt>
          <dd className="procedure-cost">{procedure.cost_display}</dd>
        </div>
      </dl>
      {procedure.official_url !== null && (
        <p className="procedure-official-link">
          <a href={procedure.official_url}>Ir al trámite oficial</a>
        </p>
      )}
      <footer className="procedure-attribution">
        <Attribution source={procedure.source} />
      </footer>
    </article>
  );
}
