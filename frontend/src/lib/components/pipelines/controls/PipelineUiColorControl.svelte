<script lang="ts">
  import ColorWheelPicker from '$lib/components/controls/ColorWheelPicker.svelte';

  type Props = {
    value: string;
    textValue: string;
    disabled?: boolean;
    onChange: (value: string) => void;
  };

  let { value, textValue, disabled = false, onChange }: Props = $props();

  let isOpen = $state(false);
  let colorButton = $state<HTMLButtonElement | null>(null);
  let colorPopover = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!isOpen) return;
    const handler = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (colorPopover?.contains(target ?? null) || colorButton?.contains(target ?? null)) return;
      isOpen = false;
    };
    window.addEventListener('pointerdown', handler);
    return () => {
      window.removeEventListener('pointerdown', handler);
    };
  });
</script>

<div class="space-y-2">
  <div class="flex items-center gap-2">
    <button
      class="h-9 w-9 rounded border border-surface-700 bg-surface-900/70"
      type="button"
      aria-label="Open color picker"
      disabled={disabled}
      bind:this={colorButton}
      onclick={() => (isOpen = !isOpen)}
    >
      <span class="color-swatch block h-full w-full rounded" style={`--swatch-color:${value};`}></span>
    </button>
    <input
      class="flex-1 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
      value={textValue}
      disabled={disabled}
      oninput={(event) => onChange((event.currentTarget as HTMLInputElement).value)}
    />
  </div>
  {#if isOpen}
    <div class="relative">
      <div class="color-popover rounded border border-surface-800/70 bg-surface-950/95 p-3 shadow-lg" bind:this={colorPopover}>
        <ColorWheelPicker
          value={value}
          size={180}
          disabled={disabled}
          on:change={(event) => onChange(event.detail.value)}
        />
      </div>
    </div>
  {/if}
</div>
