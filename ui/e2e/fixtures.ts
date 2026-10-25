// Shared fixtures for the end-to-end suite.
//
// - `page` is Playwright's page, opened at the target's start URL, and it fails the test on any
//   console error or uncaught page error (the allow-list below is the only exception).
// - `target` is "mock" or "sim", from the project. Sim tests check the device with `device.ts`.
import { expect, test as base, type ConsoleMessage, type Page } from "@playwright/test";

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
];

function allowed(text: string) {
  return ALLOWED_CONSOLE.some((entry) => entry.pattern.test(text));
}

export const test = base.extend<{
  page: Page;
  target: Target;
  /** Per test: console errors a test expects (a device condition it waits out), on top of the allow-list. */
  allowConsole: RegExp[];
}>({
  allowConsole: [[], { option: true }],

  target: async ({}, use, testInfo) => {
    await use(testInfo.project.name === "sim" ? "sim" : "mock");
  },

  page: async ({ page, target, allowConsole }, use) => {
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
    // Every test starts past the first-run wizard (a per-browser flag, `helios:onboarded`). The
    // wizard has its own test. A test that needs it fresh clears the flag itself.
    await page.addInitScript(() => {
      try {
        // A test that wants the wizard sets this session flag first (see setup.spec.ts).
        if (sessionStorage.getItem("e2e:no-onboarding-seed") !== null) return;
        if (localStorage.getItem("helios:onboarded") === null) localStorage.setItem("helios:onboarded", "true");
      } catch {
        // Storage can be blocked: the wizard then shows, and the test's scrim check fails loudly.
      }
    });
    // The mock runs without a device: `?mock=1` is its switch. The sim is the device itself.
    await page.goto(target === "mock" ? "/?mock=1" : "/");
    await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
    await use(page);
    expect(problems, "console errors and uncaught page errors during the test").toEqual([]);
  },

});

export { expect };
