<script lang="ts">
  // Renders the open menu, clamped inside the window. Keyboard: arrows, Enter,
  // Escape, Right/Left for submenus.
  import Icon from "#lib/components/common/Icon.svelte";
  import { menu, type MenuItem } from "#lib/core/menu.svelte.js";

  let el = $state<HTMLDivElement>();
  let sub = $state<{ index: number; top: number } | null>(null);
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    const m = menu.current;
    sub = null;
    if (!m || !el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let left = m.x < 0 ? -m.x - w : m.x;
    left = Math.max(4, Math.min(window.innerWidth - w - 4, left));
    let top = m.y;
    if (top + h > window.innerHeight - 4) top = Math.max(4, window.innerHeight - h - 4);
    pos = { left, top };
    el.focus();
  });

  function run(item: MenuItem) {
    if ("separator" in item || "heading" in item || item.disabled) return;
    if (item.items) return;
    menu.close();
    item.run?.();
  }

  function keydown(event: KeyboardEvent) {
    if (!menu.current) return;
    if (event.key === "Escape") {
      event.preventDefault();
      menu.close();
      return;
    }
    const buttons = [...(el?.querySelectorAll<HTMLButtonElement>("button.item:not(:disabled)") ?? [])];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = event.key === "ArrowDown" ? (i + 1) % buttons.length : (i - 1 + buttons.length) % buttons.length;
      buttons[next]?.focus();
    }
  }
</script>

<svelte:window
  onkeydown={keydown}
  onpointerdown={(e) => {
    if (menu.current && el && !el.contains(e.target as Node)) menu.close();
  }}
  onblur={() => menu.close()}
  onresize={() => menu.close()}
/>

{#snippet list(items: MenuItem[], isSub: boolean)}
  {#each items as item, i (i)}
    {#if "separator" in item}
      <div class="sep"></div>
    {:else if "heading" in item}
      <div class="heading">{item.heading}</div>
    {:else}
      <button
        type="button"
        class="item"
        class:danger={item.danger}
        class:open={!isSub && sub?.index === i}
        disabled={item.disabled}
        onclick={() => run(item)}
        onpointerenter={(e) => {
          if (isSub) return;
          sub = item.items ? { index: i, top: (e.currentTarget as HTMLElement).offsetTop } : null;
        }}
        title={item.hint}
      >
        <span class="lead">
          {#if item.checked !== undefined}
            {#if item.checked}<Icon name="check" size={13} stroke={2.2} />{/if}
          {:else if item.color}
            <span class="dot" style:background={item.color}></span>
          {:else if item.icon}
            <Icon name={item.icon} size={14} />
          {/if}
        </span>
        <span class="label">{item.label}</span>
        {#if item.shortcut}<kbd>{item.shortcut}</kbd>{/if}
        {#if item.items}<Icon name="chevron-right" size={12} class="more" />{/if}
      </button>
    {/if}
  {/each}
{/snippet}

{#if menu.current}
  {@const m = menu.current}
  <div class="menu" bind:this={el} tabindex="-1" role="menu" style:left="{pos.left}px" style:top="{pos.top}px" style:min-width="{m.minWidth}px">
    {@render list(m.items, false)}
    {#if sub}
      {@const parent = m.items[sub.index]}
      {#if parent && !("separator" in parent) && !("heading" in parent) && parent.items}
        <div class="menu submenu" role="menu" style:top="{sub.top - 4}px">
          {@render list(parent.items, true)}
        </div>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 1000;
    padding: 4px;
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    background: var(--s2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    box-shadow: var(--shadow);
    outline: none;
    font-size: 12.5px;
  }
  .submenu {
    position: absolute;
    left: calc(100% + 2px);
    min-width: 180px;
    overflow: visible;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px 0 4px;
    border-radius: var(--r-1);
    color: var(--fg);
    text-align: left;
    white-space: nowrap;
  }
  .item:hover:not(:disabled),
  .item:focus-visible,
  .item.open {
    background: var(--accent-tint-strong);
    outline: none;
  }
  .item:disabled {
    color: var(--fg-4);
  }
  .item.danger {
    color: var(--err);
  }
  .lead {
    display: inline-flex;
    justify-content: center;
    width: 18px;
    color: var(--fg-2);
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .label {
    flex: 1;
  }
  kbd {
    font-family: var(--font-code);
    font-size: 10.5px;
    color: var(--fg-3);
  }
  .item :global(.more) {
    color: var(--fg-3);
  }
  .sep {
    height: 1px;
    margin: 4px 2px;
    background: var(--line);
  }
  .heading {
    padding: 6px 8px 3px 26px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--fg-3);
  }
</style>
