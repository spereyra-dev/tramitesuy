import Link from 'next/link';

/**
 * Shared not-found state (web spec "Route inventory": unknown event and
 * category slugs render 404; design §1.1: one app-level not-found shared
 * by both dynamic segments).
 */
export default function NotFound() {
  return (
    <div>
      <h1>Página no encontrada</h1>
      <p>El contenido que buscás no existe o cambió de dirección.</p>
      <p>
        <Link href="/">Volver al inicio</Link>
      </p>
    </div>
  );
}
