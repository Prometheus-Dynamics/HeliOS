<script lang="ts">
  // What each camera runs, all cameras at once: the setup step's screen. Each camera picks from
  // the default pipelines; tuning a pipeline afterwards is the pipeline's own settings.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { guide } from "#lib/core/guide.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import { shell } from "#lib/core/shell.svelte.js";
  import PipelineChoice from "#lib/camera/PipelineChoice.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";

  let {}: PaneProps = $props();
</script>

<div class="scroll">
  {#each cluster.cameras as camera (camera.resourceId)}
    {@const running = guide.active(camera)}
    <section style:--c={colorVar(identity.get(camera.resourceId).color)}>
      <header>
        <span class="dot"></span>
        <b>{identity.name(camera.resourceId, camera.name)}</b>
        <span class="sub">{cluster.node(camera.nodeId)?.name ?? camera.nodeId}</span>
        {#if running}
          <button type="button" class="link" onclick={() => { selection.select({ kind: "workload", id: running.id }); shell.go("pipelines"); }}>
            Tune pipeline <Icon name="arrow-right" size={12} />
          </button>
        {/if}
      </header>
      <PipelineChoice {camera} />
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
    border-bottom: 1px solid var(--line);
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px 2px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--c);
    flex-shrink: 0;
  }
  b {
    font-size: 13px;
    font-weight: 650;
  }
  .sub {
    flex: 1;
    min-width: 0;
    font-size: 11px;
    color: var(--fg-3);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11.5px;
    color: var(--accent-fg);
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
