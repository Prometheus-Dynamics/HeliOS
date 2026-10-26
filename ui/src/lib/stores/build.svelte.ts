// Which HeliOS build this page is talking to. Each event stream opens with a `hello` that names the
// API's build. The first one sets it; a later one (after a reconnect, which an update forces) that
// names another build means the device was updated under this page, so its UI is out of date.

class BuildStore {
  /** The build the page started against (null until the first `hello`). */
  id = $state<string | null>(null);
  /** The device now runs another build than the page started with: offer a reload. */
  updated = $state(false);

  note(build: string | undefined) {
    if (!build) return;
    if (this.id === null) this.id = build;
    else if (build !== this.id) this.updated = true;
  }
}

export const build = new BuildStore();
