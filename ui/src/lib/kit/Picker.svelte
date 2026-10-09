<script lang="ts">
  // An object picker for a pane toolbar: identity swatch + name, opens a menu
  // of the options (each with its colour). Shows a pin when pinned.
  import Icon from "#lib/components/common/Icon.svelte";
  import { colorVar, identity } from "#lib/core/identity.svelte.js";
  import { menu } from "#lib/core/menu.svelte.js";
  import type { IconName } from "#lib/ui/icons.js";

  let {
    value,
    options,
    onpick,
    icon,
    pinned,
    ontogglepin,
    label,
  }: {
    value: string | undefined;
    options: { id: string; name: string; hint?: string }[];
    onpick: (id: string) => void;
    icon: IconName;
    pinned?: boolean;
    ontogglepin?: () => void;
    label: string;
  } = $props();

  const current = $derived(options.find((o) => o.id === value));
</script>

<span class="picker">
  <button
    type="button"
    class="pick"
    aria-label={label}
    onclick={(e) =>
      menu.below(e.currentTarget, [
        { heading: label },
        ...options.map((o) => ({ label: o.name, hint: o.hint, color: colorVar(identity.get(o.id).color), run: () => onpick(o.id) })),
      ])}
  >
    <span class="sw" style:background={value ? colorVar(identity.get(value).color) : "var(--fg-4)"}></span>
    <Icon name={identity.get(value ?? "").icon ?? icon} size={13} />
    <span class="name">{current?.name ?? "None"}</span>
    <Icon name="chevron-down" size={11} stroke={2.2} />
  </button>
  {#if ontogglepin}
    <button type="button" class="pin" class:on={pinned} onclick={ontogglepin} aria-label={pinned ? "Unpin" : "Pin"} data-tip={pinned ? "Pinned. Click to follow the selection again." : "Following the selection. Click to pin this one."}>
      <Icon name={pinned ? "pinned" : "pin"} size={12} />
    </button>
  {/if}
</span>

<style>
  .picker {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    min-width: 0;
  }
  .pick {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 22px;
    padding: 0 6px;
    min-width: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg);
    border-radius: var(--r-1);
  }
  .pick:hover {
    background: var(--s3);
  }
  .sw {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }
  .pin {
    display: inline-flex;
    padding: 4px;
    color: var(--fg-4);
    border-radius: var(--r-1);
  }
  .pin:hover {
    color: var(--fg);
    background: var(--s3);
  }
  .pin.on {
    color: var(--accent-fg);
  }
</style>
