<script lang="ts">
  // An object's identity mark: its colour, optionally with its icon.
  import Icon from "$lib/components/common/Icon.svelte";
  import { colorVar, identity } from "$lib/core/identity.svelte";
  import type { IconName } from "$lib/ui/icons";

  let { id, size = 18, icon: fallbackIcon, solid = false }: { id: string; size?: number; icon?: IconName; solid?: boolean } = $props();
  const ident = $derived(identity.get(id));
  const color = $derived(colorVar(ident.color));
  const icon = $derived(ident.icon ?? fallbackIcon);
</script>

{#if icon}
  <span
    class="sw"
    style:width="{size}px"
    style:height="{size}px"
    style:color={solid ? "var(--s0)" : color}
    style:background={solid ? color : `color-mix(in oklab, ${color} 18%, transparent)`}
  >
    <Icon name={icon} size={Math.round(size * 0.66)} stroke={2} />
  </span>
{:else}
  <span class="dot" style:width="{size * 0.5}px" style:height="{size * 0.5}px" style:background={color}></span>
{/if}

<style>
  .sw {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--r-1);
    flex-shrink: 0;
  }
  .dot {
    display: inline-block;
    border-radius: 50%;
    flex-shrink: 0;
  }
</style>
