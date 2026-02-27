#!/usr/bin/env node
/**
 * Lightweight docs linter to prevent placeholder text from shipping.
 *
 * Usage:
 *   node docs/scripts/lint-docs.mjs
 */

import { readdir, readFile, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const docsDir = path.join(repoRoot, 'docs', 'docs');

const PATTERNS = [
  /Populate this page/i,
  /\bWhat to document next\b/i,
  /\bWhat to document\b/i,
  /\bTBD\b/i,
  /\bWIP\b/i,
];

async function* walk(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      yield* walk(p);
    } else if (e.isFile()) {
      yield p;
    }
  }
}

async function main() {
  const bad = [];
  for await (const file of walk(docsDir)) {
    if (!file.endsWith('.md') && !file.endsWith('.mdx')) continue;
    const s = await stat(file);
    if (s.size === 0) continue;
    const content = await readFile(file, 'utf8');
    for (const pattern of PATTERNS) {
      const m = content.match(pattern);
      if (m) {
        bad.push({ file, pattern: String(pattern) });
        break;
      }
    }
  }

  if (bad.length) {
    console.error('[lint-docs] Placeholder content detected:');
    for (const b of bad) {
      console.error(`- ${path.relative(repoRoot, b.file)} matched ${b.pattern}`);
    }
    process.exit(1);
  }
}

main().catch((err) => {
  console.error('[lint-docs:error]', err?.stack || err?.message || String(err));
  process.exit(1);
});

