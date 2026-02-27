<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { faEyeDropper } from '@fortawesome/free-solid-svg-icons';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { pickEyeDropperColor, supportsEyeDropper } from '$lib/utils/eyeDropper';

  type Props = {
    onPick?: (hex: string) => void;
    className?: string;
    title?: string;
    ariaLabel?: string;
    disabled?: boolean;
    children?: Snippet;
  };

  let {
    onPick = () => {},
    className = 'btn btn-3xs preset-ghost',
    title = 'Pick color from screen',
    ariaLabel = 'Pick color from screen',
    disabled = false,
    children
  }: Props = $props();

  let supported = $state(false);
  let picking = $state(false);

  onMount(() => {
    supported = supportsEyeDropper();
  });

  const handleClick = async () => {
    if (disabled || picking) return;
    picking = true;
    try {
      const color = await pickEyeDropperColor();
      if (color) onPick(color);
    } finally {
      picking = false;
    }
  };

  const isDisabled = $derived(disabled || picking);
  const resolvedTitle = $derived(supported ? title : `${title} (unsupported)`);
</script>

<button
  type="button"
  class={`${className} color-dropper`}
  title={resolvedTitle}
  aria-label={ariaLabel}
  disabled={isDisabled}
  data-eye-dropper={supported ? 'true' : 'false'}
  onclick={handleClick}
>
  {#if children}
    <span class="sr-only">{@render children()}</span>
  {/if}
  <FaIcon icon={faEyeDropper} class="color-dropper__icon" ariaHidden="true" />
</button>

<style>
  .color-dropper {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 2rem;
    min-height: 2rem;
  }

  .color-dropper__icon {
    width: 0.9rem;
    height: 0.9rem;
  }
</style>
