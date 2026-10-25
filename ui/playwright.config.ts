import { defineConfig, devices } from "@playwright/test";

// Two targets, both Chromium only (see e2e/README.md):
//   mock  the UI's built-in simulated robot (`?mock=1`) on the Vite dev server this config starts;
//   sim   a running HeliOS sim (tools/sim/up.sh), at HELIOS_E2E_URL.
// `process` is read through globalThis, as vite.config.ts does: the app's type check has no Node types.
const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env ?? {};
const SIM_URL = env.HELIOS_E2E_URL ?? "http://127.0.0.1:5811";
const MOCK_URL = "http://localhost:5899";

export default defineConfig({
  testDir: "./e2e",
  // The UI is one shared device (or one mock per page): tests run one after another.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  forbidOnly: !!env.CI,
  timeout: 120_000,
  expect: { timeout: 10_000 },
  reporter: [["list"], ["html", { open: "never", outputFolder: "playwright-report" }]],
  outputDir: "test-results",
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    viewport: { width: 1600, height: 1000 },
  },
  // The mock target's server. The sim target never uses it (reuseExistingServer keeps a dev server running).
  webServer: {
    command: "bun run dev --port 5899 --strictPort",
    url: `${MOCK_URL}/?mock=1`,
    reuseExistingServer: !env.CI,
    timeout: 60_000,
    stdout: "ignore",
    stderr: "pipe",
  },
  projects: [
    {
      name: "mock",
      use: { ...devices["Desktop Chrome"], baseURL: MOCK_URL },
    },
    {
      name: "sim",
      use: { ...devices["Desktop Chrome"], baseURL: SIM_URL },
    },
  ],
});
