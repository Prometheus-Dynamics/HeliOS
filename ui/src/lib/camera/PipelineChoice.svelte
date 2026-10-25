<script lang="ts">
  // What one camera runs, picked from the default pipelines: AprilTag localization (with the
  // live 3D map and its depth on the same detector), ArUco markers, feature tracks or video only.
  // The choices become the one template that does them all, deployed as the camera's pipeline.
  import type { Camera } from "#lib/api/model.js";
  import { choiceOf, normalized, templateFor, type Choice, type TagKind } from "#lib/core/defaults.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { identity } from "#lib/core/identity.svelte.js";
  import { offeredTemplates } from "#lib/core/templates.svelte.js";
  import Seg from "#lib/kit/Seg.svelte";
  import Switch from "#lib/kit/Switch.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";

  let { camera }: { camera: Camera } = $props();

  $effect(() => {
    void offeredTemplates.load();
  });
  const offered = $derived(offeredTemplates.list);
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
    const applied = draft;
    busy = true;
    try {
      await cluster.applyDefault(camera.resourceId, template, identity.name(camera.resourceId, camera.name));
      // Only the choice that was applied is cleared: a choice made while it applied stays.
      if (draft === applied) draft = null;
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
    <!-- Not <label>s: a label forwards its click to the switch (a button), which would toggle it back. -->
    {#if choice.tags === "apriltag"}
      <div class="opt">
        <Switch checked label="Field position" disabled />
        <span class="txt"><b>Field position</b><i>Where the robot is on the field, from the tags. Always on with AprilTag.</i></span>
      </div>
      <div class="opt">
        <Switch checked={choice.map} label="3D map" onchange={(v) => set({ map: v })} />
        <button type="button" class="txt" onclick={() => set({ map: !choice.map })}><b>3D map</b><i>A live map of the surroundings and the camera's pose in it.</i></button>
      </div>
      <div class="opt">
        <Switch checked={choice.depth} label="Depth" onchange={(v) => set({ depth: v })} />
        <button type="button" class="txt" onclick={() => set({ depth: !choice.depth })}><b>Depth</b><i>A dense depth mesh of what the camera sees. Needs the 3D map.</i></button>
      </div>
    {:else if choice.tags === "aruco"}
      <p class="note">ArUco markers give each marker's pose. The 3D map and depth need AprilTags.</p>
    {:else}
      <div class="opt">
        <Switch checked={choice.features} label="Feature tracks" onchange={(v) => set({ features: v })} />
        <button type="button" class="txt" onclick={() => set({ features: !choice.features })}><b>Feature tracks</b><i>Image features followed from frame to frame. Off: video only.</i></button>
      </div>
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
        <span class="btns">
          <button type="button" class="ghost" onclick={() => (draft = null)}>Cancel</button>
          <button type="button" class="apply" disabled={busy || (!template && !wantedNothing(choice))} onclick={apply}>{busy ? "Applying…" : "Apply"}</button>
        </span>
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
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
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
  }
  .opt > :global(.sw) {
    margin-top: 2px;
  }
  .txt {
    display: block;
    flex: 1;
    min-width: 0;
    padding: 0;
    text-align: left;
    color: inherit;
  }
  button.txt {
    cursor: pointer;
  }
  .opt b,
  .opt i {
    display: block;
  }
  .opt b {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.3;
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
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in oklab, var(--line) 60%, transparent);
  }
  .runs {
    flex: 1;
    min-width: 140px;
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
  .btns {
    display: flex;
    gap: 6px;
    margin-left: auto;
  }
  button {
    height: 24px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 600;
    border-radius: var(--r-1);
    white-space: nowrap;
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
