// Shared fixtures for the end-to-end suite.
//
// - `page` is Playwright's page, opened at the target's start URL, and it fails the test on any
//   console error or uncaught page error (the allow-list below is the only exception).
// - `target` is "mock" or "sim", from the project. Sim tests check the device with `device.ts`.
import { expect, test as base, type ConsoleMessage, type Page } from "@playwright/test";
import { device, restoreDevice, snapshotDevice, waitForCameraReady } from "./device";

export type Target = "mock" | "sim";

/**
 * Console errors the UI is known to emit that are not bugs. Keep this short; add an entry only with
 * a reason, and only when the message comes from outside the UI (a browser or a dev server).
 */
const ALLOWED_CONSOLE: { pattern: RegExp; why: string }[] = [
  // The sim's live camera preview is an MJPEG <img>; when a test navigates away, Chromium aborts
  // the stream and logs the aborted request as a failed resource load.
  { pattern: /ERR_ABORTED|net::ERR_.*(preview|frames|stream)/i, why: "aborted MJPEG preview or stream when a test navigates away" },
  // Browsers ask for /favicon.ico on their own; the UI ships no icon file.
  { pattern: /favicon\.ico/i, why: "browser favicon request" },
  // A running pipeline the device has not planned yet answers 404 to its plan; the graph says so
  // under the canvas, and the browser logs the response as a failed resource load.
  { pattern: /status of 404 .*\/v1\/pipelines\/[^/]+\/plan/i, why: "the device has no plan for the pipeline yet" },
];

function allowed(text: string) {
  return ALLOWED_CONSOLE.some((entry) => entry.pattern.test(text));
}

/**
 * Waits until the UI has no device write in flight (`window.__helios.settled()`, from the cluster
 * store). A page that is already gone, or an older UI without the hook, is nothing to wait for.
 */
export async function settleWrites(page: Page) {
  try {
    await page.evaluate(() => (window as Window & { __helios?: { settled(): Promise<void> } }).__helios?.settled());
  } catch {
    // The page was closed or navigated by the test: no writes of its own are left to wait for.
  }
}

/**
 * Whether first-time setup is done for the target: the simulated robot's document in this browser
 * (mock), or the device's own `/v1/device/setup` (sim). The page must be open on the target's origin.
 */
export async function setOnboarded(page: Page, target: Target, onboarded: boolean) {
  if (target === "mock") {
    await page.evaluate((value) => {
      const raw = localStorage.getItem("helios:mock-device");
      const doc = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
      localStorage.setItem("helios:mock-device", JSON.stringify({ ...doc, setup: { onboarded: value } }));
    }, onboarded);
    return;
  }
  const response = await page.request.put("/v1/device/setup", { data: { onboarded } });
  expect(response.ok(), "PUT /v1/device/setup").toBeTruthy();
}

export const test = base.extend<{
  page: Page;
  target: Target;
  /** Per test: console errors a test expects (a device condition it waits out), on top of the allow-list. */
  allowConsole: RegExp[];
  /** Sim only, automatic: the device as the test found it, put back after the test (see `device.ts`). */
  deviceAsFound: void;
}>({
  allowConsole: [[], { option: true }],

  target: async ({}, use, testInfo) => {
    await use(testInfo.project.name === "sim" ? "sim" : "mock");
  },

  // Set up before the page and torn down after it, so the restore follows the page's last write.
  deviceAsFound: [
    async ({ target, request }, use) => {
      if (target !== "sim") {
        await use();
        return;
      }
      // A device that has just booted: its cameras are not ready until their services are. Waits for
      // each frame camera (up to 3 minutes, with the stage that holds it), so the test starts on a device.
      for (const camera of (await device.cameras(request)).filter((c) => c.frames_endpoint)) {
        await waitForCameraReady(request, camera);
      }
      const before = await snapshotDevice(request);
      await use();
      await restoreDevice(request, before);
    },
    { auto: true },
  ],

  page: async ({ page, target, allowConsole, deviceAsFound: _asFound }, use) => {
    const problems: string[] = [];
    const permitted = (text: string) => allowed(text) || allowConsole.some((pattern) => pattern.test(text));
    page.on("console", (message: ConsoleMessage) => {
      if (message.type() !== "error") return;
      // The URL says which request failed (a generic "Failed to load resource" alone does not).
      const where = message.location().url;
      const text = `console.error: ${message.text()}${where ? ` (${where})` : ""}`;
      if (!permitted(`${message.text()} ${where}`)) problems.push(text);
    });
    page.on("pageerror", (error) => {
      // The stack names the component that threw, which is what a reader of the report needs.
      const frames = (error.stack ?? "").split("\n").slice(0, 4).join(" | ");
      const text = `pageerror: ${error.message}${frames ? ` [${frames}]` : ""}`;
      if (!permitted(error.message)) problems.push(text);
    });
    // Every test starts past the first-run wizard: first-time setup is done on the device (the sim's
    // own `/v1/device/setup`, or the simulated robot's document in this browser). The wizard has its
    // own test, which sets it back (see setup.spec.ts).
    await page.addInitScript(() => {
      try {
        // A test that wants the wizard sets this session flag first (see setup.spec.ts).
        if (sessionStorage.getItem("e2e:no-onboarding-seed") !== null) return;
        const raw = localStorage.getItem("helios:mock-device");
        if (raw === null) localStorage.setItem("helios:mock-device", JSON.stringify({ setup: { onboarded: true } }));
      } catch {
        // Storage can be blocked: the wizard then shows, and the test's scrim check fails loudly.
      }
    });
    if (target === "sim") await setOnboarded(page, target, true);
    // The mock runs without a device: `?mock=1` is its switch. The sim is the device itself.
    await page.goto(target === "mock" ? "/?mock=1" : "/");
    await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
    await use(page);
    // A test ends only once the UI's writes have landed. A page closed with a request in flight cuts
    // the device's handler off mid-way (the device once kept a revision its history never recorded),
    // and the next test would start from a state no one checked.
    await settleWrites(page);
    expect(problems, "console errors and uncaught page errors during the test").toEqual([]);
  },

});

export { expect };
