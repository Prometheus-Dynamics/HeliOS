// The field: its size and where every tag is. The layout is the device's selected field layout
// (`/v1/field-layouts`, the one the pipelines without their own use), so every tab and every
// pipeline see the same field. Edits are saved back to the device as WPILib's AprilTag field JSON,
// which is also the file format the field imports and exports, so robot code and HeliOS share one file.

import { api, errorText } from "#lib/api/client.js";
import { onSettingsChange } from "#lib/core/sync.js";
import { type FieldLayout, type WpilibFieldLayout, anglesOf, demoLayout, layoutFromDocument, layoutToWpilib } from "./field-layout";
import { toasts } from "./toasts.svelte";

export type { FieldLayout, FieldTag } from "./field-layout";

/** Edits are saved once they pause this long (a drag is one save, not one per pointer move). */
const SAVE_DELAY_MS = 400;
/** The id an edited layout is saved under when the selected one is built in (read only). */
const EDITED_ID = "field-edited";

class FieldStore {
  layout = $state<FieldLayout>(demoLayout());
  /** The device's selected layout (`null` until the device answers). */
  selectedId = $state<string | null>(null);
  /** Where the robot is, for previews: metres, degrees. */
  robotPose = $state({ x: 3.2, y: 2.4, heading: 20 });
  #builtin = false;
  #edits = 0;
  #saved = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #chain: Promise<void> = Promise.resolve();
  #started = false;

  /** Read the device's field and follow its changes (once). */
  start() {
    if (this.#started) return;
    this.#started = true;
    onSettingsChange("field_layout", () => void this.load());
    void this.load();
  }

  /** The selected layout from the device; not while edits are waiting to be saved. */
  async load() {
    if (this.#edits !== this.#saved) return;
    try {
      const list = await api.fieldLayouts();
      const doc = await api.fieldLayout(list.selected);
      this.layout = layoutFromDocument(doc);
      this.selectedId = list.selected;
      this.#builtin = list.layouts.find((layout) => layout.id === list.selected)?.builtin ?? false;
    } catch {
      // The device is not answering: the layout on screen stays until it does.
    }
  }

  update(id: number, patch: Partial<FieldLayout["tags"][number]>) {
    const t = this.layout.tags.find((x) => x.id === id);
    if (!t) return;
    Object.assign(t, patch);
    this.persist();
  }

  add(x: number, y: number): number {
    const id = Math.max(0, ...this.layout.tags.map((t) => t.id)) + 1;
    this.layout.tags.push({ id, x: +x.toFixed(3), y: +y.toFixed(3), z: 0.5, yaw: 0, pitch: 0 });
    this.persist();
    return id;
  }

  remove(id: number) {
    this.layout.tags = this.layout.tags.filter((t) => t.id !== id);
    this.persist();
  }

  resetDemo() {
    this.layout = demoLayout();
    this.persist();
  }

  /** Save the layout to the device, shortly after the last edit. */
  persist() {
    this.#edits++;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      this.#chain = this.#chain.then(() => this.#save());
    }, SAVE_DELAY_MS);
  }

  async #save() {
    const edits = this.#edits;
    const current = $state.snapshot(this.layout);
    const id = this.selectedId && !this.#builtin ? this.selectedId : EDITED_ID;
    try {
      await api.uploadFieldLayout(id, current.name || "Field", JSON.stringify(layoutToWpilib(current)), current.tagSize);
      if (id !== this.selectedId) {
        await api.selectFieldLayout(id);
        this.selectedId = id;
        this.#builtin = false;
      }
      this.#saved = Math.max(this.#saved, edits);
    } catch (error) {
      toasts.error(`Field layout not saved: ${errorText(error)}`);
    }
  }

  toWpilib(): WpilibFieldLayout {
    return layoutToWpilib(this.layout);
  }

  fromWpilib(doc: unknown, name: string): boolean {
    const d = doc as Partial<WpilibFieldLayout>;
    if (!Array.isArray(d?.tags) || !d.field) {
      toasts.error("That file is not a WPILib AprilTag field layout");
      return false;
    }
    const tags = d.tags.map((t) => ({
      id: t.ID,
      x: t.pose.translation.x,
      y: t.pose.translation.y,
      z: t.pose.translation.z,
      ...anglesOf(t.pose.rotation.quaternion),
    }));
    this.layout = { ...this.layout, name, length: d.field.length, width: d.field.width, tags };
    this.persist();
    toasts.success(`Loaded ${tags.length} tags from ${name}`);
    return true;
  }
}

export const field = new FieldStore();
