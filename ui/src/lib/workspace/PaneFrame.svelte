<script lang="ts">
  // Any pane, embedded in a page: a slim header carrying the pane's own
  // toolbar, then the pane. The same panes power pages and the Workbench.
  import { untrack, type Snippet } from "svelte";
  import Icon from "$lib/components/common/Icon.svelte";
  import { workspaces } from "$lib/core/workspace.svelte";
  import { setBarSlot } from "./bar";
  import { paneDef } from "./panes";

  let {
    id,
    type,
    props = {},
    title,
    header = true,
    lead,
  }: { id: string; type: string; props?: Record<string, unknown>; title?: string; header?: boolean; lead?: Snippet } = $props();

  // Created once per frame (it is keyed by id/type where used); page-given
  // props are pushed in by an effect, since state can't change mid-render.
  const ref = untrack(() => workspaces.embed(id, type, props));
  const def = untrack(() => paneDef(type));
  $effect(() => {
    const next = props;
    untrack(() => {
      if (Object.entries(next).some(([k, v]) => ref.props?.[k] !== v)) workspaces.setProps("embed", ref.id, next);
    });
  });
  let target = $state<HTMLElement | null>(null);
  setBarSlot({
    get target() {
      return target;
    },
    get active() {
      return true;
    },
  });
</script>

<div class="frame">
  {#if header}
    <div class="head">
      {#if lead}{@render lead()}{:else if title !== ""}
        <span class="title"><Icon name={def?.icon ?? "circle-dashed"} size={14} />{title ?? def?.title}</span>
      {/if}
      <div class="bar" bind:this={target}></div>
    </div>
  {/if}
  <div class="body">
    {#if def}
      <def.component pane={ref} ws="embed" />
    {/if}
  </div>
</div>

<style>
  .frame {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
    background: var(--s1);
    border: 1px solid var(--line);
    border-radius: var(--r-3);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 6px 0 12px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--line);
    background: var(--s2);
  }
  .title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg-2);
    white-space: nowrap;
  }
  .bar {
    flex: 1;
    display: flex;
    justify-content: flex-end;
    min-width: 0;
    overflow: hidden;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
</style>
