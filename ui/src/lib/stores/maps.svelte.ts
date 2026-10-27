// Saved maps: the maps each device keeps (`GET /v1/maps`), which a pipeline starts from. Read per
// device (a peer's through the gateway) and read again when a device announces a change to them
// (the `settings` event, domain `maps`), so every open tab follows. A save or a delete that fails
// keeps its message here: the control that made it shows the reason (a refused delete says which
// pipelines start from the map), rather than a toast.

import { errorText, mapDownloadUrl, type HeliosApi } from "#lib/api/client.js";
import type { MapEntry, MapList } from "#lib/api/types.js";
import { onSettingsChange } from "#lib/core/sync.js";

/** A device that keeps maps: its node id, whether it is a peer (reached through the gateway), and its API. */
export interface MapHost {
  id: string;
  remote: boolean;
  scope: HeliosApi;
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  return Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));
}

class MapsStore {
  /** The maps each device keeps, by its node id (absent until read). */
  lists = $state<Record<string, MapList>>({});
  /** Why a device's maps could not be read, by its node id. */
  readErrors = $state<Record<string, string>>({});
  /** The actions in progress: `save:<pipeline key>` and `delete:<map id>`. */
  busy = $state<Record<string, boolean>>({});
  /** Why the last action on a key failed, shown beside the control that made it. */
  failures = $state<Record<string, string>>({});

  /** The devices read so far, so a change announced for any of them is read again. */
  private hosts = new Map<string, MapHost>();

  constructor() {
    onSettingsChange("maps", () => {
      for (const host of this.hosts.values()) void this.refresh(host);
    });
  }

  /** Reads the maps a device keeps. */
  async refresh(host: MapHost): Promise<void> {
    this.hosts.set(host.id, host);
    try {
      const list = await host.scope.maps();
      this.lists = { ...this.lists, [host.id]: list };
      this.readErrors = without(this.readErrors, host.id);
    } catch (error) {
      this.readErrors = { ...this.readErrors, [host.id]: errorText(error) };
    }
  }

  /**
   * Saves a pipeline's live map now, as `name` (default: its id). `key` is the pipeline's key in the
   * cluster, `pipeline` its id on its device. The device waits for the mapper's map: this takes seconds.
   */
  async save(host: MapHost, key: string, pipeline: string, name?: string): Promise<MapEntry | undefined> {
    return this.act(`save:${key}`, () => host.scope.saveMap(pipeline, name), () => this.refresh(host));
  }

  /** Deletes a stored map. A refusal (a pipeline starts from it) is kept as the map's failure. */
  async remove(host: MapHost, id: string): Promise<boolean> {
    const done = await this.act(`delete:${id}`, () => host.scope.deleteMap(id).then(() => true), () => this.refresh(host));
    return done === true;
  }

  /** The file of a stored map, for a link (a peer's through the gateway). */
  downloadUrl(host: MapHost, id: string): string {
    return mapDownloadUrl(id, host.remote ? host.id : undefined);
  }

  private async act<T>(key: string, run: () => Promise<T>, after: () => Promise<void>): Promise<T | undefined> {
    this.busy = { ...this.busy, [key]: true };
    this.failures = without(this.failures, key);
    try {
      const value = await run();
      await after();
      return value;
    } catch (error) {
      this.failures = { ...this.failures, [key]: errorText(error) };
      return undefined;
    } finally {
      this.busy = without(this.busy, key);
    }
  }
}

export const maps = new MapsStore();
