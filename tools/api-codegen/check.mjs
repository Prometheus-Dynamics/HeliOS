#!/usr/bin/env node
import { fileURLToPath } from 'url';
import path from 'path';
import { spawn } from 'child_process';
import { access, rm } from 'fs/promises';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const frontendBindingsRoot = path.join(repoRoot, 'frontend', 'src', 'lib', 'ts-bindings');
const trackedGeneratedPaths = [
  'backend/src/helios-engine/src/ipc/types/generated_codec_families.rs',
  'backend/src/helios-engine/src/contracts/generated_runtime_contracts.rs'
];
const requiredFrontendGeneratedPaths = [
  'frontend/src/lib/ts-bindings/types.ts',
  'frontend/src/lib/ts-bindings/codecFamilies.ts',
  'frontend/src/lib/ts-bindings/runtimeContracts.ts',
  'frontend/src/lib/ts-bindings/http/openapi.json',
  'frontend/src/lib/ts-bindings/http/client/index.ts',
  'frontend/src/lib/ts-bindings/http/client/core/OpenAPI.ts',
  'frontend/src/lib/ts-bindings/ws/asyncapi.json'
];

function run(cmd, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      stdio: 'inherit',
      cwd: repoRoot,
      ...options
    });
    child.on('close', (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      if (signal) {
        reject(new Error(`${cmd} ${args.join(' ')} exited due to signal ${signal}`));
        return;
      }
      reject(new Error(`${cmd} ${args.join(' ')} exited with code ${code}`));
    });
    child.on('error', reject);
  });
}

async function ensureRequiredGeneratedFiles() {
  for (const relativePath of requiredFrontendGeneratedPaths) {
    try {
      await access(path.join(repoRoot, relativePath));
    } catch (_) {
      throw new Error(`missing generated frontend binding ${relativePath}`);
    }
  }
}

async function main() {
  console.log('[api-codegen:check] Removing ignored frontend bindings to simulate a fresh clone');
  await rm(frontendBindingsRoot, { recursive: true, force: true });

  console.log('[api-codegen:check] Regenerating API bindings');
  await run('node', [path.join(__dirname, 'index.mjs')]);

  console.log('[api-codegen:check] Verifying frontend generated binding surface');
  try {
    await ensureRequiredGeneratedFiles();
  } catch (error) {
    console.error(`[api-codegen:check] ${error.message}`);
    process.exit(1);
  }

  console.log('[api-codegen:check] Verifying tracked generated bindings are committed');
  try {
    await run('git', ['diff', '--exit-code', '--', ...trackedGeneratedPaths]);
  } catch (_) {
    console.error('[api-codegen:check] Generated bindings are out of date. Re-run frontend codegen and commit the changes.');
    process.exit(1);
  }

  console.log('[api-codegen:check] Generated bindings are in sync');
}

main().catch((error) => {
  console.error('[api-codegen:check:error]', error.message);
  process.exit(1);
});
