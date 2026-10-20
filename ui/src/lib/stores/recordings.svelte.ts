// Recordings: the device's running recording, the stored parts, the capture-last
// rings and the recorder settings. Live from helios-api: the `recording` events
// keep the running one current (the listing is read when a part finishes, and as
// a fallback when the events go quiet). With `?mock=1` it runs the simulated
// recorder from mock-recorder.ts, which reports through the same path.

import { errorText } from "#lib/api/client.js";
import { MOCK } from "#lib/api/mode.js";
import { createMockRecorder } from "#lib/api/mock-recorder.js";
import { liveRecorder, type RecorderBackend } from "#lib/api/recorder.js";
import type { EidosExport, PartSummary, RecorderSettings, RecordingListing, RecordingStatus, RingStatus, StoredRecording } from "#lib/api/recording-types.js";
import { isActive } from "#lib/recording/format.js";
import { sortStored } from "#lib/recording/mapping.js";
import { toasts } from "./toasts.svelte";

/** While a recording runs and no event has come for this long, the listing is read instead. */
const STALE_MS = 5000;
const FALLBACK_POLL_MS = 2000;

class RecordingsStore {
  readonly mock = MOCK;
  listing = $state<RecordingListing | null>(null);
  listingError = $state<string | null>(null);
  settings = $state<RecorderSettings | null>(null);
  settingsError = $state<string | null>(null);
  /** The clock the elapsed times are drawn with; ticks once a second while a recording runs. */
  now = $state(Date.now());
  /** When the running recording's last status arrived here (its duration counts on from then). */
  receivedAt = $state(Date.now());
  /** The action in progress (`start`, `stop:<id>`, `delete:<id>`, `capture`, `export:<id>`, `save`), for its button. */
  busy = $state<string | null>(null);
  /** The last clip capture-last wrote, and each export's result, by recording id. */
  lastClip = $state<PartSummary | null>(null);
  exports = $state<Record<string, EidosExport>>({});

  private backend: RecorderBackend;
  private lastEventAt = 0;
  private clock: ReturnType<typeof setInterval> | undefined;

  constructor() {
    this.backend = MOCK ? createMockRecorder((status) => this.applyStatus(status)) : liveRecorder;
    setInterval(() => this.poll(), FALLBACK_POLL_MS);
    if (MOCK) void this.refresh();
  }

  /** The recording running on the device, if any (the recorder runs one at a time). */
  get active(): RecordingStatus | null {
    return this.listing?.active ?? null;
  }

  /** Stored parts, newest first. */
  get stored(): StoredRecording[] {
    return sortStored(this.listing?.stored ?? []);
  }

  /** The running recording, when it is this camera's. */
  activeFor(camera: string): RecordingStatus | null {
    const running = this.active;
    return running && running.camera === camera ? running : null;
  }

  /** This camera's capture-last ring, when it has one. */
  ringFor(camera: string): RingStatus | undefined {
    return this.listing?.rings.find((r) => r.camera === camera);
  }

  downloadUrl(id: string): string | null {
    return this.backend.downloadUrl(id);
  }

  /** Read the listing (the running recording, stored parts, storage and rings). */
  async refresh(): Promise<void> {
    try {
      this.listing = await this.backend.listing();
      this.listingError = null;
    } catch (error) {
      this.listingError = errorText(error);
    }
    this.syncClock();
  }

  /** A status from the event stream (or an action's answer): keeps the running recording current. */
  applyStatus(status: RecordingStatus): void {
    const now = Date.now();
    this.lastEventAt = now;
    this.receivedAt = now;
    this.now = now;
    if (isActive(status.state)) {
      this.listing = { ...(this.listing ?? EMPTY), active: status };
      this.syncClock();
      return;
    }
    const wasRunning = this.listing?.active?.id === status.id;
    if (this.listing && wasRunning) this.listing = { ...this.listing, active: null };
    this.syncClock();
    // A finished part is listed now (its summary is written when it closes).
    if (wasRunning || !this.listing) void this.refresh();
  }

  /** Read the listing while a recording runs and the events have gone quiet. */
  poll(): void {
    if (this.active && Date.now() - this.lastEventAt > STALE_MS) void this.refresh();
  }

  async start(camera: string): Promise<RecordingStatus | undefined> {
    return this.run("start", "Start recording", () => this.backend.start(camera), () => "Recording started", (status) => this.applyStatus(status));
  }

  async stop(id: string): Promise<void> {
    await this.run(`stop:${id}`, "Stop recording", () => this.backend.stop(id), () => "Finishing the last part", (status) => this.applyStatus(status));
  }

  async remove(id: string): Promise<void> {
    await this.run(`delete:${id}`, "Delete", () => this.backend.remove(id), () => "Recording deleted");
    await this.refresh();
  }

  /** Writes the ring's last `seconds` (default: the configured length) as a new stored recording. */
  async captureLast(camera?: string, seconds?: number): Promise<PartSummary | undefined> {
    const clip = await this.run("capture", "Capture last", () => this.backend.captureLast({ camera, seconds }), (part) => `Wrote ${part.file}`);
    if (clip) this.lastClip = clip;
    await this.refresh();
    return clip;
  }

  async exportEidos(id: string): Promise<EidosExport | undefined> {
    const done = await this.run(`export:${id}`, "Export for Eidos", () => this.backend.exportEidos(id), (e) => `Exported ${e.frames} frames for Eidos`);
    if (done) this.exports = { ...this.exports, [id]: done };
    return done;
  }

  async loadSettings(): Promise<void> {
    try {
      this.settings = await this.backend.settings();
      this.settingsError = null;
    } catch (error) {
      this.settingsError = errorText(error);
    }
  }

  /** Settings the device announced (`recording_settings`). */
  applySettings(settings: RecorderSettings): void {
    this.settings = settings;
  }

  /** Saves the whole settings object. A refusal stays with the form (`settingsError`), it is not a toast. */
  async saveSettings(next: RecorderSettings): Promise<boolean> {
    this.busy = "save";
    try {
      this.settings = await this.backend.saveSettings(next);
      this.settingsError = null;
      return true;
    } catch (error) {
      this.settingsError = errorText(error);
      return false;
    } finally {
      if (this.busy === "save") this.busy = null;
    }
  }

  /** Runs one action: a failure is a toast; `after` gets the answer. */
  private async run<T>(key: string, what: string, action: () => Promise<T>, success: (value: T) => string, after?: (value: T) => void): Promise<T | undefined> {
    this.busy = key;
    try {
      const value = await action();
      toasts.success(success(value));
      after?.(value);
      return value;
    } catch (error) {
      toasts.error(`${what}: ${errorText(error)}`);
      return undefined;
    } finally {
      if (this.busy === key) this.busy = null;
    }
  }

  /** The elapsed time ticks only while something runs. */
  private syncClock(): void {
    const running = this.active !== null;
    if (running && this.clock === undefined) {
      this.clock = setInterval(() => (this.now = Date.now()), 1000);
    } else if (!running && this.clock !== undefined) {
      clearInterval(this.clock);
      this.clock = undefined;
    }
  }
}

const EMPTY: RecordingListing = { active: null, stored: [], storage: null, storage_error: null, rings: [] };

export const recordings = new RecordingsStore();
