'use client';

export default function ErrorPage({ reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return (
    <section className="status-page" role="alert">
      <h1>No pudimos cargar esta página</h1>
      <p>El servicio no está disponible por ahora. Podés intentar de nuevo.</p>
      <button className="retry-button" type="button" onClick={reset}>Reintentar</button>
    </section>
  );
}
