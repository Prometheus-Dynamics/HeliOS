<script lang="ts">
  // Set up › Cameras: every camera, with its name, icon and colour, and what it runs, picked from the
  // default pipelines. Tuning a pipeline afterwards is the pipeline's own settings (Tune).
  import Icon from "#lib/components/common/Icon.svelte";
  import StatusChain from "#lib/components/status/StatusChain.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import PipelineChoice from "#lib/camera/PipelineChoice.svelte";
  import IdentityEditor from "#lib/kit/IdentityEditor.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";

  let {}: PaneProps = $props();
</script>

<div class="scroll">
  {#each cluster.cameras as camera (camera.resourceId)}
    {@const running = guide.active(camera)}
    {@const sel = selection.last.camera?.id === camera.resourceId}
    <section class:sel style:--c={colorVar(identity.get(camera.resourceId).color)} data-camera={camera.resourceId}>
      <header>
        <IdentityEditor id={camera.resourceId} fallbackName={camera.name} fallbackIcon="camera" sub="{camera.sensor} · {cluster.node(camera.nodeId)?.name ?? camera.nodeId}{camera.mount ? ` · ${camera.mount}` : ''}" />
        {#if running}
          <button type="button" class="link" onclick={() => { selection.select({ kind: "workload", id: running.id }); shell.open("tune", "pipeline-settings"); }}>
            Tune pipeline <Icon name="arrow-right" size={12} />
          </button>
        {/if}
      </header>
      <PipelineChoice {camera} />
      <div class="why"><StatusChain status={camera.status} only={["pipeline", "detections"]} variant="why" /></div>
    </section>
  {:else}
    <Empty icon="camera" text="No cameras yet. Plug one in and it appears here." />
  {/each}
</div>

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  section {
    position: relative;
    border-bottom: 1px solid var(--line);
  }
  section.sel::before {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--c);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px 0 2px;
  }
  header > :global(.ident) {
    flex: 1;
    min-width: 0;
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11.5px;
    color: var(--accent-fg);
    white-space: nowrap;
  }
  .link:hover {
    text-decoration: underline;
  }
  .why {
    padding: 0 10px 8px;
  }
  .why:empty {
    display: none;
  }
</style>
