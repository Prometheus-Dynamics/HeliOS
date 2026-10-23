// Every pipeline output of every tick (`GET /v1/live/ws`: newline-delimited JSON, one line per
// output, `{pipeline, port, observed_at_ms, frame_ts_ns, value}`), kept without reactivity:
//
// - the latest value of each output, handed to `onlatest` at most once per animation frame (a
//   120 fps or 1 kHz pipeline costs one UI update per screen frame, not one per message);
// - a short history per output keyed by the frame's capture time, so a camera view can draw each
//   preview frame with the outputs of that very frame (`at`).
//
// Any client takes what it needs from the same stream; nothing here is specific to drawing.

import type * as W from "#lib/api/types.js";

/** One output of one tick. */
export interface LiveOutput {
  id: string;
  frameTs: number | null;
  value: W.PipelineOutput;
}

/** Ticks kept per output for frame matching (over 0.5 s at 120 fps). */
const HISTORY = 64;
const RECONNECT_MS = 1000;

export class LiveOutputs {
  private socket: WebSocket | null = null;
  private history = new Map<string, LiveOutput[]>();
  private pending = new Map<string, LiveOutput>();
  private frame = 0;
  private stopped = false;
  private retry = 0;
  /** Whether the stream is connected (the event stream's slower copies are then ignored). */
  connected = false;
  /** Lines received, for the profiler and tests. */
  received = 0;

  constructor(
    private url: string,
    private onlatest: (outputs: LiveOutput[]) => void,
  ) {}

  start(): () => void {
    this.stopped = false;
    this.connect();
    return () => {
      this.stopped = true;
      clearTimeout(this.retry);
      if (this.frame) cancelAnimationFrame(this.frame);
      this.socket?.close();
      this.socket = null;
      this.connected = false;
    };
  }

  private connect() {
    if (this.stopped || typeof WebSocket === "undefined") return;
    const socket = new WebSocket(this.url);
    this.socket = socket;
    socket.onopen = () => (this.connected = true);
    socket.onmessage = (event) => {
      if (typeof event.data === "string") this.ingest(event.data);
    };
    socket.onclose = () => {
      this.connected = false;
      if (this.socket === socket && !this.stopped) this.retry = window.setTimeout(() => this.connect(), RECONNECT_MS);
    };
  }

  /** A text message: lines of outputs. Public for tests. */
  ingest(text: string) {
    for (const line of text.split("\n")) {
      if (!line) continue;
      let parsed: { pipeline?: string; port?: string; frame_ts_ns?: number | null; observed_at_ms?: number; value?: unknown };
      try {
        parsed = JSON.parse(line);
      } catch {
        continue;
      }
      if (typeof parsed.pipeline !== "string" || typeof parsed.port !== "string") continue;
      this.received += 1;
      const output: LiveOutput = {
        id: `${parsed.pipeline}/${parsed.port}`,
        frameTs: typeof parsed.frame_ts_ns === "number" ? parsed.frame_ts_ns : null,
        value: { pipeline: parsed.pipeline, port: parsed.port, value: parsed.value, observed_at_ms: parsed.observed_at_ms ?? 0 } as W.PipelineOutput,
      };
      let list = this.history.get(output.id);
      if (!list) this.history.set(output.id, (list = []));
      list.push(output);
      if (list.length > HISTORY) list.splice(0, list.length - HISTORY);
      this.pending.set(output.id, output);
    }
    if (this.pending.size && !this.frame && typeof requestAnimationFrame !== "undefined") this.frame = requestAnimationFrame(() => this.flush());
  }

  /** Hand the latest of each output over (once per animation frame). Public for tests. */
  flush() {
    this.frame = 0;
    if (!this.pending.size) return;
    const outputs = [...this.pending.values()];
    this.pending.clear();
    this.onlatest(outputs);
  }

  /**
   * The output `id` at the frame captured at `frameTs`: the tick of that frame, else the newest
   * tick not after it (a preview frame the pipeline skipped), else the newest. Null without any.
   */
  at(id: string, frameTs: number): LiveOutput | null {
    const list = this.history.get(id);
    if (!list?.length) return null;
    let best: LiveOutput | null = null;
    for (const output of list) {
      if (output.frameTs === null) continue;
      if (output.frameTs === frameTs) return output;
      if (output.frameTs < frameTs && (!best || output.frameTs! > best.frameTs!)) best = output;
    }
    return best ?? list[list.length - 1];
  }
}
