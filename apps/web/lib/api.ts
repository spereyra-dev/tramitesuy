/**
 * Typed API client for the frozen /api/v1 surface.
 *
 * The types below mirror the shipped Rust DTOs exactly
 * (apps/api/src/dto.rs, apps/api/src/handlers/search.rs) — they are the
 * contract, not an approximation. Every request goes through `apiFetch`,
 * the single fetch wrapper: relative /api/v1/... paths (same-origin proxy
 * requirement), cache: 'no-store' AND revalidate: 0 (fresh-data
 * requirement, pinned by tests/freshness.test.ts).
 *
 * Exclusions (structural, per the web spec): there is deliberately NO
 * client function for /search/debug, /search/feedback, or
 * /procedures/{id} — those endpoints stay deployed and unused by the web.
 */

/** Mirrors dto.rs `SourceAttribution` (API-4). */
export interface SourceAttribution {
  official: boolean;
  name: string;
  official_url: string | null;
  last_synced_at: string | null;
  license: string;
}

/** Mirrors dto.rs `CostFields` (API-3). */
export interface CostFields {
  cost: string | null;
  cost_display: string;
}

/** Mirrors dto.rs `ProcedureCard` (API-6). */
export interface ProcedureCard {
  external_id: string;
  name: string;
  order: number;
  required: boolean;
  official_url: string | null;
  cost: string | null;
  cost_display: string;
  source: SourceAttribution;
}

/** Mirrors dto.rs `EventPage` (API-6). */
export interface EventPage {
  slug: string;
  name: string;
  description: string | null;
  category: string;
  procedures: ProcedureCard[];
}

/** Mirrors `open_payload` in handlers/search.rs (API-2). */
export interface SearchOpenResult {
  event: { slug: string; name: string };
  score: number;
  confidence: number;
  procedures: ProcedureCard[];
}

export interface SearchOption {
  slug: string;
  name: string;
  score: number;
  confidence: number;
}

export interface SearchCategory {
  slug: string;
  name: string;
}

interface SearchResponseBase {
  query: string;
  normalized_query: string;
  confidence: number;
}

/** The three-mode search payload, discriminated by `mode` (API-2). */
export type SearchResponse =
  | (SearchResponseBase & { mode: 'open'; results: SearchOpenResult[] })
  | (SearchResponseBase & { mode: 'disambiguation'; options: SearchOption[] })
  | (SearchResponseBase & { mode: 'categories'; categories: SearchCategory[] });

/** Mirrors dto.rs `CategoriesPage` (API-7). */
export interface CategoriesPage {
  categories: Array<{
    slug: string;
    name: string;
    order_index: number;
  }>;
}

/** Mirrors dto.rs `CategoryEventsPage` (API-7). */
export interface CategoryEventsPage {
  category: string;
  events: Array<{ slug: string; name: string }>;
}

/** Tagged API failures; pages translate `not-found` to Next's notFound(). */
export type ApiError =
  | { kind: 'not-found' }
  | { kind: 'bad-request'; message: string }
  | { kind: 'server'; message: string }
  | { kind: 'network'; message: string };

/** The upstream the dev proxy forwards /api/v1 requests to. */
export const API_BASE_URL_DEFAULT = 'http://localhost:8080';

/** RequestInit extended with Next's fetch `revalidate` option. */
export type FetchInit = RequestInit & { revalidate?: number };

/**
 * The one fetch wrapper every API request in the app uses. Always a
 * relative /api/v1/... path (never an API origin — the proxy requirement)
 * and always uncached (the freshness requirement).
 */
export async function apiFetch<T>(
  path: string,
): Promise<{ ok: true; data: T } | { ok: false; error: ApiError }> {
  if (!path.startsWith('/api/v1/') && !path.startsWith('/api/v1?')) {
    return Promise.resolve({
      ok: false,
      error: {
        kind: 'bad-request',
        message: `apiFetch only accepts relative /api/v1/... paths, got ${path}`,
      } satisfies ApiError,
    });
  }

  let response: Response;
  const init: FetchInit = {
    cache: 'no-store',
    revalidate: 0,
  };
  try {
    response = await fetch(path, init);
  } catch (error) {
    return {
      ok: false,
      error: {
        kind: 'network',
        message: error instanceof Error ? error.message : String(error),
      },
    };
  }

  if (response.status === 404) {
    return { ok: false, error: { kind: 'not-found' } };
  }
  if (response.status === 400) {
    return {
      ok: false,
      error: {
        kind: 'bad-request',
        message: `API rejected the request: ${response.status}`,
      },
    };
  }
  if (!response.ok) {
    return {
      ok: false,
      error: {
        kind: 'server',
        message: `API error: ${response.status}`,
      },
    };
  }

  return { ok: true, data: (await response.json()) as T };
}

/** GET /api/v1/search?q= (API-2). Empty queries are never submitted by the UI. */
export async function search(q: string): Promise<SearchResponse> {
  const result = await apiFetch<SearchResponse>(
    `/api/v1/search?q=${encodeURIComponent(q)}`,
  );
  if (!result.ok) {
    throw new Error(`search failed: ${result.error.kind}`);
  }
  return result.data;
}

/**
 * GET /api/v1/events/{slug} (API-6). Returns null on 404 so the caller can
 * call Next's notFound(); the slug passes through untouched.
 */
export async function getEvent(slug: string): Promise<EventPage | null> {
  const result = await apiFetch<EventPage>(`/api/v1/events/${slug}`);
  if (!result.ok) {
    if (result.error.kind === 'not-found') return null;
    throw new Error(`getEvent failed: ${result.error.kind}`);
  }
  return result.data;
}

/** GET /api/v1/categories (API-7). */
export async function getCategories(): Promise<CategoriesPage> {
  const result = await apiFetch<CategoriesPage>('/api/v1/categories');
  if (!result.ok) {
    throw new Error(`getCategories failed: ${result.error.kind}`);
  }
  return result.data;
}

/**
 * GET /api/v1/categories/{slug}/events (API-7). Returns null on 404 so the
 * caller can call notFound().
 */
export async function getCategoryEvents(
  slug: string,
): Promise<CategoryEventsPage | null> {
  const result = await apiFetch<CategoryEventsPage>(
    `/api/v1/categories/${slug}/events`,
  );
  if (!result.ok) {
    if (result.error.kind === 'not-found') return null;
    throw new Error(`getCategoryEvents failed: ${result.error.kind}`);
  }
  return result.data;
}
