import type { Metadata } from 'next';
import Link from 'next/link';
import './globals.css';

export const metadata: Metadata = {
  title: 'TrámitesUY',
  description:
    'Descubrí qué trámites oficiales aplican a tu situación, con resultados audibles y atribuidos al Catálogo de trámites y servicios del Estado — AGESIC.',
};

/**
 * Root layout (design §1.1): Spanish document language, one global
 * stylesheet (P3 minimal plain CSS), no framework.
 */
export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="es">
      <body>
        <a className="skip-link" href="#main-content">
          Saltar al contenido principal
        </a>
        <header className="site-header">
          <div className="site-header__content">
            <Link href="/" className="site-title" aria-label="TrámitesUY, inicio">
              TrámitesUY
            </Link>
            <p className="site-tagline">
              Trámites del Estado uruguayo, explicados por situación de vida.
            </p>
            <nav className="site-nav" aria-label="Navegación principal">
              <Link href="/categories">Explorar categorías</Link>
            </nav>
          </div>
        </header>
        <main id="main-content" className="site-main" tabIndex={-1}>
          {children}
        </main>
        <footer className="site-footer">
          <div className="site-footer__content">
            <p>
              Fuente oficial:{' '}
              <a href="https://catalogodatos.gub.uy/dataset/agesic-guia-de-tramites">
                Catálogo de trámites y servicios del Estado — AGESIC
              </a>{' '}
              · licencia odc-uy
            </p>
          </div>
        </footer>
      </body>
    </html>
  );
}
