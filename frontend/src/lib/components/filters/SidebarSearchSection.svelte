<script lang="ts">
  import SearchInputField from './SearchInputField.svelte';

  type SidebarSearchSectionProps = {
    label?: string;
    placeholder?: string;
    description?: string | null;
    value?: string;
    size?: 'default' | 'compact';
    ariaLabel?: string;
    autofocus?: boolean;
  };

  let {
    label = 'Search',
    placeholder = 'Search',
    description = null,
    value = $bindable(''),
    size = 'compact',
    ariaLabel,
    autofocus = false
  }: SidebarSearchSectionProps = $props();

  const resolvedAriaLabel = $derived.by(() => ariaLabel ?? label);
  const labelClass = $derived.by(() =>
    size === 'compact'
      ? 'text-[0.62rem] uppercase tracking-[0.2em] text-surface-500'
      : 'text-micro uppercase tracking-[0.3em] text-surface-500'
  );
  const descriptionClass = $derived.by(() =>
    size === 'compact' ? 'text-[0.62rem] text-surface-500' : 'text-micro-tight text-surface-500'
  );
  const sectionClass = $derived.by(() => (size === 'compact' ? 'space-y-1.5' : 'space-y-2'));

  export type $$Props = SidebarSearchSectionProps;
</script>

<div class={sectionClass}>
  <div class="flex items-center justify-between gap-3">
    <p class={labelClass}>{label}</p>
    {#if description}
      <p class={descriptionClass}>{description}</p>
    {/if}
  </div>
  <SearchInputField
    bind:value
    {placeholder}
    ariaLabel={resolvedAriaLabel}
    {size}
    {autofocus}
  />
</div>
