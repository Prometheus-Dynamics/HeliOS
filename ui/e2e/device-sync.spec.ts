// Live sync: a setting changed in one tab reaches every other open tab, with no reload.
//
// The sim is one device with two browser contexts, two separate browsers on the same device. The
// mock keeps its settings in one browser, so its two tabs are two pages of one context (they share
// its storage and its BroadcastChannel, as two tabs of one browser do).
import type { Browser, BrowserContext, Page } from "@playwright/test";
import { expect, setOnboarded, test, type Target } from "./fixtures";
import { openScreen } from "./nav";

/** Skips the first-run wizard in a mock tab (the simulated robot's document is in this browser). */
function seedMockDevice() {
  if (localStorage.getItem("helios:mock-device") === null) localStorage.setItem("helios:mock-device", JSON.stringify({ setup: { onboarded: true } }));
}

async function openTabs(browser: Browser, baseURL: string, target: Target): Promise<{ a: Page; b: Page; contexts: BrowserContext[] }> {
  const contexts: BrowserContext[] = [];
  if (target === "mock") {
    const shared = await browser.newContext({ baseURL });
    await shared.addInitScript(seedMockDevice);
    contexts.push(shared);
  } else {
    contexts.push(await browser.newContext({ baseURL }), await browser.newContext({ baseURL }));
  }
  const a = await contexts[0].newPage();
  const b = await contexts[target === "mock" ? 0 : 1].newPage();
  if (target === "sim") {
    await setOnboarded(a, target, true);
    await setOnboarded(b, target, true);
  }
  const start = target === "mock" ? "/?mock=1" : "/";
  await a.goto(start);
  await b.goto(start);
  for (const page of [a, b]) await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
  return { a, b, contexts };
}

test.describe("live sync", () => {
  test("a team set in one tab shows in the other without a reload", async ({ browser, target }, testInfo) => {
    const baseURL = testInfo.project.use.baseURL as string;
    const { a, b, contexts } = await openTabs(browser, baseURL, target);
    // The device's own settings are put back as they were (the sim is a device the others use too).
    const before = target === "sim" ? await (await a.request.get("/v1/device/nt4")).json() : null;
    try {
      await openScreen(a, "Settings");
      await openScreen(b, "Settings");
      const teamInA = a.getByLabel("Team number", { exact: true });
      await expect(teamInA).toBeVisible();
      await teamInA.fill("4321");
      await teamInA.press("Tab");
      // The other tab, which nobody touched, shows the team and where robot code is.
      await expect(b.getByLabel("Team number", { exact: true })).toHaveValue("4321", { timeout: 15_000 });
      await expect(b.getByText("10.43.21.2", { exact: true })).toBeVisible();
    } finally {
      if (before) await a.request.put("/v1/device/nt4", { data: before.settings });
      for (const context of contexts) await context.close();
    }
  });

  test("publishing switched off in one tab shows as off in the other", async ({ browser, target }, testInfo) => {
    const baseURL = testInfo.project.use.baseURL as string;
    const { a, b, contexts } = await openTabs(browser, baseURL, target);
    const before = target === "sim" ? await (await a.request.get("/v1/device/nt4")).json() : null;
    try {
      await openScreen(a, "Settings");
      await openScreen(b, "Settings");
      await expect(b.getByRole("switch", { name: "Publish to NetworkTables" })).toBeChecked({ timeout: 15_000 });
      await a.getByRole("switch", { name: "Publish to NetworkTables" }).click();
      await expect(b.getByRole("switch", { name: "Publish to NetworkTables" })).not.toBeChecked({ timeout: 15_000 });
    } finally {
      if (before) await a.request.put("/v1/device/nt4", { data: before.settings });
      for (const context of contexts) await context.close();
    }
  });
});
