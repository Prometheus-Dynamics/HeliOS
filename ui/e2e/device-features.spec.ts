// A device's own features are on screen only where the device has them (`GET /v1/device`,
// `features`): the LED ring, security, reboot and safe mode, the boot slots and the recovery ladder.
// The mock has two devices: `?mock=1` is the Raze image (every feature) and `?mock=systemcore` a
// SystemCore guest (none of them). The sim (HELIOS_E2E_URL) is checked against what it reports.
import { expect, test, type Page } from "./fixtures";
import { openPane } from "./nav";
import type { Capabilities } from "../src/lib/api/types";

/** A section by its title (the `Section` header's text). */
function section(page: Page, title: string) {
  return page.locator("section.sec").filter({ has: page.locator(".title", { hasText: new RegExp(`^${title}$`) }) });
}

function reboot(page: Page) {
  return page.getByRole("button", { name: "Reboot", exact: true });
}

// The mock's robot data (`static/mock/robot`, gitignored) may be absent: the two fetches for it then
// 404, which the mock tests here do not depend on.
const MOCK_ROBOT_DATA = /\/mock\/robot\//;

test.describe("the Raze image (mock)", () => {
  test.skip(({ target }) => target !== "mock", "the mock's devices");
  test.use({ allowConsole: [MOCK_ROBOT_DATA] });

  test("has its LED ring, security, reboot, boot slots and recovery ladder", async ({ page }) => {
    await openPane(page, "Device", "Hardware");
    await expect(section(page, "LEDs")).toBeVisible();
    await openPane(page, "Device", "Robot link");
    await expect(section(page, "Security")).toBeVisible();
    await openPane(page, "Device", "Software");
    await expect(reboot(page)).toBeVisible();
    await expect(section(page, "Boot slots")).toBeVisible();
    await expect(section(page, "Recovery")).toBeVisible();
  });
});

test.describe("a SystemCore guest (mock)", () => {
  test.skip(({ target }) => target !== "mock", "the mock's devices");
  test.use({ allowConsole: [MOCK_ROBOT_DATA] });

  test("has no LED ring, security, reboot, slots or recovery ladder, and lists its own services", async ({ page }) => {
    await page.goto("/?mock=systemcore");
    await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
    await openPane(page, "Device", "Hardware");
    await expect(section(page, "LEDs")).toHaveCount(0);
    await openPane(page, "Device", "Robot link");
    await expect(section(page, "Security")).toHaveCount(0);
    await openPane(page, "Device", "Software");
    await expect(reboot(page)).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Safe mode", exact: true })).toHaveCount(0);
    await expect(section(page, "Boot slots")).toHaveCount(0);
    await expect(section(page, "Recovery")).toHaveCount(0);
    await expect(page.locator(".svc .nm", { hasText: "helios-orion" })).toBeVisible();
  });
});

test.describe("the device (HELIOS_E2E_URL)", () => {
  test.skip(({ target }) => target !== "sim", "the device itself");

  test("the UI shows exactly the features the device reports", async ({ page, request }) => {
    const response = await request.get("/v1/device");
    expect(response.ok(), `GET /v1/device: HTTP ${response.status()}`).toBeTruthy();
    const capabilities = (await response.json()) as Partial<Capabilities>;
    expect(capabilities.features, "the device reports its features (an older helios-api does not: redeploy it)").toBeDefined();
    const features = capabilities.features!;
    await openPane(page, "Device", "Hardware");
    await expect(section(page, "LEDs")).toHaveCount(features.leds ? 1 : 0);
    await openPane(page, "Device", "Robot link");
    await expect(section(page, "Security")).toHaveCount(features.security ? 1 : 0);
    await openPane(page, "Device", "Software");
    await expect(reboot(page)).toHaveCount(features.reboot ? 1 : 0);
    await expect(section(page, "Recovery")).toHaveCount(features.recovery_ladder ? 1 : 0);
  });
});
