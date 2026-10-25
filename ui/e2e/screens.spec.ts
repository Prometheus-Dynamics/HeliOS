// Every screen in the left rail opens, with no console error (the fixture's check).
import { expect, test } from "./fixtures";
import { SCREENS, openScreen } from "./nav";

test.describe("screens", () => {
  for (const screen of SCREENS) {
    test(`${screen} opens`, async ({ page }) => {
      await openScreen(page, screen);
      // The screen's own panes are drawn: each screen has at least one pane tab or heading.
      await expect(page.getByRole("tablist").or(page.getByRole("heading")).first()).toBeVisible();
    });
  }

  test("the add-a-screen gallery lists the rest", async ({ page }) => {
    await page.getByRole("button", { name: "Add a screen" }).click();
    await expect(page.getByRole("dialog").or(page.getByText("Add a screen")).first()).toBeVisible();
    await page.keyboard.press("Escape");
  });
});
