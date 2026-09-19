/**
 * The render-decision logic for the three search modes (task 10 GREEN,
 * spec "Three-mode search rendering" ×3, design §2.1).
 *
 * `searchView` narrows the `SearchResponse` union by its `mode` discriminant
 * and returns the view model the home page renders inline on `/?q=`. The
 * union makes reading `options` on an open response (or `results` on a
 * disambiguation one) a compile error, so a page cannot cross the arms.
 */

import type { ProcedureCard, SearchResponse } from './api';

/** Cards render in API `order`, never raw array position. */
function ordered<T extends { order: number }>(cards: T[]): T[] {
  return [...cards].sort((a, b) => a.order - b.order);
}

export type SearchView =
  | {
      kind: 'open';
      eventName: string;
      eventHref: string;
      confidence: number;
      procedures: ProcedureCard[];
    }
  | {
      kind: 'disambiguation';
      heading: string;
      options: Array<{ name: string; href: string }>;
    }
  | {
      kind: 'categories';
      categories: Array<{ name: string; href: string }>;
    };

export const DISAMBIGUATION_HEADING = '¿Te referías a...?';

export function searchView(response: SearchResponse): SearchView {
  switch (response.mode) {
    case 'open': {
      const result = response.results[0];
      return {
        kind: 'open',
        eventName: result.event.name,
        eventHref: `/events/${result.event.slug}`,
        confidence: result.confidence,
      // Full cards, not projections: every rendered card must carry its
      // per-card attribution block (spec "Procedure card attribution").
      procedures: ordered(result.procedures),
    };
    }
    case 'disambiguation':
      return {
        kind: 'disambiguation',
        heading: DISAMBIGUATION_HEADING,
        options: response.options.map((option) => ({
          name: option.name,
          href: `/events/${option.slug}`,
        })),
      };
    case 'categories':
      return {
        kind: 'categories',
        categories: response.categories.map((category) => ({
          name: category.name,
          href: `/categories/${category.slug}`,
        })),
      };
  }
}
