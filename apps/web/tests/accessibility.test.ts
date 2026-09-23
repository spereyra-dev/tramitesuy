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

function declaredColor(selector: string, property: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const rule = styles.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`));
  const declaration = rule?.[1].match(new RegExp(`(?:^|[;\\n])\\s*${property}:\\s*([^;]+)`));
  if (!declaration) throw new Error(`Missing ${property} for ${selector}`);
  return declaration[1].trim();
}

function luminance(hex: string): number {
  const channels = hex.match(/^#([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i);
  if (!channels) throw new Error(`Expected six-digit hex colour: ${hex}`);
  const [r, g, b] = channels.slice(1).map((part) => {
    const value = parseInt(part, 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(foreground: string, background: string): number {
  const light = Math.max(luminance(foreground), luminance(background));
  const dark = Math.min(luminance(foreground), luminance(background));
  return (light + 0.05) / (dark + 0.05);
}

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

  it('keeps the header title and navigation text AA-contrast on hover and keyboard focus', () => {
    const backgroundToken = declaredColor('.site-header', 'background');
    const background = declaredColor(':root', backgroundToken.slice(4, -1));
    // Header selectors must outrank the later generic a:hover (0,1,1).
    expect(styles).toMatch(/\.site-header a:hover\s*,\s*\.site-header a:focus-visible\s*\{/);
    const foreground = declaredColor('.site-header a:focus-visible', 'color');
    expect(foreground).toBe('#ffffff');
    expect(contrast(foreground, background)).toBeCloseTo(9.44, 2);
    expect(contrast(foreground, background)).toBeGreaterThanOrEqual(4.5);
    expect(styles).toMatch(/:focus-visible\s*\{[^}]*outline:\s*3px solid var\(--focus\)/s);
  });
});
