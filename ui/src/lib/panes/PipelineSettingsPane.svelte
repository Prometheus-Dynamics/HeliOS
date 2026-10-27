<script lang="ts">
  // One pipeline's settings (pipeline/PipelineSettings.svelte): follows the selected pipeline, or
  // stays pinned to one. A node selected in the graph opens that stage's section here.
  import { identity } from "#lib/core/identity.svelte.js";
  import { selection } from "#lib/core/selection.svelte.js";
  import Picker from "#lib/kit/Picker.svelte";
  import PipelineSettings from "#lib/pipeline/PipelineSettings.svelte";
  import { cluster } from "#lib/stores/cluster.svelte.js";
  import PaneBar from "#lib/workspace/PaneBar.svelte";
  import type { PaneProps } from "#lib/workspace/panes.js";
  import Empty from "./Empty.svelte";
  import { follow } from "./follow.svelte";

  let { pane, ws }: PaneProps = $props();
  const target = follow(() => pane, () => ws, "workload", () => cluster.workloads[0]?.id);
  // A graph node is selected within its pipeline: that pipeline, with the node's stage in front.
  const node = $derived(selection.current?.kind === "graph-node" && selection.current.parent === target.id ? selection.current.id : null);
  const workload = $derived(target.id ? cluster.workload(target.id) : undefined);
</script>

<PaneBar>
  <Picker
    label="Pipeline"
    icon="schema"
    value={workload?.id}
    options={cluster.workloads.map((w) => ({ id: w.id, name: identity.name(w.id, w.name), hint: cluster.node(w.nodeId)?.name }))}
    onpick={(id) => (target.pinned ? target.pin(id) : selection.select({ kind: "workload", id }))}
    pinned={target.pinned}
    ontogglepin={() => target.toggle()}
  />
</PaneBar>

{#if workload}
  <div class="scroll">
    {#key workload.id}
      <PipelineSettings {workload} focusNode={node} />
    {/key}
  </div>
{:else if cluster.workloads.length}
  <Empty icon="settings-automation" text="Pick a pipeline in the tab bar, or click one in the Pipelines list." />
{:else}
  <Empty icon="settings-automation" text="No pipeline yet. Choose what each camera runs on the Set up screen and its pipeline appears here." />
{/if}

<style>
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }
</style>
