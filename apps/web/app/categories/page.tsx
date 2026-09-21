/**
 * Category list page (task 20 GREEN, spec "Route inventory" scenario 4,
 * design §1.1). A server component rendering the categories in
 * `order_index` ascending, each linking to its /categories/[slug] page.
 *
 * Structural navigation (T8): a fixed Spanish document title.
 */
import type { Metadata } from 'next';
import Link from 'next/link';

import { getCategories } from '@/lib/api';

export function generateMetadata(): Metadata {
  return { title: 'Categorías — TrámitesUY' };
}

// Request-time rendering everywhere (design §4): this page is API-backed and
// must never be prerendered or cached — every request fetches fresh.
export const dynamic = 'force-dynamic';

export default async function CategoriesPage() {
  const page = await getCategories();

  // The list renders in API order_index, never raw array position.
  const categories = [...page.categories].sort(
    (a, b) => a.order_index - b.order_index,
  );

  return (
    <section className="discovery-page" aria-labelledby="categories-heading">
      <header className="discovery-page__header">
        <p className="section-kicker">Explorá por tema</p>
        <h1 id="categories-heading">Categorías</h1>
        <p>Explorá los trámites según el tema que necesitás resolver.</p>
      </header>
      <ul className="option-list option-list--categories">
        {categories.map((category) => (
          <li key={category.slug}>
            <Link href={`/categories/${category.slug}`}>{category.name}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
