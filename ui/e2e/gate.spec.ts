// The UI gate: every screen, every interactive control, clicked once. Each screen is one test, and
// the screens run in parallel (`--workers`). A screen fails on:
//   - a console error or an uncaught page error (`fixtures.ts`);
//   - an HTTP answer of 500 or more, for any request the page makes;
//   - a layout shift (`PerformanceObserver` "layout-shift") with a source outside the clicked control's
//     subtree, counted from the click until the next control is clicked (three frames after it);
//   - a list whose order changes between two renders (the items it still shows, in their order).
// Mock only: clicking everything on a real device would reboot it. The sim keeps its own specs.
//
// Not clicked: links that leave the app (http, download), and controls that reboot the simulated
// robot, leave the app or sign out (`NOT_CLICKED`). A control that a layer covers is clicked through
// the DOM, so a covered control is still exercised and never skipped.
import { test, expect, type Page } from "./fixtures";
import { SCREENS, openScreen } from "./nav";

const CONTROLS = [
  "button",
  "a[href]",
  "summary",
  "input[type=checkbox]",
  "input[type=radio]",
  "input[type=button]",
  "input[type=submit]",
  "[role=button]",
  "[role=link]",
  "[role=tab]",
  "[role=menuitem]",
  "[role=menuitemcheckbox]",
  "[role=menuitemradio]",
  "[role=option]",
  "[role=switch]",
  "[role=checkbox]",
  "[role=radio]",
  "[role=treeitem]",
  "[data-testid]",
].join(", ");

/** Controls that leave the app or stop the simulated robot for good. */
const NOT_CLICKED = "\\b(reboot|safe mode|sign out|log out|shut ?down)\\b";

/** The lists' order is compared between two renders this long apart at most. */
const RENDER_WAIT_MS = 3000;

/**
 * Installed in the page once: the layout-shift log (buffered, so shifts from load are kept) and the
 * list snapshots. Read through `window.__gate`.
 */
const INIT = () => {
  const entries: { t: number; value: number; nodes: (Node | null)[]; sources: number }[] = [];
  const record = (list: PerformanceEntry[]) => {
    for (const entry of list as (PerformanceEntry & { value: number; sources?: { node: Node | null }[] })[]) {
      const sources = entry.sources ?? [];
      entries.push({ t: entry.startTime, value: entry.value, nodes: sources.map((source) => source.node), sources: sources.length });
    }
  };
  let observer: PerformanceObserver | null = null;
  try {
    observer = new PerformanceObserver((list) => record(list.getEntries()));
    observer.observe({ type: "layout-shift", buffered: true });
  } catch {
    // No layout-shift support: the gate checks nothing for shifts (Chromium has it).
  }
  const describe = (node: Node | null) => {
    if (!node) return "(removed)";
    if (!(node instanceof Element)) return node.nodeName.toLowerCase();
    const cls = typeof node.className === "string" && node.className ? `.${node.className.trim().split(/\s+/)[0]}` : "";
    return `${node.tagName.toLowerCase()}${node.id ? `#${node.id}` : ""}${cls}`;
  };
  const shifts = (since: number, target: Element | null) => {
    if (observer) record(observer.takeRecords());
    return entries
      .filter((entry) => entry.t >= since)
      .map((entry) => ({
        value: entry.value,
        outside: entry.sources === 0 || entry.nodes.some((node) => !node || !target || !target.contains(node)),
        sources: entry.nodes.map(describe),
      }));
  };
  // Each list's items in order, keyed without their live numbers ("12 ms" and "3 ms" are one key).
  const lists = () => {
    const visible = (el: Element) => {
      const rect = el.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0;
    };
    return Array.from(document.querySelectorAll("ul, ol, [role=list], [role=listbox], [role=tree]"))
      .filter((list) => visible(list))
      .map((list) => {
        const seen = new Map<string, number>();
        return Array.from(list.children)
          .filter((item) => visible(item))
          .map((item) => {
            const base = (item.textContent ?? "").replace(/[\d.,:%+\-–\s]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 60);
            const count = (seen.get(base) ?? 0) + 1;
            seen.set(base, count);
            return `${base}#${count}`;
          });
      })
      .filter((keys) => keys.length >= 2);
  };
  (window as unknown as { __gate: unknown }).__gate = { shifts, lists };
};

/**
 * One pass, in the page: wait three frames after the last click, return the shifts since `since`
 * outside the last clicked control, then click the next unclicked control. The click is the
 * control's own `click()`: the handler it has, with no pointer geometry in the way (a covered
 * control still answers). A covered control is counted and reported, not failed.
 */
const PASS = async ({ selector, visitedKeys, denied, since }: { selector: string; visitedKeys: string[]; denied: string; since: number | null }) => {
  const frames = () => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
  if (since !== null) await frames();
  const gate = (window as unknown as { __gate: { shifts: (since: number, target: Element | null) => { value: number; outside: boolean; sources: string[] }[] } }).__gate;
  const previous = document.querySelector("[data-gate-target]");
  const shifts = since !== null && previous ? gate.shifts(since, previous) : [];
  document.querySelectorAll("[data-gate-target]").forEach((el) => el.removeAttribute("data-gate-target"));
  const pattern = new RegExp(denied, "i");
  const visible = (el: Element) => {
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    return rect.width > 0 && rect.height > 0 && style.visibility !== "hidden" && style.display !== "none" && el.closest("[inert], [aria-hidden=true]") === null;
  };
  const counts = new Map<string, number>();
  for (const el of Array.from(document.querySelectorAll(selector))) {
    if (!visible(el) || (el as HTMLButtonElement).disabled) continue;
    if (el.matches("a[href]") && (/^https?:/i.test(el.getAttribute("href") ?? "") || el.hasAttribute("download"))) continue;
    const role = el.getAttribute("role") ?? el.tagName.toLowerCase();
    const testId = el.getAttribute("data-testid");
    const text = (el.getAttribute("aria-label") ?? el.getAttribute("title") ?? el.textContent ?? "").replace(/\s+/g, " ").trim().slice(0, 60);
    const label = testId ? `#${testId}` : text || el.tagName.toLowerCase();
    if (pattern.test(label)) continue;
    // Live numbers (a frame rate, a count) change between renders: one control keeps one key.
    const base = `${role}|${label.replace(/[\d.,:%°·+\-–]+/g, "#").replace(/\s+/g, " ").slice(0, 40)}`;
    const count = (counts.get(base) ?? 0) + 1;
    counts.set(base, count);
    const key = `${base}#${count}`;
    if (visitedKeys.includes(key)) continue;
    const rect = el.getBoundingClientRect();
    const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    const covered = hit !== null && !el.contains(hit) && !hit.contains(el);
    el.setAttribute("data-gate-target", key);
    const clickedAt = performance.now();
    (el as HTMLElement).click();
    return { key, label: `${role} "${label}"`, shifts, clickedAt, covered };
  }
  return { key: "", label: "", shifts, clickedAt: 0, covered: false };
};

/** Opens a screen from the rail, and waits for its change to render. */
async function enter(page: Page, screen: (typeof SCREENS)[number]) {
  await openScreen(page, screen);
  await page.evaluate(() => new Promise<void>((done) => requestAnimationFrame(() => requestAnimationFrame(() => done()))));
}

/** Resolves at the next render of the page (a poll or an event changing the DOM), or after `RENDER_WAIT_MS`. */
const nextRender = (page: Page) =>
  page.evaluate(
    (ms) =>
      new Promise<void>((done) => {
        const observer = new MutationObserver(() => {
          observer.disconnect();
          requestAnimationFrame(() => done());
        });
        observer.observe(document.body, { subtree: true, childList: true, characterData: true, attributes: true });
        setTimeout(() => {
          observer.disconnect();
          done();
        }, ms);
      }),
    RENDER_WAIT_MS,
  );

test.describe("UI gate", () => {
  test.describe.configure({ mode: "parallel" });
  test.skip(({ target }) => target !== "mock", "the gate clicks everything, so it runs on the simulated robot only");

  // The dev server compiles the app on its first page load, which can outlast a test's expectation
  // timeout: this load warms it, with a longer wait, before the first screen is tested.
  test.beforeAll(async ({ browser }, workerInfo) => {
    const context = await browser.newContext({ baseURL: workerInfo.project.use.baseURL });
    // First-run setup is done, as in the fixture's pages (otherwise the wizard covers the screens).
    await context.addInitScript(() => localStorage.setItem("helios:mock-device", JSON.stringify({ setup: { onboarded: true } })));
    const page = await context.newPage();
    // A cold dev server can fail a module import once (Vite optimises its deps on the first load): reload.
    for (let attempt = 1; ; attempt++) {
      await page.goto("/?mock=1", { timeout: 120_000 });
      const shown = await page.getByRole("navigation", { name: "Screens" }).waitFor({ timeout: 60_000 }).then(
        () => true,
        () => false,
      );
      if (shown || attempt === 3) break;
    }
    await context.close();
  });

  for (const screen of SCREENS) {
    test(`${screen}: every control clicks clean, no 5xx, no stray layout shift, lists keep their order`, async ({ page }) => {
      test.setTimeout(240_000);
      const failures: string[] = [];
      const covered: string[] = [];
      page.on("response", (response) => {
        if (response.status() >= 500) failures.push(`HTTP ${response.status()} ${response.request().method()} ${new URL(response.url()).pathname}`);
      });
      // A native dialog would wait for a person: the gate dismisses it, and that is a failure to report.
      page.on("dialog", (dialog) => {
        failures.push(`dialog "${dialog.message()}"`);
        void dialog.dismiss();
      });
      // The page is already open (the fixture); the observer starts now and, buffered, sees earlier shifts.
      await page.evaluate(INIT);

      await enter(page, screen);
      const lists = (p: Page) => p.evaluate(() => (window as unknown as { __gate: { lists: () => string[][] } }).__gate.lists());
      const beforeClicks = await lists(page);

      // One control at a time: each pass checks the shifts after the previous click, then clicks the next.
      const visited = new Set<string>();
      let clicks = 0;
      let clickedLabel = "";
      let since: number | null = null;
      for (let guard = 0; guard < 600; guard++) {
        const pass = await page.evaluate(PASS, { selector: CONTROLS, visitedKeys: [...visited], denied: NOT_CLICKED, since });
        for (const shift of pass.shifts) {
          if (shift.outside) failures.push(`${clickedLabel}: layout shift ${shift.value.toFixed(4)} outside the clicked control (sources ${shift.sources.join(", ") || "none"})`);
        }
        if (!pass.key) break;
        visited.add(pass.key);
        clicks++;
        if (pass.covered) covered.push(pass.label);
        clickedLabel = pass.label;
        since = pass.clickedAt;
      }

      // List order: the lists as the screen opened, and after the clicks, against the next render.
      const afterClicks = await lists(page);
      await nextRender(page);
      const later = await lists(page);
      const order = (before: string[][], after: string[][]) => {
        const problems: string[] = [];
        before.forEach((items, index) => {
          const other = after[index];
          if (!other) return;
          const common = new Set(items.filter((key) => other.includes(key)));
          const first = items.filter((key) => common.has(key));
          const second = other.filter((key) => common.has(key));
          if (first.join("|") !== second.join("|")) problems.push(`list ${index + 1} reorders between renders: ${first.slice(0, 4).join(" , ")} -> ${second.slice(0, 4).join(" , ")}`);
        });
        return problems;
      };
      failures.push(...order(beforeClicks, later).map((problem) => `list order: ${problem}`));
      failures.push(...order(afterClicks, later).map((problem) => `list order: ${problem}`));

      console.log(`[gate] ${screen}: ${clicks} controls clicked, ${covered.length} covered by another element (clicked anyway)`);
      expect(failures, `${screen}: the gate found problems`).toEqual([]);
    });
  }
});
