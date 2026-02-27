import { execSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, '..', '..');
const docsDir = join(repoRoot, 'docs');
const docsBuildDir = join(docsDir, 'build');
const docsDocusaurusBin = join(docsDir, 'node_modules', '.bin', 'docusaurus');
const frontendStaticDocsDir = join(repoRoot, 'frontend', 'static', 'docs');
const baseUrl = '/docs/';

const run = (cmd, cwd) => {
  execSync(cmd, { cwd, stdio: 'inherit', env: { ...process.env, DOCUSAURUS_BASE_URL: baseUrl } });
};

function ensureDocsTooling() {
  if (existsSync(docsDocusaurusBin)) {
    return;
  }
  console.log(`Installing docs dependencies in ${docsDir}`);
  run('bun install --frozen-lockfile', docsDir);
  if (!existsSync(docsDocusaurusBin)) {
    throw new Error(`Docusaurus binary not found at ${docsDocusaurusBin} after install.`);
  }
}

function ensureDocsBuilt() {
  ensureDocsTooling();
  console.log(`Building docs in ${docsDir} with baseUrl=${baseUrl}`);
  run('bun run build', docsDir);
}

function syncDocs() {
  if (!existsSync(docsBuildDir)) {
    throw new Error(`Docs build output not found at ${docsBuildDir}. Run the Docusaurus build first.`);
  }
  const normalizedBase = baseUrl.replace(/\/$/, '');
  const baseSegment = normalizedBase.replace(/^\//, '');
  const sourceDir =
    baseSegment && existsSync(join(docsBuildDir, baseSegment)) ? join(docsBuildDir, baseSegment) : docsBuildDir;
  rmSync(frontendStaticDocsDir, { recursive: true, force: true });
  mkdirSync(frontendStaticDocsDir, { recursive: true });
  cpSync(sourceDir, frontendStaticDocsDir, { recursive: true });
  console.log(`Copied docs build from ${sourceDir} to ${frontendStaticDocsDir}`);
}

ensureDocsBuilt();
syncDocs();
