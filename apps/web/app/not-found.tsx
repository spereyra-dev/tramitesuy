import Link from 'next/link';

/**
 * Shared not-found state (web spec "Route inventory": unknown event and
 * category slugs render 404; design §1.1: one app-level not-found shared
 * by both dynamic segments).
 */
export default function NotFound() {
  return (
    <section className="status-page" aria-labelledby="not-found-heading">
      <p className="section-kicker">Error 404</p>
      <h1 id="not-found-heading">Página no encontrada</h1>
      <p role="status">El contenido que buscás no existe o cambió de dirección.</p>
      <Link className="text-link" href="/">
        Volver al inicio
      </Link>
    </section>
  );
}
