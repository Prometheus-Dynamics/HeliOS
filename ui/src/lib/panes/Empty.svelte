<script lang="ts">
  import Icon from "$lib/components/common/Icon.svelte";
  import type { IconName } from "$lib/ui/icons";
  import type { Snippet } from "svelte";
  let { icon = "circle-dashed", text, children }: { icon?: IconName; text: string; children?: Snippet } = $props();
</script>

<div class="empty">
  <Icon name={icon} size={22} stroke={1.5} />
  <p>{text}</p>
  {@render children?.()}
</div>

<style>
  .empty {
    margin: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 20px;
    max-width: 320px;
    text-align: center;
    color: var(--fg-3);
    font-size: 12.5px;
  }
</style>
