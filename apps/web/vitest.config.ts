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
    },
  },
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts'],
  },
});
