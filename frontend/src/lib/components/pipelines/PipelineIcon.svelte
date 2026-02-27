<script lang="ts">
  import { createEventDispatcher } from 'svelte';
  import FaIcon from '$lib/components/icons/FaIcon.svelte';
  import { pipelineIconConfig } from '$lib/features/pipelines/iconCatalog';
  import type { PipelineAppearance } from '$lib/types/pipeline';

  type Size = 'sm' | 'md' | 'lg';
  type ElementTag = 'div' | 'span' | 'button';

  const props = $props<{
    pipelineId: string | null | undefined;
    appearance?: PipelineAppearance | null;
    revision?: string | null;
    as?: ElementTag;
    size?: Size;
    className?: string;
    ariaLabel?: string | null;
    title?: string | null;
    stopPropagation?: boolean;
    borderClass?: string;
  }>();

  const pipelineId = $derived(props.pipelineId ?? '');
  const appearance = $derived(props.appearance ?? null);
  const revision = $derived(props.revision ?? null);
  const as = $derived(props.as ?? ('div' satisfies ElementTag));
  const size = $derived(props.size ?? ('md' satisfies Size));
  const className = $derived(props.className ?? '');
  const ariaLabel = $derived(props.ariaLabel ?? null);
  const title = $derived(props.title ?? null);
  const stopPropagation = $derived(Boolean(props.stopPropagation));
  const borderClass = $derived(props.borderClass ?? 'border-white/20');

  const dispatch = createEventDispatcher<{
    click: MouseEvent;
    keydown: KeyboardEvent;
  }>();

  const icon = $derived(pipelineIconConfig(pipelineId, appearance, revision));

  const sizeClass = $derived(
    size === 'lg' ? 'h-12 w-12 text-base' : size === 'sm' ? 'h-8 w-8 text-sm' : 'h-10 w-10 text-sm'
  );
  const interactiveClass = $derived(
    as === 'button' ? 'focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-300' : ''
  );
  const combinedClass = $derived(
    `flex items-center justify-center rounded-full border text-white shadow-inner shadow-black/30 ${borderClass} ${sizeClass} ${interactiveClass} ${className}`
      .replace(/\s+/g, ' ')
      .trim()
  );
  const elementRole = $derived(as === 'button' ? undefined : 'button');
  const elementTabIndex = $derived(as === 'button' ? undefined : 0);

  function handleClick(event: MouseEvent) {
    if (stopPropagation) {
      event.stopPropagation();
    }
    dispatch('click', event);
  }

  function handleKeydown(event: KeyboardEvent) {
    dispatch('keydown', event);
  }
</script>

{#if as === 'button'}
  <button
    class={combinedClass}
    style={`background:${icon.color};`}
    aria-label={ariaLabel ?? undefined}
    title={title ?? undefined}
    type="button"
    onclick={handleClick}
    onkeydown={handleKeydown}
  >
    <FaIcon icon={icon.option.icon} class="h-[1em] w-[1em]" />
  </button>
{:else}
  <svelte:element
    this={as}
    class={combinedClass}
    style={`background:${icon.color};`}
    aria-label={ariaLabel ?? undefined}
    title={title ?? undefined}
    role={elementRole}
    tabindex={elementTabIndex}
    onclick={handleClick}
    onkeydown={handleKeydown}
  >
    <FaIcon icon={icon.option.icon} class="h-[1em] w-[1em]" />
  </svelte:element>
{/if}
