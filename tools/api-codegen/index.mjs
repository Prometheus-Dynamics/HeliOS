#!/usr/bin/env node
import { fileURLToPath } from 'url';
import path from 'path';
import { mkdtemp, mkdir, rm, cp, readFile } from 'fs/promises';
import { tmpdir } from 'os';
import { spawn } from 'child_process';
import { generateCodecFamilies } from './generate-codec-families.mjs';
import { generateRuntimeContracts } from './generate-runtime-contracts.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const backendDir = path.join(repoRoot, 'backend');
const frontendDir = path.join(repoRoot, 'frontend');
const clientTemplateDir = path.join(repoRoot, 'tools', 'api-client');
const overridesDir = path.join(__dirname, 'overrides');
const tsBindingsRoot = path.join(frontendDir, 'src', 'lib', 'ts-bindings');

const SPEC_TIMEOUT_MS = Number.parseInt(process.env.API_CODEGEN_SPEC_TIMEOUT_MS ?? '', 10) || 600_000;
const SPEC_MAX_RSS_BYTES =
  ((Number.parseInt(process.env.API_CODEGEN_SPEC_MAX_RSS_MB ?? '', 10) || 4_096) * 1024) * 1024;

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const index = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

async function readProcessRss(pid) {
  const status = await readFile(`/proc/${pid}/status`, 'utf8');
  const match = status.match(/^VmRSS:\s+(\d+)\s+kB$/m);
  if (!match) return 0;
  return Number.parseInt(match[1], 10) * 1024;
}

async function run(cmd, args, options = {}) {
  return new Promise((resolve, reject) => {
    const { timeoutMs, maxRssBytes, ...spawnOptions } = options;
    const child = spawn(cmd, args, { stdio: 'inherit', ...spawnOptions });
    let timedOut = false;
    let killedForMemory = false;
    let timeoutHandle;
    let monitorHandle;
    let rssCheckInProgress = false;

    const cleanup = () => {
      if (timeoutHandle) {
        clearTimeout(timeoutHandle);
        timeoutHandle = undefined;
      }
      if (monitorHandle) {
        clearInterval(monitorHandle);
        monitorHandle = undefined;
      }
    };

    const terminate = (reason) => {
      if (child.exitCode !== null || child.killed) {
        return;
      }
      console.warn(`[api-codegen:monitor] ${reason}; terminating ${cmd}`);
      child.kill('SIGKILL');
    };

    if (Number.isFinite(timeoutMs) && timeoutMs > 0) {
      timeoutHandle = setTimeout(() => {
        timedOut = true;
        terminate(`timeout ${timeoutMs}ms exceeded`);
      }, timeoutMs);
    }

    if (Number.isFinite(maxRssBytes) && maxRssBytes > 0) {
      monitorHandle = setInterval(async () => {
        if (rssCheckInProgress) return;
        if (child.exitCode !== null || child.killed) return;
        rssCheckInProgress = true;
        try {
          const rss = await readProcessRss(child.pid);
          if (rss > maxRssBytes) {
            killedForMemory = true;
            terminate(`RSS ${formatBytes(rss)} exceeded limit ${formatBytes(maxRssBytes)}`);
          }
        } catch (err) {
          if (err && err.code !== 'ENOENT') {
            console.warn(`[api-codegen:monitor] failed to read RSS for pid ${child.pid}:`, err.message ?? err);
          }
        } finally {
          rssCheckInProgress = false;
        }
      }, 1_000);
    }

    child.on('close', (code, signal) => {
      cleanup();
      if (code === 0) {
        resolve();
      } else {
        if (timedOut) {
          reject(new Error(`${cmd} ${args.join(' ')} terminated after exceeding timeout`));
        } else if (killedForMemory) {
          reject(new Error(`${cmd} ${args.join(' ')} terminated after exceeding RSS limit`));
        } else if (signal) {
          reject(new Error(`${cmd} ${args.join(' ')} exited due to signal ${signal}`));
        } else {
          reject(new Error(`${cmd} ${args.join(' ')} exited with code ${code}`));
        }
      }
    });
    child.on('error', (error) => {
      cleanup();
      reject(error);
    });
  });
}

async function ensureDir(dir) {
  await mkdir(dir, { recursive: true });
}

function log(step, message) {
  const label = `[api-codegen:${step}]`;
  console.log(`${label} ${message}`);
}

async function getWorkTmpRoot() {
  if (process.env.API_CODEGEN_TMPDIR && process.env.API_CODEGEN_TMPDIR.trim().length > 0) {
    return process.env.API_CODEGEN_TMPDIR.trim();
  }

  const candidate = path.join(repoRoot, '.cache', 'api-codegen');
  try {
    await mkdir(candidate, { recursive: true });
    return candidate;
  } catch (_) {
    return tmpdir();
  }
}

function getCargoTargetDirOverride() {
  const override = process.env.API_CODEGEN_CARGO_TARGET_DIR?.trim();
  if (override && override.length > 0) return override;
  return null;
}

function getDefaultCargoTargetDir() {
  return path.join(repoRoot, '.cache', 'api-codegen', 'cargo-target');
}

async function main() {
  const tmpRoot = await getWorkTmpRoot();
  const specDir = await mkdtemp(path.join(tmpRoot, 'helios-spec-'));
  const httpSpec = path.join(specDir, 'http.json');
  const wsSpec = path.join(specDir, 'ws.json');
  const tmpDir = path.join(specDir, 'tmp');
  await mkdir(tmpDir, { recursive: true });

  log('spec', 'Generating OpenAPI + AsyncAPI descriptions via helios-api CLI');
  let cleanupSpecDir = false;
  const cargoEnv = { ...process.env, TMPDIR: tmpDir };
  const cargoTargetDirOverride = getCargoTargetDirOverride();
  const cargoTargetDir = cargoTargetDirOverride ?? getDefaultCargoTargetDir();
  await ensureDir(cargoTargetDir);
  cargoEnv.CARGO_TARGET_DIR = cargoTargetDir;
  await run(
    'cargo',
    ['run', '-p', 'helios-api', '--', 'apispec', '--http', httpSpec, '--ws', wsSpec],
    {
      cwd: backendDir,
      env: cargoEnv,
      timeoutMs: SPEC_TIMEOUT_MS,
      maxRssBytes: SPEC_MAX_RSS_BYTES
    }
  );
  cleanupSpecDir = true;

  log('deps', 'Installing JS dependencies with bun');
  try {
    await run('bun', ['install'], { cwd: clientTemplateDir });
  } catch (error) {
    // Bun can occasionally fail with link-related EEXIST errors when the template's
    // node_modules is in a partially-installed state. Nuke and retry once.
    log('deps', `bun install failed (${error?.message ?? String(error)}); retrying after removing node_modules`);
    await rm(path.join(clientTemplateDir, 'node_modules'), { recursive: true, force: true });
    await run('bun', ['install'], { cwd: clientTemplateDir });
  }

  await ensureDir(tsBindingsRoot);
  const httpBindingsDir = path.join(tsBindingsRoot, 'http');
  await ensureDir(httpBindingsDir);
  const wsBindingsDir = path.join(tsBindingsRoot, 'ws');
  await ensureDir(wsBindingsDir);

  log('types', 'Generating shared TypeScript types');
  await run('bun', ['x', 'openapi-typescript', httpSpec, '-o', path.join(tsBindingsRoot, 'types.ts')], { cwd: clientTemplateDir });

  log('http-client', 'Generating HTTP client bindings');
  const httpClientDir = path.join(httpBindingsDir, 'client');
  await rm(httpClientDir, { recursive: true, force: true });
  await run(
    'bun',
    [
      'x',
      'openapi-typescript-codegen',
      '--input',
      httpSpec,
      '--output',
      httpClientDir,
      '--useOptions',
      '--useUnionTypes'
    ],
    { cwd: clientTemplateDir }
  );

  try {
    await cp(overridesDir, httpClientDir, { recursive: true, force: true, errorOnExist: false });
    log('http-client', 'Applied custom overrides');
  } catch (error) {
    if (error?.code !== 'ENOENT') {
      throw error;
    }
  }

  await cp(httpSpec, path.join(httpBindingsDir, 'openapi.json'), { force: true });
  await cp(wsSpec, path.join(wsBindingsDir, 'asyncapi.json'), { force: true });
  log('codec-families', 'Generating shared codec family bindings');
  await generateCodecFamilies();
  log('runtime-contracts', 'Generating shared runtime contract bindings');
  await generateRuntimeContracts();
  log('done', `Artifacts written to ${path.relative(repoRoot, tsBindingsRoot)}`);

  if (cleanupSpecDir) {
    await rm(specDir, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error('[api-codegen:error]', error.message);
  process.exit(1);
});
