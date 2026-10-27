// The robot's peers: a peer's path through the gateway, a peer's node, a peer that does not answer,
// and the merge of the peers' lists with this device's (each item keeps its node id).
import { describe, expect, test } from "bun:test";
import { api, peerPath, scoped } from "../src/lib/api/client";
import { deviceId, entityKey, peerNode, routedPeers, unreachableNode } from "../src/lib/api/peers";
import { toWorkload } from "../src/lib/api/adapt";
import { isReachable, placeOf } from "../src/lib/devices";
import type { ClusterNode } from "../src/lib/api/model";
import type { Device, FeatureName, Peer } from "../src/lib/api/types";

const FEATURES: FeatureName[] = ["updates", "slots", "reboot", "safe_mode", "recovery_ladder", "leds", "fan", "imu", "power", "usb_power", "security", "console"];

/** A peer's device as its own API answers it (only the fields the node mapping reads). */
function deviceOf(node: string, kind: string): Device {
  const features = Object.fromEntries(FEATURES.map((f) => [f, false]));
  return {
    node_id: node,
    hostname: "systemcore",
    model: "Compute Module 5",
    profile: "guest",
    host: { kind, name: null, release: null },
    features,
    reasons: {},
    os: { name: "LIMELIGHTOS_SYSTEMCORE", version: "2027.0.0" },
    uptime_s: 120,
    api: "0.3.0",
    orion: { reachable: true, desired_revision: null, observed_revision: null, peers_ready: null, peers_configured: null, message: null },
    clock: null,
  } as unknown as Device;
}

function peerOf(node: string, overrides: Partial<Peer> = {}): Peer {
  return { node_id: node, state: "enrolled", address: "172.27.0.1", api_port: 5800, key_fingerprint: "sha256:x", needs_enrollment: false, routed: true, ...overrides };
}

const snapshot = (device: Device) => ({ device, identity: null, metrics: null, services: [], update: null });

describe("a peer's path", () => {
  test("is the device's own path under the peer's prefix", () => {
    expect(peerPath("systemcore", "/v1/cameras/camera.systemcore.cam0/settings")).toBe("/v1/peers/systemcore/cameras/camera.systemcore.cam0/settings");
  });

  test("encodes the node id", () => {
    expect(peerPath("a/b", "/v1/pipelines")).toBe("/v1/peers/a%2Fb/pipelines");
  });

  test("keeps a path with a query as it is", () => {
    expect(peerPath("raze-2", "/v1/logs")).toBe("/v1/peers/raze-2/logs");
  });
});

describe("the calls the UI makes", () => {
  test("the device's own calls stay on its API, a peer's go through the gateway", async () => {
    const seen: string[] = [];
    const original = globalThis.fetch;
    globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
      seen.push(`${init?.method ?? "GET"} ${String(input)}`);
      return new Response("[]", { status: 200, headers: { "content-type": "application/json" } });
    }) as typeof fetch;
    try {
      await api.cameras();
      await scoped("systemcore").cameras();
      await scoped("systemcore").setCameraSettings("camera.systemcore.cam0", { gain: 2 });
    } finally {
      globalThis.fetch = original;
    }
    expect(seen).toEqual(["GET /v1/cameras", "GET /v1/peers/systemcore/cameras", "PATCH /v1/peers/systemcore/cameras/camera.systemcore.cam0/settings"]);
  });
});

describe("a peer's node", () => {
  test("takes the peer's id, address and state, and the device's host facts", () => {
    const node = peerNode(peerOf("systemcore"), snapshot(deviceOf("systemcore", "systemcore")));
    expect(node.id).toBe("systemcore");
    expect(node.kind).toBe("systemcore");
    expect(node.address).toBe("172.27.0.1");
    expect(node.os).toEqual({ name: "LIMELIGHTOS_SYSTEMCORE", version: "2027.0.0" });
    expect(node.health).toBe("online");
    expect(node.peer).toEqual({ routed: true, state: "enrolled", needsEnrollment: false, keyFingerprint: "sha256:x" });
  });

  test("with no address, the node has an empty one", () => {
    const node = peerNode(peerOf("systemcore", { address: null }), snapshot(deviceOf("systemcore", "systemcore")));
    expect(node.address).toBe("");
  });

  test("says how the gateway found it, for the devices pane", () => {
    const node = peerNode(peerOf("raze-37bc", { found_by: "neighbour" }), snapshot(deviceOf("raze-37bc", "raze")));
    expect(node.peer?.foundBy).toBe("neighbour");
    expect(unreachableNode(peerOf("raze-37bc", { found_by: "orion" })).peer?.foundBy).toBe("orion");
  });
});

describe("a peer that does not answer", () => {
  const previous = peerNode(peerOf("systemcore"), snapshot(deviceOf("systemcore", "systemcore")));

  test("is offline, and keeps what it last said", () => {
    const node = unreachableNode(peerOf("systemcore", { address: "172.27.0.9" }), previous);
    expect(node.health).toBe("offline");
    expect(node.name).toBe(previous.name);
    expect(node.os).toEqual(previous.os);
    expect(node.address).toBe("172.27.0.9");
    expect(node.peer).toEqual({ routed: true, state: "enrolled", needsEnrollment: false, keyFingerprint: "sha256:x" });
  });

  test("never read, it is an empty offline node with its id", () => {
    const node: ClusterNode = unreachableNode(peerOf("pi-2"));
    expect(node.id).toBe("pi-2");
    expect(node.health).toBe("offline");
    expect(node.services).toEqual([]);
    expect(node.cpu).toBe(0);
  });
});

describe("which peers the UI reaches", () => {
  test("only the routed ones", () => {
    const peers = [peerOf("a"), peerOf("b", { routed: false, state: "discovered" })];
    expect(routedPeers(peers).map((p) => p.node_id)).toEqual(["a"]);
  });

  test("a listed peer that is not routed says so, and is not reachable", () => {
    const node = unreachableNode(peerOf("b", { routed: false, state: "discovered" }));
    expect(isReachable(node)).toBe(false);
    expect(placeOf(node)).toEqual({ text: "found, not enrolled", tone: "warn" });
  });

  test("a routed peer and this device are reachable and say where they are", () => {
    const peer = peerNode(peerOf("systemcore"), snapshot(deviceOf("systemcore", "systemcore")));
    expect(isReachable(peer)).toBe(true);
    expect(placeOf(peer)).toEqual({ text: "peer · routed", tone: "info" });
    const local = { ...peer, peer: undefined };
    expect(placeOf(local)).toEqual({ text: "this device", tone: "ok" });
  });
});

describe("the cluster's keys for a device's entities", () => {
  test("this device's own ids stay as they are", () => {
    expect(entityKey(undefined, "default")).toBe("default");
    expect(deviceId("raze-owner", "default")).toBe("default");
  });

  test("a peer's ids are keyed by its node, and the device's id is recovered from the key", () => {
    expect(entityKey("systemcore", "default")).toBe("systemcore/default");
    expect(deviceId("systemcore", "systemcore/default")).toBe("default");
  });

  test("a key with no node prefix is the device's own id", () => {
    expect(deviceId(undefined, "default")).toBe("default");
    expect(deviceId("systemcore", "default")).toBe("default");
  });
});

describe("two devices with a pipeline of the same id", () => {
  const pipeline = (id: string, camera: string) =>
    ({
      id,
      name: id,
      enabled: true,
      state: "running",
      revision: 1,
      bindings: { camera: { source: "camera", resource_id: camera } },
      outputs: [{ port: "pose" }],
      graph: { format: "daedalus.graph", schema_version: 1, requires: [], graph: { nodes: [], edges: [] } },
    }) as unknown as import("../src/lib/api/types").Pipeline;

  test("each keeps its own workload, under its own key, bound to its own camera", () => {
    const local = toWorkload(pipeline("default", "camera.raze.cam0"), "raze-owner");
    const peer = toWorkload(pipeline("default", "camera.sc.cam0"), "systemcore", undefined, (id) => entityKey("systemcore", id));
    expect(local.id).toBe("default");
    expect(local.bindings).toEqual({ camera: "camera.raze.cam0" });
    expect(peer.id).toBe("systemcore/default");
    expect(peer.nodeId).toBe("systemcore");
    expect(peer.bindings).toEqual({ camera: "systemcore/camera.sc.cam0" });
    expect(peer.outputs).toEqual(["output.systemcore/default.pose"]);
    expect(local.id).not.toBe(peer.id);
  });
});
