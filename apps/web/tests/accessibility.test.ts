import { readFileSync } from 'node:fs';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';

import RootLayout from '@/app/layout';
import { SearchForm } from '@/components/SearchForm';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
}));

const styles = readFileSync(new URL('../app/globals.css', import.meta.url), 'utf8');

describe('semantic accessibility regression coverage', () => {
  it('keeps the application landmarks ordered around the main content and names navigation links', () => {
    const html = renderToStaticMarkup(
      createElement(RootLayout, null, createElement('p', null, 'Contenido de prueba')),
    );

    expect(html).toMatch(/<html lang="es">/);
    expect(html).toMatch(/href="#main-content">Saltar al contenido principal/);
    expect(html).toMatch(/<nav[^>]*aria-label="Navegación principal"/);
    expect(html).toMatch(/href="\/categories">Explorar categorías/);
    expect(html).toMatch(/href="https:\/\/catalogodatos\.gub\.uy\/dataset\/agesic-guia-de-tramites"[^>]*>Catálogo de trámites y servicios del Estado — AGESIC/);
    expect(html.indexOf('<header')).toBeLessThan(html.indexOf('<main'));
    expect(html.indexOf('<main')).toBeLessThan(html.indexOf('<footer'));
  });

  it('associates the search input with its visible label and guidance', () => {
    const html = renderToStaticMarkup(createElement(SearchForm));

    expect(html).toMatch(/<label[^>]*for="situacion"[^>]*>Describí tu situación<\/label>/);
    expect(html).toMatch(/<input[^>]*id="situacion"[^>]*name="q"/);
    expect(html).toMatch(/<input[^>]*aria-describedby="search-guidance search-limit"/);
    expect(html).toMatch(/<p[^>]*id="search-guidance"/);
  });

  it('keeps visible keyboard focus for interactive elements and the skip link', () => {
    expect(styles).toMatch(/:focus-visible\s*\{[^}]*outline:\s*3px solid var\(--focus\)/s);
    expect(styles).toMatch(/\.skip-link:focus-visible\s*\{[^}]*transform:\s*translateY\(0\)/s);
  });
});
