// The simulated robot's settings: the team and NetworkTables, camera identities, first-time setup
// and field layouts, answered in the device's own shapes (helios-api's `/v1/device/*`,
// `/v1/field-layouts`). The device keeps them; the simulated one keeps them in localStorage, so
// every tab of the browser reads the same document, and announces each change on a
// BroadcastChannel as the device announces it (`settings` events). Anything else is not on the
// simulated robot, and `mockDevice` answers `null` for it.

import { DEFAULT_NT4, roborioAddress } from "#lib/core/robot-facts.js";
import { dispatchSettings } from "#lib/core/sync.js";
import { demoDocument, documentFromWpilib, type WpilibFieldLayout } from "#lib/stores/field-layout.js";
import type { FieldLayoutDocument, FieldLayoutList, Nt4Settings, SettingsChange, SetupRecord, StoredIdentity } from "./types";

const KEY = "helios:mock-device";
const CHANNEL = "helios-mock-device";
const DEMO_ID = "demo-field";

interface Device {
  nt4: Nt4Settings;
  identities: Record<string, StoredIdentity>;
  setup: SetupRecord;
  field: { selected: string; layouts: Record<string, FieldLayoutDocument> };
}

function fresh(): Device {
  return { nt4: { ...DEFAULT_NT4 }, identities: {}, setup: { onboarded: false }, field: { selected: DEMO_ID, layouts: { [DEMO_ID]: demoDocument(DEMO_ID) } } };
}

function read(): Device {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) return { ...fresh(), ...(JSON.parse(raw) as Partial<Device>) };
  } catch {
    // Storage unavailable (private window): the simulated device starts fresh.
  }
  return fresh();
}

function write(device: Device, domain: string, key: string) {
  try {
    localStorage.setItem(KEY, JSON.stringify(device));
  } catch {
    // Not kept: the next tab starts fresh.
  }
  const change: SettingsChange = { domain, key };
  channel()?.postMessage(change);
  dispatchSettings(change);
}

let broadcast: BroadcastChannel | null | undefined;

/** The channel the other tabs announce on (opened once, when the simulated device is first used). */
function channel(): BroadcastChannel | null {
  if (broadcast === undefined) {
    broadcast = typeof BroadcastChannel === "undefined" ? null : new BroadcastChannel(CHANNEL);
    broadcast?.addEventListener("message", (event: MessageEvent<SettingsChange>) => dispatchSettings(event.data));
  }
  return broadcast;
}

/** What the device says about the robot's NetworkTables server: the same rule as helios-robot's `resolve`, without a controller. */
function nt4State(settings: Nt4Settings) {
  const team = settings.team !== null && roborioAddress(settings.team) !== null ? settings.team : null;
  const explicit = settings.server?.trim() || null;
  const server = !settings.enabled ? null : (explicit ?? (team !== null ? roborioAddress(team) : null));
  return {
    settings,
    effective: {
      team: team !== null ? { value: team, source: "user" as const } : { value: null, source: "none" as const },
      nt4_server: !settings.enabled ? { value: null, source: "user" as const } : server !== null ? { value: server, source: "user" as const } : { value: null, source: "none" as const },
    },
    controller: null,
    server,
    connected: false,
    engine_reporting: false,
  };
}

function layoutList(device: Device): FieldLayoutList {
  return {
    selected: device.field.selected,
    layouts: Object.values(device.field.layouts).map((layout) => ({ id: layout.id, name: layout.name, builtin: false, selected: layout.id === device.field.selected })),
  };
}

export interface MockAnswer {
  value: unknown;
}

/** The simulated device's answer to a request, or `null` when the simulated robot has no such route. */
export async function mockDevice(method: string, path: string, options: { query?: Record<string, unknown>; body?: unknown; raw?: BodyInit }): Promise<MockAnswer | null> {
  const device = read();
  const answer = (value: unknown): MockAnswer => ({ value });

  if (path === "/v1/device/nt4") {
    if (method === "GET") return answer(nt4State(device.nt4));
    if (method === "PUT") {
      device.nt4 = options.body as Nt4Settings;
      write(device, "nt4", "settings");
      return answer(nt4State(device.nt4));
    }
    return null;
  }

  const identity = /^\/v1\/device\/identities(?:\/([^/]+))?$/.exec(path);
  if (identity) {
    const id = identity[1] === undefined ? null : decodeURIComponent(identity[1]);
    if (id === null && method === "GET") return answer(device.identities);
    if (id !== null && method === "PUT") {
      device.identities[id] = options.body as StoredIdentity;
      write(device, "identities", id);
      return answer(device.identities);
    }
    if (id !== null && method === "DELETE") {
      delete device.identities[id];
      write(device, "identities", id);
      return answer(undefined);
    }
    return null;
  }

  if (path === "/v1/device/setup") {
    if (method === "GET") return answer(device.setup);
    if (method === "PUT") {
      device.setup = options.body as SetupRecord;
      write(device, "setup", "onboarding");
      return answer(device.setup);
    }
    return null;
  }

  if (path === "/v1/field-layouts") {
    if (method === "GET") return answer(layoutList(device));
    if (method === "POST") {
      const id = String(options.query?.id ?? "");
      const name = String(options.query?.name ?? id);
      const tagSide = Number(options.query?.tag_side_m ?? 0.1651);
      const json = JSON.parse(String(options.raw)) as WpilibFieldLayout;
      device.field.layouts[id] = documentFromWpilib(id, name, json, tagSide);
      write(device, "field_layout", id);
      return answer(layoutList(device).layouts.find((layout) => layout.id === id));
    }
    return null;
  }

  if (path === "/v1/field-layouts/selected") {
    if (method === "GET") return answer({ id: device.field.selected });
    if (method === "PUT") {
      const id = String((options.body as { id?: string }).id ?? "");
      if (!device.field.layouts[id]) throw new Error(`no field layout ${id}`);
      device.field.selected = id;
      write(device, "field_layout", "selected");
      return answer({ id });
    }
    return null;
  }

  const layout = /^\/v1\/field-layouts\/([^/]+)$/.exec(path);
  if (layout) {
    const id = decodeURIComponent(layout[1]);
    if (method === "GET") {
      const document = device.field.layouts[id];
      if (!document) throw new Error(`no field layout ${id}`);
      return answer({ ...document, selected: device.field.selected });
    }
    if (method === "DELETE") {
      delete device.field.layouts[id];
      write(device, "field_layout", id);
      return answer(undefined);
    }
  }
  return null;
}
