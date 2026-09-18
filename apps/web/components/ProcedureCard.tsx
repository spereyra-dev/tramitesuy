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
  return (
    <article className="procedure-card">
      <h3 className="procedure-name">{procedure.name}</h3>
      <p className="procedure-required">{requiredFlagCopy(procedure.required)}</p>
      <p className="procedure-cost">{procedure.cost_display}</p>
      {procedure.official_url !== null && (
        <p className="procedure-official-link">
          <a href={procedure.official_url}>Ver trámite oficial</a>
        </p>
      )}
      <Attribution source={procedure.source} />
    </article>
  );
}
