<script lang="ts">
  // Name, colour and icon for any object, inline: the swatch opens a palette
  // of colours and icons, the name edits in place.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, ID_COLORS, identity } from "#lib/core/identity.svelte.js";
  import type { IconName } from "#lib/ui/icons.js";

  let { id, fallbackName, fallbackIcon, sub }: { id: string; fallbackName: string; fallbackIcon: IconName; sub?: string } = $props();

  const ident = $derived(identity.get(id));
  const ICONS: IconName[] = ["camera", "aperture", "target", "box", "flask", "robot", "radar-2", "eye", "bolt", "bulb", "cube", "schema", "cpu", "server", "device-laptop", "map-pin", "tag", "hexagon", "compass", "gauge"];
  let open = $state(false);
  let el = $state<HTMLDivElement>();
</script>

<svelte:window onpointerdown={(e) => open && el && !el.contains(e.target as Node) && (open = false)} />

<div class="ident" bind:this={el} style:--c={colorVar(ident.color)}>
  <button type="button" class="swatch" onclick={() => (open = !open)} aria-label="Change colour and icon" data-tip="Colour and icon">
    <Icon name={ident.icon ?? fallbackIcon} size={16} stroke={2} />
  </button>
  <div class="text">
    <input
      class="name"
      value={ident.name ?? fallbackName}
      aria-label="Name"
      onchange={(e) => {
        const v = (e.currentTarget as HTMLInputElement).value.trim();
        identity.set(id, { name: v && v !== fallbackName ? v : undefined });
      }}
      onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()}
    />
    {#if sub}<span class="sub">{sub}</span>{/if}
  </div>
  {#if open}
    <div class="pop">
      <div class="colors">
        {#each ID_COLORS as c (c)}
          <button type="button" class="c" class:on={ident.color === c} style:background={colorVar(c)} aria-label="Colour {c}" onclick={() => identity.set(id, { color: c })}></button>
        {/each}
      </div>
      <div class="icons">
        {#each ICONS as i (i)}
          <button type="button" class="i" class:on={(ident.icon ?? fallbackIcon) === i} aria-label={i} onclick={() => identity.set(id, { icon: i })}>
            <Icon name={i} size={15} />
          </button>
        {/each}
      </div>
      <button type="button" class="reset" onclick={() => { identity.reset(id); open = false; }}>Reset to default</button>
    </div>
  {/if}
</div>

<style>
  .ident {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    min-width: 0;
  }
  .swatch {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: var(--r-2);
    color: var(--c);
    background: color-mix(in oklab, var(--c) 18%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--c) 40%, transparent);
    flex-shrink: 0;
  }
  .swatch:hover {
    background: color-mix(in oklab, var(--c) 28%, transparent);
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .name {
    width: 100%;
    padding: 1px 4px;
    margin-left: -4px;
    font: inherit;
    font-size: 14px;
    font-weight: 650;
    color: var(--fg);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-1);
    outline: none;
  }
  .name:hover {
    border-color: var(--line);
  }
  .name:focus {
    border-color: var(--accent-ring);
    background: var(--inset);
  }
  .sub {
    font-size: 11px;
    color: var(--fg-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pop {
    position: absolute;
    z-index: 50;
    top: calc(100% - 2px);
    left: 8px;
    width: 236px;
    padding: 8px;
    background: var(--s2);
    border: 1px solid var(--line-strong);
    border-radius: var(--r-2);
    box-shadow: var(--shadow);
  }
  .colors {
    display: grid;
    grid-template-columns: repeat(6, 1fr);
    gap: 5px;
    margin-bottom: 8px;
  }
  .c {
    height: 22px;
    border-radius: var(--r-1);
  }
  .c.on {
    box-shadow: 0 0 0 2px var(--s2), 0 0 0 4px var(--fg);
  }
  .icons {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
  }
  .i {
    display: grid;
    place-items: center;
    height: 28px;
    border-radius: var(--r-1);
    color: var(--fg-2);
  }
  .i:hover {
    background: var(--s3);
  }
  .i.on {
    color: var(--c);
    background: color-mix(in oklab, var(--c) 18%, transparent);
  }
  .reset {
    margin-top: 6px;
    width: 100%;
    font-size: 11.5px;
    color: var(--fg-3);
    padding: 4px;
    border-radius: var(--r-1);
  }
  .reset:hover {
    background: var(--s3);
    color: var(--fg);
  }
</style>
