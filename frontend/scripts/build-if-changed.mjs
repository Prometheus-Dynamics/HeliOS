#!/usr/bin/env bun
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, readlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const frontendDir = resolve(__dirname, '..');
const repoRoot = resolve(frontendDir, '..');

const outputDir = join(frontendDir, 'build');
const stampPath = join(repoRoot, 'gaia', 'build', 'artifacts', 'helios-frontend.inputs.json');

const checkOnly = process.argv.includes('--check');
const forceRebuild = process.env.FORCE_FRONTEND_BUILD === '1';

const ignoredNames = new Set([
  '.git',
  '.cache',
  '.svelte-kit',
  'build',
  'dist',
  'node_modules',
  'target',
  '.DS_Store'
]);

const inputRoots = [
  { abs: frontendDir, label: 'frontend' },
  { abs: join(repoRoot, 'docs'), label: 'docs' },
  { abs: join(repoRoot, 'tools', 'api-codegen'), label: 'tools/api-codegen' },
  { abs: join(repoRoot, 'backend', 'src', 'helios-api'), label: 'backend/src/helios-api' },
  { abs: join(repoRoot, 'backend', 'Cargo.toml'), label: 'backend/Cargo.toml' },
  { abs: join(repoRoot, 'backend', 'Cargo.lock'), label: 'backend/Cargo.lock' }
];

function sortedDirEntries(absDir) {
  return readdirSync(absDir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name));
}

function hashFile(hash, absPath, label) {
  hash.update(`F ${label}\n`);
  hash.update(readFileSync(absPath));
  hash.update('\n');
}

function hashSymlink(hash, absPath, label) {
  const target = readlinkSync(absPath);
  hash.update(`L ${label} -> ${target}\n`);
}

function hashDirectory(hash, absDir, labelPrefix) {
  for (const entry of sortedDirEntries(absDir)) {
    if (ignoredNames.has(entry.name)) {
      continue;
    }

    const abs = join(absDir, entry.name);
    const label = `${labelPrefix}/${entry.name}`;
    const stat = lstatSync(abs);

    if (stat.isDirectory()) {
      hash.update(`D ${label}\n`);
      hashDirectory(hash, abs, label);
      continue;
    }

    if (stat.isSymbolicLink()) {
      hashSymlink(hash, abs, label);
      continue;
    }

    if (stat.isFile()) {
      hashFile(hash, abs, label);
      continue;
    }

    hash.update(`O ${label}\n`);
  }
}

function computeInputsFingerprint() {
  const hash = createHash('sha256');
  hash.update('helios-frontend-inputs-v1\n');

  for (const root of inputRoots) {
    if (!existsSync(root.abs)) {
      hash.update(`MISSING ${root.label}\n`);
      continue;
    }

    const stat = lstatSync(root.abs);
    if (stat.isDirectory()) {
      hash.update(`ROOT_DIR ${root.label}\n`);
      hashDirectory(hash, root.abs, root.label);
    } else if (stat.isSymbolicLink()) {
      hash.update(`ROOT_SYMLINK ${root.label}\n`);
      hashSymlink(hash, root.abs, root.label);
    } else if (stat.isFile()) {
      hash.update(`ROOT_FILE ${root.label}\n`);
      hashFile(hash, root.abs, root.label);
    } else {
      hash.update(`ROOT_OTHER ${root.label}\n`);
    }
  }

  return hash.digest('hex');
}

function readStamp() {
  if (!existsSync(stampPath)) {
    return null;
  }
  try {
    return JSON.parse(readFileSync(stampPath, 'utf8'));
  } catch {
    return null;
  }
}

function writeStamp(fingerprint) {
  mkdirSync(dirname(stampPath), { recursive: true });
  const stamp = {
    fingerprint,
    updated_at: new Date().toISOString(),
    output_abs_path: outputDir
  };
  writeFileSync(stampPath, `${JSON.stringify(stamp, null, 2)}\n`, 'utf8');
}

function outputExists() {
  if (!existsSync(outputDir)) {
    return false;
  }
  return readdirSync(outputDir).length > 0;
}

function runFrontendBuild() {
  const result = spawnSync('bun', ['run', 'build'], {
    cwd: frontendDir,
    stdio: 'inherit',
    env: {
      ...process.env,
      SKIP_DOCS_SYNC: process.env.SKIP_DOCS_SYNC ?? '1'
    }
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const fingerprint = computeInputsFingerprint();
const previous = readStamp();
const unchanged = previous?.fingerprint === fingerprint && outputExists();

if (forceRebuild) {
  console.log('[gaia-frontend] FORCE_FRONTEND_BUILD=1 set; rebuilding frontend artifact');
} else if (unchanged) {
  console.log(`[gaia-frontend] no frontend/docs/api changes detected (${fingerprint.slice(0, 12)}), skipping build`);
  process.exit(0);
}

if (checkOnly) {
  if (forceRebuild) {
    console.log('[gaia-frontend] check: rebuild required (forced)');
  } else if (unchanged) {
    console.log('[gaia-frontend] check: up to date');
  } else {
    console.log('[gaia-frontend] check: rebuild required (inputs changed or build output missing)');
  }
  process.exit(0);
}

console.log('[gaia-frontend] changes detected; running bun run build');
runFrontendBuild();
writeStamp(fingerprint);
console.log(`[gaia-frontend] build complete; updated input stamp at ${stampPath}`);
