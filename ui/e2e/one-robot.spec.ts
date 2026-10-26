// One robot, two HeliOS devices (`?mock=robot`): the Raze image (this device, with its LED ring) and a
// SystemCore on its link (a routed peer, with a camera and a pipeline of its own). Any one UI shows
// both: the Devices pane lists each with its badge, the camera list shows the peer's camera with its
// node's badge, the LED controls are the Raze's, and a change to the peer's camera goes through the
// peer's scope, where the simulated peer keeps the request (`helios:mock-peers`, see mock-device.ts).
import { expect, test, type Page } from "./fixtures";
import { openScreen } from "./nav";

/** A section by its title (the `Section` header's text). */
function section(page: Page, title: string) {
  return page.locator("section.sec").filter({ has: page.locator(".title", { hasText: new RegExp(`^${title}$`) }) });
}

/** A camera's row in the camera list (its name, with the peer's badge when it is a peer's). */
function cameraRow(page: Page, name: RegExp) {
  return page.getByRole("button", { name });
}

/** The requests the simulated peers have had through the gateway, as the mock records them. */
function peerRequests(page: Page) {
  return page.evaluate(() => {
    const raw = localStorage.getItem("helios:mock-peers");
    return raw ? ((JSON.parse(raw) as { requests?: unknown[] }).requests ?? []) : [];
  });
}

// The mock's robot data (`static/mock/robot`, gitignored) may be absent: its fetches 404 here, and the
// tests here do not depend on the frames.
const MOCK_ROBOT_DATA = /\/mock\/robot\//;

test.describe("one robot (mock)", () => {
  test.skip(({ target }) => target !== "mock", "the mock's devices");
  test.use({ allowConsole: [MOCK_ROBOT_DATA] });

  test.beforeEach(async ({ page }) => {
    await page.goto("/?mock=robot");
    await expect(page.getByRole("navigation", { name: "Screens" })).toBeVisible();
  });

  test("the Devices pane lists this device and the SystemCore peer, each with its badge", async ({ page }) => {
    await openScreen(page, "Hardware");
    const owner = page.getByRole("button", { name: /Owner/ });
    const systemcore = page.getByRole("button", { name: /SystemCore/ });
    await expect(owner.getByText("this device", { exact: true })).toBeVisible();
    await expect(systemcore.getByText("peer · routed", { exact: true })).toBeVisible();
  });

  test("the camera list shows the peer's camera with a node badge, and this device's without one", async ({ page }) => {
    await openScreen(page, "Cameras");
    const back = cameraRow(page, /^Back /);
    await expect(back).toBeVisible();
    await expect(back.getByText("SystemCore", { exact: true })).toBeVisible();
    const front = cameraRow(page, /^Front /);
    await expect(front).toBeVisible();
    await expect(front.getByText("SystemCore", { exact: true })).toHaveCount(0);
  });

  test("the LED controls are the Raze's: they show for it and not for the SystemCore", async ({ page }) => {
    await openScreen(page, "Hardware");
    await expect(section(page, "LEDs")).toBeVisible();
    // Selecting the SystemCore in the Devices pane: the Peripherals pane follows the selection.
    await page.getByRole("button", { name: /SystemCore/ }).click();
    await expect(section(page, "LEDs")).toHaveCount(0);
  });

  test("a change to the peer's camera goes to the peer, and the peer keeps it", async ({ page }) => {
    await openScreen(page, "Cameras");
    await cameraRow(page, /^Back /).click();
    // Manual exposure: the change is the peer camera's, so it goes to the peer's own API.
    await page.getByRole("radiogroup", { name: "Exposure mode" }).getByRole("radio", { name: "Manual", exact: true }).click();
    await expect
      .poll(() => peerRequests(page))
      .toContainEqual(
        expect.objectContaining({
          peer: "systemcore-peer",
          method: "PATCH",
          path: "/v1/cameras/camera.systemcore-peer.cam0/settings",
          body: expect.objectContaining({ ae: false }),
        }),
      );
    // Only the peer's camera was sent to a peer: the Raze's own camera never goes through the gateway.
    const requests = (await peerRequests(page)) as { peer: string }[];
    expect(requests.every((r) => r.peer === "systemcore-peer")).toBe(true);
  });
});
