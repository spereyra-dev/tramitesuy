import type { NextConfig } from 'next';

/**
 * Same-origin proxy (web spec requirement "Same-origin proxy fetch").
 *
 * Every browser request to /api/v1/... on this origin is proxied to
 * API_BASE_URL (dev default http://localhost:8080; compose sets
 * http://api:8080). The browser never talks to the API origin cross-origin
 * and no CORS dependency exists anywhere. No .env file is committed; the
 * default lives here so the dev story is zero-config.
 */
const apiBaseUrl = process.env.API_BASE_URL ?? 'http://localhost:8080';

const nextConfig: NextConfig = {
  async rewrites() {
    return [
      {
        source: '/api/v1/:path*',
        destination: `${apiBaseUrl}/api/v1/:path*`,
      },
    ];
  },
};

export default nextConfig;
