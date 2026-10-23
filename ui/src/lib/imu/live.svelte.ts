// The node's live stream, shared by every view that shows it (the IMU pane, the Hardware screen).
// One socket while any view watches (`watch`), closed with the last one. The rows are kept
// outside Svelte's reactivity (drawn from directly each frame); `version` moves per batch so
// views that read it update.

import { MOCK } from "#lib/api/mode.ts";
import { openSimulatedStream } from "#lib/api/mock-stream.ts";
import { openLiveStream, type StreamHandlers } from "#lib/api/streams.ts";
import { decodeSamples, parseTextFrame, type StreamStatus } from "./frame.ts";
import { LiveFeed } from "./feed.ts";

class LiveStreams {
  readonly feed = new LiveFeed();
  /** The follower's state on the device (or the simulated robot's). */
  status = $state<StreamStatus>({ state: "connecting", reason: "" });
  /** Moves with every batch. */
  version = $state(0);
  private watchers = 0;
  private stop: (() => void) | null = null;

  /** Watches the streams while the caller is shown. Returns the stop, which the caller must call. */
  watch(): () => void {
    if (typeof window === "undefined") return () => {};
    this.watchers += 1;
    if (this.watchers === 1) this.stop = (MOCK ? openSimulatedStream : openLiveStream)(this.handlers());
    let done = false;
    return () => {
      if (done) return;
      done = true;
      this.watchers -= 1;
      if (this.watchers > 0) return;
      this.stop?.();
      this.stop = null;
      this.status = { state: "connecting", reason: "" };
    };
  }

  private handlers(): StreamHandlers {
    return {
      text: (text) => {
        const frame = parseTextFrame(text);
        if (!frame) return;
        if (frame.type === "catalog") this.feed.setCatalog(frame.catalog);
        else if (frame.type === "status") this.status = frame.status;
      },
      binary: (buffer) => {
        const batch = decodeSamples(buffer);
        if (!batch) return;
        this.feed.apply(batch);
        this.version += 1;
      },
      closed: () => {
        this.status = { state: "unavailable", reason: "the connection to helios-api dropped; retrying" };
      },
    };
  }
}

export const liveStreams = new LiveStreams();
