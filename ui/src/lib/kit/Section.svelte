<script lang="ts">
  // A collapsible group of rows. Remembers open/closed per `key`. `advanced`
  // sections are labelled so beginners know they can skip them.
  import { untrack, type Snippet } from "svelte";
  import Icon from "#lib/components/common/Icon.svelte";
  import { loadRaw, save } from "#lib/core/persist.js";

  let {
    title,
    key,
    open: initial = true,
    advanced = false,
    count,
    actions,
    children,
  }: { title: string; key?: string; open?: boolean; advanced?: boolean; count?: number | string; actions?: Snippet; children: Snippet } = $props();

  // A section's key and default never change after it is created.
  const first = untrack(() => ({ key, start: initial }));
  const storeKey = first.key ? `section:${first.key}` : null;
  let open = $state(storeKey ? loadRaw(storeKey, first.start) : first.start);
  function toggle() {
    open = !open;
    if (storeKey) save(storeKey, open);
  }
</script>

<section class="sec">
  <header>
    <button type="button" class="head" onclick={toggle} aria-expanded={open}>
      <Icon name="chevron-right" size={12} stroke={2.2} class="chev {open ? 'open' : ''}" />
      <span class="title">{title}</span>
      {#if advanced}<span class="adv">advanced</span>{/if}
      {#if count !== undefined}<span class="count">{count}</span>{/if}
    </button>
    {#if actions}<div class="acts">{@render actions()}</div>{/if}
  </header>
  {#if open}
    <div class="body">{@render children()}</div>
  {/if}
</section>

<style>
  .sec {
    border-bottom: 1px solid var(--line);
  }
  header {
    display: flex;
    align-items: center;
    padding-right: 6px;
  }
  .head {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 5px;
    height: var(--row);
    padding: 0 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--fg-2);
    text-align: left;
  }
  .head:hover {
    color: var(--fg);
  }
  :global(.chev) {
    transition: transform var(--t-fast);
    color: var(--fg-3);
  }
  :global(.chev.open) {
    transform: rotate(90deg);
  }
  .adv {
    font-size: 9.5px;
    letter-spacing: 0.06em;
    padding: 1px 5px;
    border-radius: var(--r-1);
    color: var(--fg-3);
    border: 1px solid var(--line);
  }
  .count {
    margin-left: auto;
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
    letter-spacing: 0;
  }
  .acts {
    display: flex;
    gap: 2px;
  }
  .body {
    padding: 0 0 6px;
  }
</style>
