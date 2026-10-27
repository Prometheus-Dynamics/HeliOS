// Every job in the left rail opens with its panes, with no console error (the fixture's check), and
// the shell's jumps land on the job a control lives on.
import { expect, test } from "./fixtures";
import { SCREENS, openScreen, tab } from "./nav";

/** The tabs each job starts with (screens.ts): the first of each stack is the one in front. */
const PANES: Record<(typeof SCREENS)[number], string[]> = {
  "Set up": ["Setup checklist", "Cameras", "Calibration", "Mounts", "Robot 3D", "Field", "Board maker"],
  Run: ["All cameras", "Pose", "Health"],
  Tune: ["Cameras", "Pipelines", "Nodes", "Camera", "Playback", "Graph", "Profiler", "Camera controls", "Pipeline settings"],
  Device: ["Devices", "Software", "Hardware", "Processes", "Logs", "Robot link", "Recordings", "This browser"],
};

test.describe("screens", () => {
  for (const screen of SCREENS) {
    test(`${screen} opens with its panes`, async ({ page }) => {
      await openScreen(page, screen);
      for (const pane of PANES[screen]) await expect(tab(page, pane).first()).toBeVisible();
    });
  }

  test("the rail is the four jobs, in order", async ({ page }) => {
    const rail = page.getByRole("navigation", { name: "Screens" });
    await expect(rail.getByRole("button", { name: /^(Set up|Run|Tune|Device)$/ })).toHaveText([...SCREENS]);
  });

  test("the status bar's problems and setup jump to Run and Set up", async ({ page }) => {
    await openScreen(page, "Device");
    await page.getByRole("contentinfo").getByRole("button", { name: /problem|^OK$/ }).click();
    await expect(page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: "Run", exact: true })).toHaveAttribute("class", /active/);
    await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
    await page.getByRole("contentinfo").getByRole("button", { name: /^Setup/ }).click();
    await expect(page.getByRole("navigation", { name: "Screens" }).getByRole("button", { name: "Set up", exact: true })).toHaveAttribute("class", /active/);
  });

  test("the add-a-screen gallery offers a copy of a job", async ({ page }) => {
    await page.getByRole("button", { name: "Add a screen" }).click();
    await expect(page.getByRole("dialog", { name: "Add to HeliOS" })).toBeVisible();
    await expect(page.getByText("Copy of Tune")).toBeVisible();
    await page.keyboard.press("Escape");
  });
});
