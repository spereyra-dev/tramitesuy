import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { createElement } from 'react';

import { SearchForm } from '@/components/SearchForm';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
}));

describe('search form', () => {
  it('provides a visible Spanish label, instructional help, and a submit button', () => {
    const html = renderToStaticMarkup(createElement(SearchForm));

    expect(html).toContain('<label');
    expect(html).toContain('Describí tu situación');
    expect(html).toContain('id="search-guidance"');
    expect(html).toContain('aria-describedby="search-guidance"');
    expect(html).toContain('required=""');
    expect(html).toContain('type="submit"');
    expect(html).toContain('Buscar trámites');
  });
});
