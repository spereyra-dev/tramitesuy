/**
 * Home shell (PR 1, W1 working state): static foundation so
 * `next build` succeeds. The search box and the three-mode rendering
 * land in PR 2 (slice W2); this page must not fetch anything yet.
 */
export default function HomePage() {
  return (
    <div>
      <h1>¿Qué trámite necesitás hacer?</h1>
      <p>
        Contanos tu situación (por ejemplo: “compré un auto usado”) y te
        mostramos los trámites oficiales que aplican.
      </p>
      <p>La búsqueda estará disponible en la próxima entrega.</p>
    </div>
  );
}
