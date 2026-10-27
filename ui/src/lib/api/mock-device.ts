// The simulated robot's settings: the team and NetworkTables, camera identities, first-time setup
// and field layouts, answered in the device's own shapes (helios-api's `/v1/device/*`,
// `/v1/field-layouts`). The device keeps them; the simulated one keeps them in localStorage, so
// every tab of the browser reads the same document, and announces each change on a
// BroadcastChannel as the device announces it (`settings` events). Anything else is not on the
// simulated robot, and `mockDevice` answers `null` for it.

import { DEFAULT_NT4, roborioAddress } from "#lib/core/robot-facts.js";
import { dispatchSettings } from "#lib/core/sync.js";
import { demoDocument, documentFromWpilib, type WpilibFieldLayout } from "#lib/stores/field-layout.js";
import { MOCK_DEVICE } from "./mode";
import { MOCK_PEERS } from "./mock-devices";
import { LIVE_MAP_NODE } from "#lib/maps/mapping.js";
import { ROBOT_WORKLOADS, WORKLOADS } from "./mock";
import type { FieldLayoutDocument, FieldLayoutList, MapEntry, MapList, Nt4Settings, SettingsChange, SetupRecord, StoredIdentity } from "./types";

const KEY = "helios:mock-device";
const CHANNEL = "helios-mock-device";
const DEMO_ID = "demo-field";

interface Device {
  nt4: Nt4Settings;
  identities: Record<string, StoredIdentity>;
  setup: SetupRecord;
  field: { selected: string; layouts: Record<string, FieldLayoutDocument> };
  /** The saved maps (`/v1/maps`): the map files themselves are not simulated, only their entries. */
  maps: MapEntry[];
}

/** The device's limits for saved maps (`maps.rs`): the store's total and one map's size. */
const MAX_TOTAL_MAP_BYTES = 128 * 1024 * 1024;
const MAX_MAP_BYTES = 16 * 1024 * 1024;

/** One practice map, so the Maps section of a live-map pipeline has something to show on a fresh simulated robot. */
function demoMaps(): MapEntry[] {
  const saved = Date.now() - 2 * 24 * 60 * 60 * 1000;
  return [{ id: "map-practice-field", name: "Practice field", saved_at_ms: saved, size_bytes: 1_310_720, pipeline: "live-map-front", camera: "camera.raze-front.cam0", format_version: 1, created_ns: saved * 1_000_000, creator: "eidos-mapping" }];
}

function fresh(): Device {
  return { nt4: { ...DEFAULT_NT4 }, identities: {}, setup: { onboarded: false }, field: { selected: DEMO_ID, layouts: { [DEMO_ID]: demoDocument(DEMO_ID) } }, maps: demoMaps() };
}

/** The map each simulated pipeline starts from (`null`: none), by pipeline id. Kept with the tab's pipelines, as the simulated pipelines are. */
const pipelineMaps = new Map<string, string | null>();

function mapList(device: Device): MapList {
  const maps = [...device.maps].sort((a, b) => b.saved_at_ms - a.saved_at_ms || a.id.localeCompare(b.id));
  return { maps, total_bytes: device.maps.reduce((sum, map) => sum + map.size_bytes, 0), max_total_bytes: MAX_TOTAL_MAP_BYTES, max_map_bytes: MAX_MAP_BYTES };
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

const PEERS_KEY = "helios:mock-peers";

/**
 * A request the simulated robot's peer got through the gateway (`/v1/peers/{id}/...`), kept in this
 * browser so a test can see it reach the peer: the mock peer has no device behind it.
 */
interface PeerRequest {
  peer: string;
  method: string;
  path: string;
  body: unknown;
}

function recordPeerRequest(request: PeerRequest) {
  try {
    const raw = localStorage.getItem(PEERS_KEY);
    const doc = raw ? (JSON.parse(raw) as { requests?: PeerRequest[] }) : {};
    const requests = [...(doc.requests ?? []), request].slice(-100);
    localStorage.setItem(PEERS_KEY, JSON.stringify({ requests }));
  } catch {
    // Not kept: the request still happened on the simulated robot.
  }
}

/** The simulated device's answer to a request, or `null` when the simulated robot has no such route. */
export async function mockDevice(method: string, path: string, options: { query?: Record<string, unknown>; body?: unknown; raw?: BodyInit }): Promise<MockAnswer | null> {
  const device = read();
  const answer = (value: unknown): MockAnswer => ({ value });

  // The robot's peers (the gateway's list, from the Raze's side) and a request routed to one of them.
  if (path === "/v1/peers") return method === "GET" ? answer(MOCK_DEVICE === "robot" ? MOCK_PEERS : { cluster: null, peers: [] }) : null;
  const peer = /^\/v1\/peers\/([^/]+)\/(.+)$/.exec(path);
  if (peer) {
    const id = decodeURIComponent(peer[1]);
    const routed = MOCK_DEVICE === "robot" && MOCK_PEERS.peers.some((p) => p.node_id === id && p.routed);
    if (!routed) throw new Error(`peer ${id} is not routed`);
    if (method === "GET") return null;
    recordPeerRequest({ peer: id, method, path: `/v1/${peer[2]}`, body: options.body ?? null });
    return answer(undefined);
  }

  // A pipeline's plan (`explain_plan`): the simulated engine runs the mask and quad stages on the GPU,
  // so the frame goes up once and the runs come down once, as a real plan would show.
  const plan = /^\/v1\/pipelines\/([^/]+)\/plan$/.exec(path);
  if (plan && method === "GET") {
    const id = decodeURIComponent(plan[1]);
    const w = [...WORKLOADS, ...ROBOT_WORKLOADS].find((x) => x.id === id);
    if (!w) return null;
    const gpu = (nodeId: string) => /mask_prep|quads_from/.test(w.graph.nodes.find((n) => n.id === nodeId)?.type ?? "");
    return answer({
      format: "helios.engine.plan.v1",
      plan: {
        nodes: w.graph.nodes.map((n, index) => ({ index, id: n.id, label: n.label, compute: gpu(n.id) ? "gpu" : "cpu" })),
        edges: w.graph.edges.map((e, index) => {
          const up = !gpu(e.from.node) && gpu(e.to.node);
          const down = gpu(e.from.node) && !gpu(e.to.node);
          return { index, from_node_id: e.from.node, from_port: e.from.port, to_node_id: e.to.node, to_port: e.to.port, transport: "shm", copies_frame: up || down, crosses_residency: up || down, adapter_steps: up ? ["DeviceUpload"] : down ? ["DeviceDownload"] : [] };
        }),
      },
    });
  }

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

  // Saved maps: listed, saved from a live-map pipeline, deleted (refused while a pipeline starts from one).
  if (path === "/v1/maps" && method === "GET") return answer(mapList(device));

  const map = /^\/v1\/maps\/([^/]+)$/.exec(path);
  if (map && method === "DELETE") {
    const id = decodeURIComponent(map[1]);
    if (!device.maps.some((m) => m.id === id)) throw new Error(`no map ${id}`);
    const users = [...pipelineMaps].filter(([, chosen]) => chosen === id).map(([pipeline]) => pipeline);
    if (users.length) throw new Error(`pipelines ${users.join(", ")} start from map ${id}; choose another map for them first`);
    device.maps = device.maps.filter((m) => m.id !== id);
    write(device, "maps", id);
    return answer(undefined);
  }

  const save = /^\/v1\/pipelines\/([^/]+)\/map\/save$/.exec(path);
  if (save && method === "POST") {
    const id = decodeURIComponent(save[1]);
    const w = [...WORKLOADS, ...ROBOT_WORKLOADS].find((x) => x.id === id);
    if (!w) throw new Error(`no pipeline ${id}`);
    if (!w.graph.nodes.some((n) => n.type === LIVE_MAP_NODE)) throw new Error(`pipeline ${id} has no live map to save`);
    const size = 1_100_000 + Math.floor(Math.random() * 400_000);
    const used = mapList(device).total_bytes;
    if (used + size > MAX_TOTAL_MAP_BYTES) throw new Error(`the maps take ${used} of ${MAX_TOTAL_MAP_BYTES} bytes and this map needs ${size}: delete a map first`);
    const now = Date.now();
    const mapId = `map-${now.toString(36)}${Math.random().toString(36).slice(2, 6)}`;
    const name = ((options.body as { name?: string } | undefined)?.name ?? "").trim();
    const entry: MapEntry = {
      id: mapId,
      name: name || mapId,
      saved_at_ms: now,
      size_bytes: size,
      pipeline: id,
      camera: Object.values(w.bindings)[0] ?? null,
      format_version: 1,
      created_ns: now * 1_000_000,
      creator: "eidos-mapping (simulated)",
    };
    device.maps = [...device.maps, entry];
    write(device, "maps", entry.id);
    return answer(entry);
  }

  // A pipeline's spec: only the map it starts from is kept here (the simulated engine runs no graph).
  const pipeline = /^\/v1\/pipelines\/([^/]+)$/.exec(path);
  if (pipeline && method === "PUT") {
    const id = decodeURIComponent(pipeline[1]);
    const chosen = (options.body as { map?: string | null } | undefined)?.map ?? null;
    if (chosen !== null && !device.maps.some((m) => m.id === chosen)) throw new Error(`no map ${chosen}`);
    pipelineMaps.set(id, chosen);
    return answer(undefined);
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
