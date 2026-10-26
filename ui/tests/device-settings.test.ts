import { beforeEach, expect, test } from "bun:test";
import { mockDevice } from "#lib/api/mock-device.js";
import type { Nt4Settings, Nt4State, StoredIdentity } from "#lib/api/types.js";
import { roborioAddress } from "#lib/core/robot-facts.js";
import { dispatchSettings, onSettingsChange } from "#lib/core/sync.js";
import { demoDocument, demoLayout, layoutFromDocument, layoutToWpilib, type FieldLayout } from "#lib/stores/field-layout.js";

// Bun has no localStorage: the simulated device keeps its document there.
const memory = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (key: string) => memory.get(key) ?? null,
  setItem: (key: string, value: string) => void memory.set(key, value),
  removeItem: (key: string) => void memory.delete(key),
};

beforeEach(() => memory.clear());

const answer = async (method: string, path: string, options: { query?: Record<string, unknown>; body?: unknown; raw?: BodyInit } = {}) => {
  const result = await mockDevice(method, path, options);
  if (!result) throw new Error(`no route ${method} ${path}`);
  return result.value;
};

test("roboRIO address: 10.TE.AM.2 for teams with one, none otherwise", () => {
  expect(roborioAddress(6390)).toBe("10.63.90.2");
  expect(roborioAddress(254)).toBe("10.2.54.2");
  expect(roborioAddress(0)).toBeNull();
  expect(roborioAddress(25_600)).toBeNull();
});

test("a settings change reaches the handlers of its domain only, until unsubscribed", () => {
  const seen: string[] = [];
  const stop = onSettingsChange("identities", (key) => seen.push(`identities:${key}`));
  const other = onSettingsChange("setup", (key) => seen.push(`setup:${key}`));
  dispatchSettings({ domain: "identities", key: "camera.a" });
  dispatchSettings({ domain: "setup", key: "onboarding" });
  expect(seen).toEqual(["identities:camera.a", "setup:onboarding"]);
  stop();
  other();
  dispatchSettings({ domain: "identities", key: "camera.b" });
  expect(seen).toHaveLength(2);
});

test("the simulated device resolves the team and the server like the device does", async () => {
  // Nothing set: no team, no server.
  let nt = (await answer("GET", "/v1/device/nt4")) as Nt4State;
  expect(nt.effective.team).toEqual({ value: null, source: "none" });
  expect(nt.effective.nt4_server).toEqual({ value: null, source: "none" });

  // A team gives the roboRIO address, set by the user.
  const team: Nt4Settings = { enabled: true, team: 6390, server: null, port: 5810, prefix: "/helios" };
  nt = (await answer("PUT", "/v1/device/nt4", { body: team })) as Nt4State;
  expect(nt.effective.team).toEqual({ value: 6390, source: "user" });
  expect(nt.effective.nt4_server).toEqual({ value: "10.63.90.2", source: "user" });
  expect(nt.server).toBe("10.63.90.2");

  // An explicit server beats the team; switching publishing off has no server at all.
  nt = (await answer("PUT", "/v1/device/nt4", { body: { ...team, server: " 10.0.0.5 " } })) as Nt4State;
  expect(nt.effective.nt4_server.value).toBe("10.0.0.5");
  nt = (await answer("PUT", "/v1/device/nt4", { body: { ...team, enabled: false } })) as Nt4State;
  expect(nt.effective.nt4_server.value).toBeNull();
  expect(nt.effective.team.value).toBe(6390);
});

test("identities are kept by object id, and forgotten on delete", async () => {
  const camera: StoredIdentity = { name: "Front", icon: "camera", color: 3 };
  await answer("PUT", "/v1/device/identities/camera.raze-front.cam0", { body: camera });
  expect((await answer("GET", "/v1/device/identities") as Record<string, StoredIdentity>)["camera.raze-front.cam0"]).toEqual(camera);
  await answer("DELETE", "/v1/device/identities/camera.raze-front.cam0");
  expect(await answer("GET", "/v1/device/identities")).toEqual({});
});

test("first-time setup is kept until it is done", async () => {
  expect(await answer("GET", "/v1/device/setup")).toEqual({ onboarded: false });
  await answer("PUT", "/v1/device/setup", { body: { onboarded: true } });
  expect(await answer("GET", "/v1/device/setup")).toEqual({ onboarded: true });
});

test("a field layout uploaded as WPILib JSON is the selected layout, read back as the editor's", async () => {
  const edited: FieldLayout = { ...demoLayout(), name: "Practice field", tags: demoLayout().tags.slice(0, 4).map((t) => ({ ...t, yaw: 37.5, pitch: -4 })) };
  const json = layoutToWpilib(edited);
  await answer("POST", "/v1/field-layouts", { query: { id: "practice", name: "Practice field", tag_side_m: edited.tagSize }, raw: JSON.stringify(json) });
  await answer("PUT", "/v1/field-layouts/selected", { body: { id: "practice" } });

  const list = (await answer("GET", "/v1/field-layouts")) as { selected: string; layouts: { id: string }[] };
  expect(list.selected).toBe("practice");
  const back = layoutFromDocument((await answer("GET", "/v1/field-layouts/practice")) as Parameters<typeof layoutFromDocument>[0]);
  expect(back.name).toBe("Practice field");
  expect(back.tags).toHaveLength(4);
  expect(back.tags[0].yaw).toBeCloseTo(37.5, 1);
  expect(back.tags[0].pitch).toBeCloseTo(-4, 1);
  expect(back.tags[0].x).toBeCloseTo(edited.tags[0].x, 3);
  expect(list.layouts.map((l) => l.id)).toContain("demo-field");
});

test("the demo field is a device layout with every demo tag", () => {
  const doc = demoDocument("demo-field");
  expect(layoutFromDocument(doc).tags).toHaveLength(demoLayout().tags.length);
});

test("a route the simulated robot does not have is not answered", async () => {
  expect(await mockDevice("GET", "/v1/no-such-thing", {})).toBeNull();
});
