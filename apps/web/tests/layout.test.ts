import { describe, expect, it } from 'vitest';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import RootLayout from '@/app/layout';

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
});
