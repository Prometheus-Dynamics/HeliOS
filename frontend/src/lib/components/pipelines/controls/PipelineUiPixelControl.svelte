<script lang="ts">
  import ColorDropperButton from '$lib/components/controls/ColorDropperButton.svelte';

  type Props = {
    value: string;
    disabled?: boolean;
    onChange: (value: string) => void;
  };

  let { value, disabled = false, onChange }: Props = $props();
</script>

<div class="flex items-center gap-2">
  <input
    class="h-9 w-16 rounded border border-surface-700 bg-surface-900/70"
    type="color"
    value={value}
    disabled={disabled}
    oninput={(event) => onChange(event.currentTarget.value)}
  />
  <ColorDropperButton
    title="Pick color from screen"
    ariaLabel="Pick color from screen"
    disabled={disabled}
    onPick={(hex) => onChange(hex)}
  />
  <input
    class="flex-1 rounded border border-surface-700 bg-surface-900/70 px-2 py-1 text-xs"
    value={value}
    disabled={disabled}
    oninput={(event) => onChange(event.currentTarget.value)}
  />
</div>
