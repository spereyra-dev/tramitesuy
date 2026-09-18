import type { Metadata } from 'next';
import Link from 'next/link';
import './globals.css';

export const metadata = {
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
        <header className="site-header">
          <Link href="/" className="site-title">
            TrámitesUY
          </Link>
          <p className="site-tagline">
            Trámites del Estado uruguayo, explicados por situación de vida.
          </p>
        </header>
        <main className="site-main">{children}</main>
        <footer className="site-footer">
          <p>
            Fuente oficial:{' '}
            <a href="https://catalogodatos.gub.uy/dataset/agesic-guia-de-tramites">
              Catálogo de trámites y servicios del Estado — AGESIC
            </a>{' '}
            · licencia odc-uy
          </p>
        </footer>
      </body>
    </html>
  );
}
