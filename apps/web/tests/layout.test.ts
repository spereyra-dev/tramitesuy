import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import RootLayout, { metadata } from '@/app/layout';

describe('root layout', () => {
  it('provides an accessible application shell with discovery navigation', () => {
    const html = renderToStaticMarkup(
      createElement(RootLayout, null, createElement('p', null, 'Contenido de prueba')),
    );

    expect(html).toContain('href="#main-content"');
    expect(html).toContain('<header');
    expect(html).toContain('<nav');
    expect(html).toContain('Explorar categorías');
    expect(html).toContain('<main');
    expect(html).toContain('id="main-content"');
    expect(html).toContain('<footer');
    expect(html).toContain('TrámitesUY');
  });

  it('identifies the independent publisher and the official origin of procedure data', () => {
    const html = renderToStaticMarkup(
      createElement(RootLayout, null, createElement('p', null, 'Contenido de prueba')),
    );
    expect(html).toContain('TrámitesUY es un proyecto ciudadano independiente y no oficial; los datos de los trámites provienen del catálogo oficial de AGESIC.');
    expect(html).toContain('Catálogo de trámites y servicios del Estado — AGESIC');
  });

  it('describes auditable results and uses situación rather than eventos', () => {
    expect(metadata.description).toContain('resultados auditables');
    expect(metadata.description).not.toContain('audibles');
    expect(metadata.description).toContain('situación');
    expect(metadata.description).not.toMatch(/eventos/i);
  });
});
