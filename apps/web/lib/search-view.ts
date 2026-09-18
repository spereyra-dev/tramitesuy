/**
 * The render-decision logic for the three search modes (task 10 GREEN,
 * spec "Three-mode search rendering" ×3, design §2.1).
 *
 * `searchView` narrows the `SearchResponse` union by its `mode` discriminant
 * and returns the view model the home page renders inline on `/?q=`. The
 * union makes reading `options` on an open response (or `results` on a
 * disambiguation one) a compile error, so a page cannot cross the arms.
 */

import type { SearchResponse } from './api';

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
      procedures: Array<{ order: number; name: string; required: boolean; costDisplay: string }>;
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
        procedures: ordered(result.procedures).map((card) => ({
          order: card.order,
          name: card.name,
          required: card.required,
          costDisplay: card.cost_display,
        })),
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
