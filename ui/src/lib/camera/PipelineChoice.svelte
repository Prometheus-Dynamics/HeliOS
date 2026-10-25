<script lang="ts">
  // What one camera runs, picked from the default pipelines: AprilTag localization (with the
  // live 3D map and its depth on the same detector), ArUco markers, feature tracks or video only.
  // The choices become the one template that does them all, deployed as the camera's pipeline.
  import type { Camera } from "#lib/api/model.js";
  import { choiceOf, normalized, templateFor, type Choice, type DefaultTemplate, type TagKind } from "#lib/core/defaults.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import Seg from "#lib/kit/Seg.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { drafts } from "#lib/graph/drafts.svelte.js";
  import { cluster } from "#lib/stores/cluster.svelte.js";

  let { camera, offered }: { camera: Camera; offered: DefaultTemplate[] } = $props();

  const running = $derived(guide.active(camera));
  const current = $derived(choiceOf(running?.graph, guide.dictionary(camera)));
  let draft = $state<Choice | null>(null);
  const choice = $derived(draft ?? current);
  const template = $derived(templateFor(choice, offered));
  const changed = $derived(draft !== null && JSON.stringify(normalized(draft)) !== JSON.stringify(current));
  let busy = $state(false);

  function set(change: Partial<Choice>) {
    const next = { ...choice, ...change };
    // Depth is the 3D map's: turning it on turns the map on, turning the map off turns it off.
    if (change.depth) next.map = true;
    if (change.map === false) next.depth = false;
    draft = normalized(next);
  }

  async function apply() {
    busy = true;
    try {
      await cluster.applyDefault(camera.resourceId, template, identity.name(camera.resourceId, camera.name));
      // The camera's pipeline has a new graph: its editor drafts start from it.
      for (const w of guide.workloadsFor(camera)) drafts.get(w.id).reload();
      draft = null;
    } finally {
      busy = false;
    }
  }
</script>

<div class="choice">
  {#if camera.foreign}
    <p class="muted">Run by {camera.foreign}; choose what it does there.</p>
  {:else}
    <div class="row">
      <span class="k">Tags</span>
      <Seg
        label="Tags"
        value={choice.tags ?? "none"}
        options={[
          { value: "apriltag", label: "AprilTag" },
          { value: "aruco", label: "ArUco" },
          { value: "none", label: "None" },
        ]}
        onchange={(v) => set({ tags: v === "none" ? null : (v as TagKind) })}
      />
    </div>
    {#if choice.tags === "apriltag"}
      <label class="opt">
        <Switch checked label="Field position" disabled />
        <span><b>Field position</b><i>Where the robot is on the field, from the tags. Always on with AprilTag.</i></span>
      </label>
      <label class="opt">
        <Switch checked={choice.map} label="3D map" onchange={(v) => set({ map: v })} />
        <span><b>3D map</b><i>A live map of the surroundings and the camera's pose in it.</i></span>
      </label>
      <label class="opt">
        <Switch checked={choice.depth} label="Depth" onchange={(v) => set({ depth: v })} />
        <span><b>Depth</b><i>A dense depth mesh of what the camera sees. Needs the 3D map.</i></span>
      </label>
    {:else if choice.tags === "aruco"}
      <p class="note">ArUco markers give each marker's pose. The 3D map and depth need AprilTags.</p>
    {:else}
      <label class="opt">
        <Switch checked={choice.features} label="Feature tracks" onchange={(v) => set({ features: v })} />
        <span><b>Feature tracks</b><i>Image features followed from frame to frame. Off: video only.</i></span>
      </label>
    {/if}
    <div class="foot">
      <span class="runs">
        {#if template}
          <b>{template.title}</b> · one pipeline, one core
        {:else if wantedNothing(choice)}
          <b>Video only</b> · no pipeline
        {:else}
          <span class="warn">This device has no pipeline for that combination.</span>
        {/if}
      </span>
      {#if changed}
        <button type="button" class="ghost" onclick={() => (draft = null)}>Cancel</button>
        <button type="button" class="apply" disabled={busy || (!template && !wantedNothing(choice))} onclick={apply}>{busy ? "Applying…" : "Apply"}</button>
      {/if}
    </div>
  {/if}
</div>

<script module lang="ts">
  import type { Choice as C } from "#lib/core/defaults.js";
  function wantedNothing(choice: C) {
    return !choice.tags && !choice.features;
  }
</script>

<style>
  .choice {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 10px 10px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .k {
    width: 44px;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .opt {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    cursor: pointer;
  }
  .opt span {
    display: flex;
    flex-direction: column;
  }
  .opt b {
    font-size: 12px;
    font-weight: 600;
  }
  .opt i {
    font-style: normal;
    font-size: 11px;
    color: var(--fg-3);
    line-height: 1.4;
  }
  .note,
  .muted {
    margin: 0;
    font-size: 11.5px;
    color: var(--fg-3);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in oklab, var(--line) 60%, transparent);
  }
  .runs {
    flex: 1;
    min-width: 0;
    font-size: 11.5px;
    color: var(--fg-2);
  }
  .runs b {
    color: var(--fg);
    font-weight: 600;
  }
  .warn {
    color: var(--warn);
  }
  button {
    height: 24px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 600;
    border-radius: var(--r-1);
  }
  .ghost {
    color: var(--fg-2);
    border: 1px solid var(--line-strong);
  }
  .apply {
    color: var(--on-accent);
    background: var(--accent);
    border: 1px solid var(--accent);
  }
  .apply:disabled {
    opacity: 0.6;
  }
</style>
