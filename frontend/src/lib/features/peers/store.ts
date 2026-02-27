import { derived, get, writable } from 'svelte/store';
import { discoverPeers, fetchPeerInventory, removePeer } from '$lib/api/peers';
import type { PeerDiscoveryInfo, PeerInventoryPayload, PeerSummary } from '$lib/types/peer';
import { clonePayload } from './utils';

export type PeersPending = {
  refresh: boolean;
  discover: boolean;
  removing: string[];
};

export type PeersStore = ReturnType<typeof createPeersStore>;

export function createPeersStore(initial: PeerInventoryPayload) {
  const payload = writable<PeerInventoryPayload>(clonePayload(initial));
  const pending = writable<PeersPending>({ refresh: false, discover: false, removing: [] });
  const hasLoadedOnce = writable(false);

  const peers = derived(payload, ($payload) => $payload.peers ?? []);
  const discovery = derived(payload, ($payload) => $payload.discovery ?? null);
  const fetchedAt = derived(payload, ($payload) => $payload.fetchedAt ?? Date.now());

  async function refresh(options: { bootstrap?: boolean } = {}): Promise<PeerInventoryPayload> {
    const { bootstrap = false } = options;
    const snapshot = get(pending);
    if (snapshot.refresh) return get(payload);
    pending.set({ ...snapshot, refresh: true });
    try {
      const next = await fetchPeerInventory();
      payload.set(clonePayload(next));
      return next;
    } finally {
      pending.update((current) => ({ ...current, refresh: false }));
      if (bootstrap || !get(hasLoadedOnce)) {
        hasLoadedOnce.set(true);
      }
    }
  }

  async function discover(options: { scopes?: Array<'mdns' | 'broadcast'>; timeoutSecs?: number } = {}): Promise<PeerDiscoveryInfo> {
    const snapshot = get(pending);
    if (snapshot.discover) throw new Error('Discovery already running');
    pending.set({ ...snapshot, discover: true });
    try {
      const info = await discoverPeers({ scopes: options.scopes ?? ['mdns', 'broadcast'], timeoutSecs: options.timeoutSecs ?? 5 });
      try {
        const next = await fetchPeerInventory();
        payload.set(clonePayload(next));
      } catch {
        payload.update((current) => ({ ...current, discovery: info, fetchedAt: Date.now() }));
      }
      return info;
    } finally {
      pending.update((current) => ({ ...current, discover: false }));
    }
  }

  async function removePeerById(peerId: string): Promise<boolean> {
    if (!peerId) return false;
    const snapshot = get(pending);
    if (snapshot.removing.includes(peerId)) return false;
    pending.set({ ...snapshot, removing: [...snapshot.removing, peerId] });
    try {
      const removed = await removePeer(peerId);
      if (removed) {
        payload.update((current) => ({
          ...current,
          peers: (current.peers ?? []).filter((peer) => peer.id !== peerId)
        }));
      }
      return removed;
    } finally {
      pending.update((current) => ({
        ...current,
        removing: current.removing.filter((id) => id !== peerId)
      }));
    }
  }

  function upsertPeer(peer: PeerSummary): void {
    payload.update((current) => {
      const entries = current.peers ?? [];
      const idx = entries.findIndex((entry) => entry.id === peer.id);
      if (idx === -1) return { ...current, peers: [peer, ...entries] };
      const next = [...entries];
      next[idx] = peer;
      return { ...current, peers: next };
    });
  }

  function removePeerLocal(peerId: string): void {
    payload.update((current) => ({
      ...current,
      peers: (current.peers ?? []).filter((peer) => peer.id !== peerId)
    }));
  }

  function setPayload(next: PeerInventoryPayload): void {
    payload.set(clonePayload(next));
  }

  return {
    payload,
    pending,
    hasLoadedOnce,
    peers,
    discovery,
    fetchedAt,
    refresh,
    discover,
    removePeerById,
    upsertPeer,
    removePeerLocal,
    setPayload
  };
}
