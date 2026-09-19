/**
 * No-API-origin source scan (task 22 — spec "Same-origin proxy fetch"
 * scenario "Direct API-origin fetch is absent", proposal R2).
 *
 * Statically scans every source file under app/, components/ and lib/ and
 * fails if any fetch call site targets an absolute API origin, or if any
 * scanned file carries a concrete API-origin literal
 * (http://localhost:8080, http://api:8080, or any http(s):// host). The
 * only origin a fetch may ever resolve to is the web app's own — and that
 * resolution is pinned at runtime by freshness.test.ts.
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

const WEB_ROOT = path.resolve(__dirname, '..');
const SCAN_DIRS = ['app', 'components', 'lib'];

/** Concrete API origins that must never appear in scanned source. */
const API_ORIGIN_PATTERN =
  /https?:\/\/(localhost:8080|api:8080|127\.0\.0\.1:8080|\[[^\]]*\]:8080)/;

function collectFiles(dir: string): string[] {
  const entries = readdirSync(dir);
  const files: string[] = [];
  for (const entry of entries) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      files.push(...collectFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry)) {
      files.push(full);
    }
  }
  return files;
}

const sources = SCAN_DIRS.flatMap((dir) =>
  collectFiles(path.join(WEB_ROOT, dir)),
).map((file) => ({
  file: path.relative(WEB_ROOT, file),
  text: readFileSync(file, 'utf8'),
}));

/**
 * Extracts every `fetch(` call site's URL argument expression (the text
 * between the opening paren and the first comma) so each can be checked
 * for absolute origins.
 */
function fetchUrlArguments(text: string): string[] {
  const args: string[] = [];
  const callPattern = /\bfetch\s*\(/g;
  let match: RegExpExecArray | null;
  while ((match = callPattern.exec(text)) !== null) {
    const from = match.index + match[0].length;
    const window = text.slice(from, from + 200);
    const arg = window.slice(0, Math.min(...[window.indexOf(','), window.indexOf(')')].filter((i) => i >= 0)));
    args.push(arg);
  }
  return args;
}

describe('no API-origin fetch anywhere (source scan)', () => {
  it('no fetch call site targets an absolute origin', () => {
    for (const { file, text } of sources) {
      for (const arg of fetchUrlArguments(text)) {
        expect(arg, `${file}: fetch call site uses an absolute origin`).not.toContain(
          'http',
        );
      }
    }
  });

  it('no scanned source carries a concrete API-origin literal', () => {
    for (const { file, text } of sources) {
      expect(text, `${file}: contains an API-origin literal`).not.toMatch(
        API_ORIGIN_PATTERN,
      );
    }
  });

  it('every request path stays on the relative same-origin /api/v1/... proxy', () => {
    // The single wrapper is the only place that fetches, and it refuses any
    // path that is not a relative /api/v1/... (runtime pinned by
    // freshness.test.ts; here the guard itself is pinned statically).
    const wrapper = sources.find((s) => s.file.includes('lib') && s.file.endsWith('api.ts'));
    expect(wrapper).toBeDefined();
    expect(wrapper!.text).toContain("path.startsWith('/api/v1/')");
  });
});
