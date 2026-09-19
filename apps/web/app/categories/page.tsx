/**
 * Category list page (task 20 GREEN, spec "Route inventory" scenario 4,
 * design §1.1). A server component rendering the categories in
 * `order_index` ascending, each linking to its /categories/[slug] page.
 */
import Link from 'next/link';

import { getCategories } from '@/lib/api';

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
    <section>
      <h1>Categorías</h1>
      <ul className="option-list">
        {categories.map((category) => (
          <li key={category.slug}>
            <Link href={`/categories/${category.slug}`}>{category.name}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
