<script lang="ts">
  // The right control for a catalog parameter.
  import type { Param } from "$lib/api/catalog";
  import Choice from "./Choice.svelte";
  import Num from "./Num.svelte";
  import Slider from "./Slider.svelte";
  import Switch from "./Switch.svelte";

  let { param, value, onchange }: { param: Param; value: number | string | boolean; onchange: (v: number | string | boolean) => void } = $props();
</script>

{#if param.kind === "bool"}
  <Switch checked={Boolean(value)} label={param.name} onchange={onchange} />
{:else if param.kind === "enum"}
  <Choice value={String(value)} options={param.options} label={param.name} onchange={onchange} />
{:else if param.kind === "int"}
  {#if param.max - param.min <= 64}
    <Slider value={Number(value)} min={param.min} max={param.max} step={1} label={param.name} {onchange} />
  {:else}
    <Num value={Number(value)} min={param.min} max={param.max} step={1} label={param.name} width={80} {onchange} />
  {/if}
{:else}
  <Slider value={Number(value)} min={param.min} max={param.max} step={param.step} label={param.name} {onchange} />
{/if}
