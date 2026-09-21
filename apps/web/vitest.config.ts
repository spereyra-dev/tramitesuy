import path from 'node:path';

import { defineConfig } from 'vitest/config';

export default defineConfig({
  // .tsx components compile with the automatic JSX runtime so test files can
  // render server components without importing React manually (no testing
  // library is added; react-dom/server does the rendering).
  esbuild: {
    jsx: 'automatic',
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, '.'),
      // next/font/google self-hosts fonts at Next build time and is not
      // runnable under vitest's node environment; tests use this stub so the
      // layout's font-variable classes stay assertable without the toolchain.
      'next/font/google': path.resolve(__dirname, 'tests/mocks/next-font-google.ts'),
    },
  },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
  },
});
