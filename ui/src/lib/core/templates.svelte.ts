// The default pipelines the device offers a camera, read once and shared by every place that
// shows the choice (the inspector, What cameras run, setup). The UI's built-in list stands in
// until the device answers; a device that could not be reached is asked again next time.

import { cluster } from "#lib/stores/cluster.svelte.js";
import { BUILT_IN, type DefaultTemplate } from "./defaults";

class OfferedTemplates {
  list = $state<DefaultTemplate[]>(BUILT_IN);
  #loading: Promise<void> | null = null;

  /** Read the device's templates (once they are known, this is free); resolves when `list` is current. */
  load(): Promise<void> {
    if (cluster.live && cluster.live.templatesKnown) return Promise.resolve();
    this.#loading ??= cluster
      .defaultTemplates()
      .then((list) => {
        this.list = list;
      })
      .finally(() => (this.#loading = null));
    return this.#loading;
  }
}

export const offeredTemplates = new OfferedTemplates();
