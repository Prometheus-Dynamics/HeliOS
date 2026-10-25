// The graph editor: selecting a node shows its settings, Find centres it, and Auto-arrange moves
// nodes without asking for a deploy (positions are not part of what a deploy changes).
import { expect, test } from "./fixtures";
import { applyAprilTag, tuneRunningPipeline } from "./flows";

test.describe("graph editor", () => {
  test("select, find, auto-arrange", async ({ page, target }) => {
    await applyAprilTag(page);
    await tuneRunningPipeline(page);

    // A node both graphs have: the sim's AprilTag detector is one node, the mock's is a group.
    const name = target === "sim" ? "detector" : "Decode";
    const canvas = page.getByRole("application", { name: "Pipeline graph" });
    await expect(canvas).toBeVisible();
    const deploy = page.getByRole("button", { name: /^Deploy as revision/ });
    await expect(deploy).toBeDisabled();

    // Select a node by clicking its banner: the inspector shows its settings.
    const decode = canvas.getByRole("group").filter({ has: page.getByRole("banner").filter({ hasText: name }) }).first();
    await decode.click();
    await expect(decode).toHaveAttribute("class", /selected/);

    // Find: the node the search matches is brought into view.
    const find = page.getByRole("searchbox", { name: "Find a node by label or type" });
    await find.fill(name);
    await find.press("Enter");
    const box = await decode.boundingBox();
    const view = await canvas.boundingBox();
    expect(box && view, "the node and the canvas have boxes").toBeTruthy();
    expect(box!.x).toBeGreaterThanOrEqual(view!.x - 1);
    expect(box!.x + box!.width).toBeLessThanOrEqual(view!.x + view!.width + 1);

    // Auto-arrange moves nodes; nothing was edited, so there is still nothing to deploy.
    const before = await decode.boundingBox();
    await page.getByRole("button", { name: "Auto-arrange" }).click();
    await expect.poll(async () => (await decode.boundingBox())?.x).not.toBe(before?.x);
    await expect(deploy).toBeDisabled();
    await expect(page.getByText("Unsaved tuning")).toBeHidden();
  });
});
