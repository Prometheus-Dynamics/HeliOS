// A live-map pipeline's saved maps, in its settings on Tune: save a map now, choose the map it
// starts from, see the device refuse to delete the map it starts from, delete the others. The mock
// keeps the maps in the simulated device; the sim target keeps this check out (it saves over the
// live mapper, which needs a running camera).
import { expect, test } from "./fixtures";
import { openScreen, tab } from "./nav";

test.describe("saved maps", () => {
  test("a live map is saved, chosen to start from, refused while in use, and deleted", async ({ page, target }) => {
    test.skip(target !== "mock", "the sim's live mapper needs a running camera");

    await openScreen(page, "Tune");
    await tab(page, "Pipelines").first().click();
    await page.getByText("Live map · front", { exact: true }).first().click();
    await tab(page, "Pipeline settings").first().click();

    // Save now, named.
    const name = page.getByRole("textbox", { name: "Map name" });
    await name.fill("E2E map");
    await page.getByRole("button", { name: "Save now", exact: true }).click();
    await expect(page.getByText("Saved the map as E2E map")).toBeVisible();
    await expect(page.getByRole("button", { name: "Delete E2E map" })).toBeVisible();

    // Keep a map file: a version 1 map is kept under its file name; a file that is not a map is
    // refused, and the device's reason is shown.
    const header = Buffer.alloc(22 + 13);
    Buffer.from("EIDMAP\0\0").copy(header, 0);
    header.writeUInt16LE(1, 8);
    header.writeUInt16LE(13, 20);
    header.write("eidos-mapping", 22);
    await page.getByLabel("Map file").setInputFiles({ name: "Lab.eidmap", mimeType: "application/octet-stream", buffer: header });
    await expect(page.getByText("Kept the map Lab")).toBeVisible();
    await page.getByLabel("Map file").setInputFiles({ name: "photo.eidmap", mimeType: "application/octet-stream", buffer: Buffer.from("PNG, not a map") });
    await expect(page.getByText("not a saved map", { exact: false })).toBeVisible();

    // Choose it as the map the pipeline starts from.
    const starts = page.getByRole("combobox").filter({ has: page.locator("option", { hasText: "E2E map" }) });
    await starts.selectOption({ label: "E2E map" });
    await expect(starts.locator("option:checked")).toHaveText("E2E map");

    // The device refuses to delete a map a pipeline starts from, and says which.
    await page.getByRole("button", { name: "Delete E2E map" }).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByText("start from map", { exact: false })).toBeVisible();
    await page.getByRole("button", { name: "Keep", exact: true }).click();

    // Start from no map again, then delete it.
    await starts.selectOption({ label: "No map" });
    await expect(starts.locator("option:checked")).toHaveText("No map");
    await page.getByRole("button", { name: "Delete E2E map" }).click();
    await page.getByRole("button", { name: "Delete", exact: true }).click();
    await expect(page.getByRole("button", { name: "Delete E2E map" })).toBeHidden();
  });
});
