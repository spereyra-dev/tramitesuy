import { createElement } from 'react';
import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';

import { ProcedureCard } from '@/components/ProcedureCard';
import type { ProcedureCard as ProcedureCardData } from '@/lib/api';

import eventPage from './fixtures/event-page.json';

const procedure = eventPage.procedures[0] as ProcedureCardData;

describe('procedure card', () => {
  it('groups the literal official data in a labelled card with a descriptive action', () => {
    const html = renderToStaticMarkup(createElement(ProcedureCard, { procedure }));

    expect(html).toContain('<article class="procedure-card" aria-labelledby="procedure-4551">');
    expect(html).toContain('<h3 id="procedure-4551" class="procedure-name">');
    expect(html).toContain('<dl class="procedure-details">');
    expect(html).toContain('<dt>Carácter</dt>');
    expect(html).toContain('<dt>Costo</dt>');
    expect(html).toContain('<dd class="procedure-cost">Sin costo informado</dd>');
    expect(html).toContain('Ir al trámite oficial');
    expect(html).toContain('href="https://www.gub.uy/tramites/solicitud-empadronamientos"');
  });

  it('retains the per-card official source and sync date in its footer', () => {
    const html = renderToStaticMarkup(createElement(ProcedureCard, { procedure }));

    expect(html).toContain('<footer class="procedure-attribution">');
    expect(html).toContain('Fuente oficial');
    expect(html).toContain('Catálogo de trámites y servicios del Estado — AGESIC');
    expect(html).toContain('Actualizado:');
  });
});
