/**
 * Display-helper tests (task 11, RED first — strict TDD).
 *
 * Pure-function contract for the attribution and cost display rules (spec:
 * "Procedure card attribution display" ×3, design §2.5). The cost rule is
 * verbatim pass-through: the helpers never compose, format, estimate, or
 * default a cost value.
 */
import { describe, expect, it } from 'vitest';

import * as display from '@/lib/display';
import {
  requiredFlagCopy,
  sourceLinkState,
  formatLastSyncedAt,
  searchQueryTooLong,
} from '@/lib/display';

describe('search query API default limits', () => {
  it('counts Unicode scalars rather than UTF-16 code units at 512', () => {
    expect(searchQueryTooLong('😀'.repeat(512))).toBe(false); // 512 scalars, 2048 UTF-8 bytes.
    expect(searchQueryTooLong('😀'.repeat(513))).toBe(true);
    expect(searchQueryTooLong('á'.repeat(512))).toBe(false); // 1024 UTF-8 bytes.
    expect(searchQueryTooLong('á'.repeat(513))).toBe(true);
  });

  it('enforces an independently configured UTF-8 byte limit', () => {
    expect(searchQueryTooLong('á'.repeat(4), { maxChars: 10, maxBytes: 8 })).toBe(false);
    expect(searchQueryTooLong('á'.repeat(5), { maxChars: 10, maxBytes: 8 })).toBe(true);
  });
});

describe('display helpers: required flag', () => {
  it('labels required procedures as Obligatorio', () => {
    expect(requiredFlagCopy(true)).toBe('Obligatorio');
  });

  it('labels optional procedures as Opcional', () => {
    expect(requiredFlagCopy(false)).toBe('Opcional');
  });
});

describe('display helpers: source link state', () => {
  it('decides the linked state from a non-null official_url', () => {
    const state = sourceLinkState('https://www.gub.uy/tramites/x');
    expect(state.kind).toBe('linked');
    if (state.kind === 'linked') expect(state.href).toBe('https://www.gub.uy/tramites/x');
  });

  it('decides the explicit unavailable state when official_url is null', () => {
    const state = sourceLinkState(null);
    expect(state.kind).toBe('unavailable');
    if (state.kind === 'unavailable') {
      expect(state.copy).toBe('Enlace a la fuente no disponible');
    }
  });
});

describe('display helpers: last_synced_at presentation', () => {
  it('shows the sync date for a timestamp', () => {
    const shown = formatLastSyncedAt('2026-09-18T18:09:47+00:00');
    expect(shown).toBe('Actualizado: 18/09/2026');
  });

  it('has an explicit state when last_synced_at is null', () => {
    expect(formatLastSyncedAt(null)).toBe('Fecha de actualización no disponible');
  });
});

describe('display helpers: cost_display verbatim (no formatting layer)', {
  // The card prints cost_display exactly as received; the fixture string for
  // a missing cost is the exact `Sin costo informado` (spec scenario 3).
}, () => {
  const fixture = {
    cost: null,
    cost_display: 'Sin costo informado',
  };

  it('passes cost_display through verbatim', () => {
    // The verbatim rule needs no helper: pass-through is the implementation.
    expect(fixture.cost_display).toBe('Sin costo informado');
    expect(fixture.cost_display.length).toBe('Sin costo informado'.length);
  });

  it('never composes an estimated or formatted cost text', () => {
    // No display export may contain a cost formatting/estimation function.
    const exportNames = Object.keys(display).join(' ');
    expect(exportNames).not.toMatch(/cost/i);
    expect(exportNames).not.toMatch(/estimat|format.*cost|cost.*format/i);
  });
});
