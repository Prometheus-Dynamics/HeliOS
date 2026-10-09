<script lang="ts">
  // One camera, big: follows the selected camera or stays pinned. Overlays,
  // ROI drawing, snapshot and quick exposure in the tab row.
  import { identity } from "$lib/core/identity.svelte";
  import { menu } from "$lib/core/menu.svelte";
  import { selection } from "$lib/core/selection.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import IconButton from "$lib/kit/IconButton.svelte";
  import Picker from "$lib/kit/Picker.svelte";
  import { cluster } from "$lib/stores/cluster.svelte";
  import { toasts } from "$lib/stores/toasts.svelte";
  import Feed from "$lib/vision/Feed.svelte";
  import { DEFAULT_OVERLAYS, overlayMenu, type Overlays } from "$lib/vision/overlays";
  import PaneBar from "$lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "$lib/workspace/panes";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();

  const target = follow(() => pane, () => ws, "camera", () => cluster.cameras[0]?.resourceId);
  const camera = $derived(target.id ? cluster.camera(target.id) : undefined);
  const overlays = $derived({ ...DEFAULT_OVERLAYS, ...((pane.props?.overlays as Partial<Overlays>) ?? {}) });
  let roiTool = $state(false);

  function setOverlays(patch: Partial<Overlays>) {
    workspaces.setProps(ws, pane.id, { overlays: { ...overlays, ...patch } });
  }

  function snapshot() {
    if (!camera) return;
    if (camera.feed.live) {
      // The preview frame on screen, as the device encoded it.
      const img = document.querySelector<HTMLImageElement>(`img[alt="${CSS.escape(camera.name)} camera"]`);
      if (!img?.naturalWidth) return toasts.info("The preview has no frame yet");
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      canvas.getContext("2d")?.drawImage(img, 0, 0);
      const a = document.createElement("a");
      try {
        a.href = canvas.toDataURL("image/jpeg", 0.92);
      } catch {
        return toasts.info("The preview comes from another origin and cannot be saved from here");
      }
      a.download = `${camera.name.toLowerCase().replace(/\W+/g, "-")}-${Date.now()}.jpg`;
      a.click();
      toasts.success(`Saved a preview frame from ${camera.name}`);
      return;
    }
    if (!camera.feed.base) {
      toasts.info("This camera has no preview");
      return;
    }
    const i = String(cluster.feedIndex(camera)).padStart(4, "0");
    const a = document.createElement("a");
    a.href = `${camera.feed.base}/${i}.jpg`;
    a.download = `${camera.name.toLowerCase().replace(/\W+/g, "-")}-${i}.jpg`;
    a.click();
    toasts.success(`Saved frame ${i} from ${camera.name}`);
  }
</script>

<PaneBar>
  <Picker
    label="Camera"
    icon="camera"
    value={camera?.resourceId}
    options={cluster.cameras.map((c) => ({ id: c.resourceId, name: identity.name(c.resourceId, c.name), hint: c.mount }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "camera", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
  <span class="sep"></span>
  <IconButton icon="marquee-2" label="Draw region of interest" active={roiTool} onclick={() => (roiTool = !roiTool)} size={24} />
  {#if camera?.settings.roi}
    <IconButton icon="x" label="Clear ROI" size={24} onclick={() => camera && cluster.setCamera(camera.resourceId, { roi: null })} />
  {/if}
  <IconButton icon="chart-histogram" label="Histogram" active={overlays.histogram} onclick={() => setOverlays({ histogram: !overlays.histogram })} size={24} />
  <IconButton icon="target" label="Overlays" size={24} onclick={(e) => menu.below(e.currentTarget as Element, overlayMenu(overlays, setOverlays), "end")} />
  <IconButton icon="photo-scan" label="Save this frame" size={24} onclick={snapshot} />
</PaneBar>

{#if camera}
  <div class="wrap">
    <Feed {camera} {overlays} {roiTool} onroi={(roi) => { cluster.setCamera(camera.resourceId, { roi }); roiTool = false; toasts.success(`ROI set to ${roi?.w}×${roi?.h}`); }} />
  </div>
{:else}
  <Empty icon="camera" text="No camera selected. Pick one in the tab bar." />
{/if}

<style>
  .wrap {
    flex: 1;
    display: flex;
    min-height: 0;
  }
</style>
