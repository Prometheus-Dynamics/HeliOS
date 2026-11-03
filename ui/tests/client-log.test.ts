import { beforeEach, expect, test } from "bun:test";
import { breadcrumbRoute, breadcrumbs, excerpt, queuedReports, reportFailure, resetClientLogForTests } from "../src/lib/api/client-log";

beforeEach(() => resetClientLogForTests());

test("an answer's body excerpt is at most 512 bytes, cut without breaking a character", () => {
  expect(excerpt("short")).toBe("short");
  const long = excerpt("é".repeat(600));
  expect(new TextEncoder().encode(long).length).toBeLessThanOrEqual(512);
  expect(long.endsWith("...")).toBe(true);
  expect(long).not.toContain("�");
});

test("the breadcrumbs keep the last 20 screen changes, and a repeat of the same screen is not one", () => {
  for (let i = 0; i < 25; i++) breadcrumbRoute(`screen${i}`);
  breadcrumbRoute("screen24");
  const kept = breadcrumbs();
  expect(kept.length).toBe(20);
  expect(kept[0]).toContain("route #screen5");
  expect(kept[19]).toContain("route #screen24");
});

test("a failure gets a short id at once, and is queued for the device", () => {
  const id = reportFailure({ kind: "api", method: "GET", path: "/v1/pipelines", status: 500, message: "boom" });
  expect(id).toMatch(/^ui-[0-9a-f]{8}$/);
  const [report] = queuedReports();
  expect(report.id).toBe(id);
  expect(report.kind).toBe("api");
  expect(report.status).toBe(500);
  expect(report.path).toBe("/v1/pipelines");
});

test("a report carries the actions before it, and never more than the ring", () => {
  for (let i = 0; i < 3; i++) breadcrumbRoute(`tab${i}`);
  reportFailure({ kind: "error", message: "thrown" });
  const [report] = queuedReports();
  expect(report.after.length).toBe(3);
  expect(report.after[2]).toContain("#tab2");
});

test("the browser sends at most 30 reports a minute, and the rest still get an id", () => {
  const ids = Array.from({ length: 40 }, () => reportFailure({ kind: "error", message: "loop" }));
  expect(new Set(ids).size).toBe(40);
  expect(queuedReports().length).toBe(30);
});

test("a long message and a long stack are clipped before they are queued", () => {
  reportFailure({ kind: "error", message: "m".repeat(5000), stack: Array.from({ length: 50 }, (_, i) => `frame ${i}`).join("\n") });
  const [report] = queuedReports();
  expect(report.message!.length).toBeLessThanOrEqual(400);
  expect(report.stack!.split("\n").length).toBeLessThanOrEqual(8);
});
