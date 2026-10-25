// Tuning, by stage: one parameter changed and deployed. The corruption bug that shipped once was a
// deploy that changed other nodes' const inputs (type or value), so on the sim the whole graph is
// compared before and after: only the one edited input may differ.
import { device, enabledPipelineOn, simCamera, type DaedalusValue, type DevicePipeline } from "./device";
import { expect, test } from "./fixtures";
import { applyAprilTag, tuneRunningPipeline } from "./flows";

/** Each node's const inputs by label, plus what must not change (requires, edges). */
function graphOf(p: DevicePipeline) {
  const graph = p.graph!;
  return {
    requires: graph.requires,
    edges: graph.graph.edges,
    consts: Object.fromEntries(graph.graph.nodes.map((n) => [n.label, n.const_inputs])),
  };
}

// The sim's AprilTag graph has the detector's settings; the mock's graph has its stream settings.
// Each target edits one parameter it has, and the sim also tabs through a float with a fraction.
const EDIT = {
  sim: { param: "min_contrast", value: "25", tabThrough: "tag_size_m", tabValue: "0.1651" },
  mock: { param: "max_rate_hz", value: "30", tabThrough: "max_rate_hz", tabValue: "60" },
} as const;

test.describe("tuning", () => {
  test("one stage parameter deploys and nothing else in the graph changes", async ({ page, target }) => {
    await applyAprilTag(page);
    await tuneRunningPipeline(page);

    const search = page.getByRole("textbox", { name: "Search settings" });
    const deploy = page.getByRole("button", { name: "Deploy", exact: true });
    const edit = { ...EDIT[target] };
    // The device keeps its last value across runs: edit to a value it does not hold now.
    const api = page.request;
    const camera = target === "sim" ? await simCamera(api) : null;
    const current = camera ? (await enabledPipelineOn(api, camera.id)).graph!.graph.nodes.find((n) => n.label === "detector")?.const_inputs.find(([name]) => name === "min_contrast")?.[1] : undefined;
    if (current && typeof current.value === "number") edit.value = current.value === 25 ? "26" : "25";

    // Tabbing through a field without typing changes nothing: no draft, nothing to deploy.
    await search.fill(edit.tabThrough);
    const tabField = page.getByRole("textbox", { name: edit.tabThrough, exact: true }).first();
    await expect(tabField).toHaveValue(edit.tabValue);
    await tabField.click();
    await tabField.press("Tab");
    await expect(deploy).toBeHidden();

    await search.fill(edit.param);
    const field = page.getByRole("textbox", { name: edit.param, exact: true });
    await field.click();
    await field.fill(edit.value);
    await field.press("Enter");
    await expect(field).toHaveValue(edit.value);
    await expect(page.getByText("Unsaved tuning")).toBeVisible();

    if (target !== "sim") {
      await deploy.click();
      await expect(page.getByText("Unsaved tuning")).toBeHidden();
      return;
    }

    if (!camera) return;
    const before = graphOf(await enabledPipelineOn(api, camera.id));
    const revisionBefore = (await enabledPipelineOn(api, camera.id)).revision ?? 0;

    await deploy.click();
    await expect(page.getByText("Unsaved tuning")).toBeHidden();

    const id = (await enabledPipelineOn(api, camera.id)).id;
    await expect.poll(async () => (await device.pipeline(api, id)).revision ?? 0).toBeGreaterThan(revisionBefore);
    const after = graphOf(await device.pipeline(api, id));

    // The one edited input: the detector's min_contrast, still an Int.
    const expected = structuredClone(before);
    const detector = expected.consts.detector as [string, DaedalusValue][];
    const index = detector.findIndex(([name]) => name === "min_contrast");
    expect(index, "the detector has min_contrast").toBeGreaterThanOrEqual(0);
    detector[index] = ["min_contrast", { type: "Int", value: Number(edit.value) }];

    expect(after.requires).toEqual(before.requires);
    expect(after.edges).toEqual(before.edges);
    expect(Object.keys(after.consts).sort()).toEqual(Object.keys(before.consts).sort());
    // Byte-for-byte: each node's const inputs, type and value, match except the one edited input.
    expect(JSON.stringify(after.consts)).toBe(JSON.stringify(expected.consts));
  });
});
