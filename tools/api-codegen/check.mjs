#!/usr/bin/env node
import { fileURLToPath } from 'url';
import path from 'path';
import { spawn } from 'child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const generatedPaths = [
  'frontend/src/lib/ts-bindings',
  'backend/src/helios-engine/src/ipc/types/generated_codec_families.rs',
  'backend/src/helios-engine/src/contracts/generated_runtime_contracts.rs'
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

async function main() {
  console.log('[api-codegen:check] Regenerating API bindings');
  await run('node', [path.join(__dirname, 'index.mjs')]);

  console.log('[api-codegen:check] Verifying generated bindings are committed');
  try {
    await run('git', ['diff', '--exit-code', '--', ...generatedPaths]);
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
