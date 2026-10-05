<script lang="ts">
  // The robot-centric 3D view: mounts, tags and trails are pushed into an
  // on-demand three.js scene. The scene owns its GPU resources and frees
  // them on unmount. Preset views are driven through `goTo`.
  import { onMount } from "svelte";
  import type * as THREE from "three";
  import type { Mount, PlacedTag } from "./mounts";
  import { FieldScene, type SceneOptions, type ViewPreset } from "./scene";

  let {
    mounts,
    tags,
    frame,
    selected = null,
    options,
    onselect,
    onusermove,
    model = null,
    modelOpts = null,
    picking = false,
    onpick,
  }: {
    mounts: Mount[];
    tags: PlacedTag[];
    frame: number;
    selected?: string | null;
    options: SceneOptions;
    onselect?: (cameraId: string) => void;
    /** The user started orbiting: the view is no longer a preset. */
    onusermove?: () => void;
    model?: THREE.Object3D | null;
    modelOpts?: { unit: number; up: "y" | "z"; offset: [number, number, number]; yaw: number } | null;
    picking?: boolean;
    onpick?: (hit: { point: THREE.Vector3; normal: THREE.Vector3; part: string }) => void;
  } = $props();

  let host: HTMLDivElement;
  let scene = $state<FieldScene | null>(null);
  let failed = $state(false);

  onMount(() => {
    try {
      scene = new FieldScene(
        host,
        (id) => onselect?.(id),
        () => onusermove?.(),
      );
    } catch {
      failed = true;
      return;
    }
    return () => {
      scene?.dispose();
      scene = null;
    };
  });

  $effect(() => scene?.setMounts(mounts));
  $effect(() => scene?.setSelected(selected));
  $effect(() => scene?.setOptions({ frustums: options.frustums, trails: options.trails, grid: options.grid, labels: options.labels }));
  $effect(() => scene?.setTags(tags, frame));
  $effect(() => scene?.setModel(model, modelOpts ? { ...modelOpts, offset: [...modelOpts.offset] as [number, number, number] } : null));
  $effect(() => scene?.setPicking(picking));
  $effect(() => {
    if (scene) scene.onPick = (h) => onpick?.(h);
  });

  /** Glides the camera to a preset view. */
  export function goTo(next: ViewPreset) {
    scene?.setView(next);
  }
</script>

<div class="scene" bind:this={host}>
  {#if failed}
    <div class="fallback hint">WebGL is not available in this browser, so the 3D view cannot be drawn.</div>
  {/if}
</div>

<style>
  .scene {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--bg);
    touch-action: none;
  }
  .fallback {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    text-align: center;
  }
  .scene :global(.f3-canvas) {
    display: block;
    outline: none;
  }
  .scene :global(.f3-labels) {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .scene :global(.f3-cam) {
    pointer-events: auto;
    cursor: pointer;
    padding: 2px 8px;
    border-radius: var(--r-pill);
    font-size: 11px;
    font-weight: 600;
    color: var(--fg);
    background: color-mix(in srgb, var(--s0) 82%, transparent);
    border: 1px solid color-mix(in srgb, var(--c) 45%, transparent);
    white-space: nowrap;
    transition:
      border-color var(--t-fast),
      background var(--t-fast);
  }
  .scene :global(.f3-cam:hover) {
    background: var(--layer);
  }
  .scene :global(.f3-cam.selected) {
    border-color: var(--accent);
    color: var(--accent-text-strong);
  }
  .scene :global(.f3-tag) {
    padding: 0 5px;
    border-radius: 5px;
    font-family: var(--font-code);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 16px;
    color: var(--rail);
    background: var(--c);
    white-space: nowrap;
  }
  .scene :global(.f3-tag.selected) {
    box-shadow: 0 0 0 1.5px var(--accent);
  }
  .scene :global(.f3-pick) {
    padding: 2px 7px;
    border-radius: var(--r-1);
    font-size: 11px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    white-space: nowrap;
  }
  .scene :global(.f3-ring) {
    font-size: 10px;
    color: var(--fg-faint);
    white-space: nowrap;
  }
</style>
