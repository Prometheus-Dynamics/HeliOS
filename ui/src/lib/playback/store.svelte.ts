// The Playback store: the open players, polled while a Playback pane is up,
// and the actions the pane takes (open, close, transport). Transport goes
// through each player's camera settings, one request at a time, so quick key
// presses cannot reorder a seek behind the play that followed it.

import { api, errorText } from "#lib/api/client.js";
import { MOCK } from "#lib/api/mode.js";
import type { PlaybackCreate, PlayerView } from "#lib/api/types.js";
import { cluster } from "#lib/stores/cluster.svelte.js";
import { toasts } from "#lib/stores/toasts.svelte.js";
import type { ReplayChange } from "./transport.js";

/** Polling while a player is open (its status moves), and while none is. */
const POLL_MS = 500;
const IDLE_POLL_MS = 2000;
/** A player's camera joins the camera list a moment after it opens: ask again at most this often. */
const ASK_CAMERAS_MS = 1500;

class PlaybackStore {
  /** True in the simulated robot (`?mock=1`), which has no players. */
  readonly mock = MOCK;
  players = $state<PlayerView[]>([]);
  /** Why the players could not be read now (null when they could). */
  error = $state<string | null>(null);
  /** The player list has been read at least once. */
  loaded = $state(false);
  #users = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #queue: Promise<void> = Promise.resolve();
  #asked = new Map<string, number>();

  /** A pane is up: poll while any is (one loop for all of them). Returns the release for the pane's teardown. */
  acquire(): () => void {
    if (this.#users++ === 0) void this.#poll();
    return () => {
      if (--this.#users === 0) clearTimeout(this.#timer);
    };
  }

  async #poll() {
    if (this.#users === 0) return;
    if (typeof document === "undefined" || !document.hidden) await this.refresh();
    if (this.#users === 0) return;
    this.#timer = setTimeout(() => void this.#poll(), this.players.length ? POLL_MS : IDLE_POLL_MS);
  }

  /** Reads the open players and their replay state. */
  async refresh() {
    if (MOCK) return;
    try {
      this.players = await api.playbackPlayers();
      this.error = null;
    } catch (error) {
      this.error = errorText(error);
    }
    this.loaded = true;
    this.#askForCameras();
  }

  /** A player's frames are a camera: until the camera list has it, ask for the list (at most every so often). */
  #askForCameras() {
    const now = Date.now();
    for (const player of this.players) {
      if (cluster.camera(player.resource_id)) continue;
      if (now - (this.#asked.get(player.resource_id) ?? 0) < ASK_CAMERAS_MS) continue;
      this.#asked.set(player.resource_id, now);
      void cluster.live?.refreshCameras();
    }
  }

  /** Opens a player (throws the API's error, for the form to show). */
  async open(body: PlaybackCreate): Promise<PlayerView> {
    const view = await api.openPlayback(body);
    this.players = [...this.players.filter((p) => p.id !== view.id), view];
    await cluster.live?.refreshCameras();
    void this.refresh();
    return view;
  }

  /** Closes a player; its pipeline runs are removed with it. */
  async close(id: string) {
    try {
      await api.closePlayback(id);
      this.players = this.players.filter((p) => p.id !== id);
      void this.refresh();
    } catch (error) {
      toasts.error(`Close playback: ${errorText(error)}`);
    }
  }

  /** Sends replay changes to a player, one after another, then reads its state again. */
  apply(player: PlayerView, changes: ReplayChange[]): Promise<void> {
    this.#queue = this.#queue.then(async () => {
      for (const change of changes) {
        try {
          await api.setCameraSettings(player.resource_id, change);
        } catch (error) {
          toasts.error(`Playback: ${errorText(error)}`);
          break;
        }
      }
      await this.refresh();
    });
    return this.#queue;
  }
}

export const playback = new PlaybackStore();
