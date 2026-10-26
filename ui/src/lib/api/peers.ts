// The robot's peers: the other HeliOS devices on this robot, reached through the gateway
// (`GET /v1/peers`). A peer's devices, cameras, pipelines and logs are its own, read through
// `scoped(nodeId)` (client.ts); the store keeps each one with its node id. A peer that stops
// answering is offline with its last known state, not an error for the whole UI.

import { toNode, type DeviceSnapshot } from "./adapt";
import type { ClusterNode } from "./model";
import type { Peer } from "./types";

/** A peer's node as the UI shows it, from what the peer's device answered. */
export function peerNode(peer: Peer, snapshot: DeviceSnapshot, previous?: ClusterNode): ClusterNode {
  return { ...toNode(snapshot, previous), id: peer.node_id, address: peer.address ?? "", peer: peerState(peer) };
}

/** How the gateway reaches a peer, as the node carries it: routed, its trust state, and how it was found. */
function peerState(peer: Peer): NonNullable<ClusterNode["peer"]> {
  return { routed: peer.routed, state: peer.state, needsEnrollment: peer.needs_enrollment, keyFingerprint: peer.key_fingerprint, foundBy: peer.found_by };
}

/**
 * A peer whose API did not answer: offline, with its last known state. A peer never read has no
 * state yet, so its node is empty apart from its id, its address and that it is offline.
 */
export function unreachableNode(peer: Peer, previous?: ClusterNode): ClusterNode {
  const state = peerState(peer);
  if (previous) return { ...previous, health: "offline", address: peer.address ?? previous.address, peer: state };
  return {
    id: peer.node_id,
    name: peer.node_id,
    kind: "peer",
    runsPipelines: true,
    model: "HeliOS device",
    role: "HeliOS",
    health: "offline",
    link: "ethernet",
    address: peer.address ?? "",
    os: { name: "HeliOS", version: "" },
    uptimeS: 0,
    cpu: 0,
    cpuHistory: [],
    memMiB: 0,
    memTotalMiB: 0,
    tempC: 0,
    tempHistory: [],
    diskUsedMiB: 0,
    diskTotalMiB: 0,
    clock: { source: "local", offsetUs: 0 },
    agent: { version: "", reachable: false },
    recovery: [],
    slots: [],
    services: [],
    peer: state,
  };
}

/** The peers the gateway can reach: the only ones the UI sends requests to. */
export function routedPeers(peers: Peer[]): Peer[] {
  return peers.filter((peer) => peer.routed);
}

/**
 * The local list, then each peer's, in order. A peer's item is left out (and its key named in
 * `dropped`) when its key is already taken: a device's ids are its own, so two devices sharing an
 * id would otherwise show one another's state. The items keep their node ids.
 */
export function mergeLists<T>(local: T[], peers: T[][], key: (item: T) => string): { items: T[]; dropped: string[] } {
  const seen = new Set(local.map(key));
  const items = [...local];
  const dropped: string[] = [];
  for (const list of peers) {
    for (const item of list) {
      const id = key(item);
      if (seen.has(id)) {
        dropped.push(id);
        continue;
      }
      seen.add(id);
      items.push(item);
    }
  }
  return { items, dropped };
}
