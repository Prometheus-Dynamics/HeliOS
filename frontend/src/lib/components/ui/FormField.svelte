<script lang="ts">
  import type { Snippet } from 'svelte';

  type FieldDensity = 'comfortable' | 'compact';
  const densityClasses: Record<FieldDensity, string> = {
    comfortable: 'gap-2',
    compact: 'gap-1'
  };

  type FormFieldProps = {
    id?: string;
    label?: string;
    description?: string;
    hint?: string;
    error?: string;
    required?: boolean;
    inline?: boolean;
    density?: FieldDensity;
    className?: string;
    labelClassName?: string;
    control?: Snippet;
    actions?: Snippet;
    footer?: Snippet;
  };

  const {
    id,
    label = '',
    description = '',
    hint = '',
    error = '',
    required = false,
    inline = false,
    density = 'comfortable',
    className = '',
    labelClassName = '',
    control,
    actions,
    footer
  }: FormFieldProps = $props();

  const labelId = $derived(id ? `${id}-label` : undefined);
  const descriptionId = $derived(id ? `${id}-description` : undefined);
  const hintId = $derived(id ? `${id}-hint` : undefined);
  const errorId = $derived(id ? `${id}-error` : undefined);

  export type $$Props = FormFieldProps;
</script>

<label
  for={id}
  class={`flex ${inline ? 'flex-row items-start justify-between gap-4' : 'flex-col'} ${densityClasses[density]} ${className}`.trim()}
  aria-labelledby={labelId}
>
  <div class="space-y-1">
    {#if label}
      <div class={`text-sm font-medium text-surface-200 ${labelClassName}`.trim()} id={labelId}>
        {label}
        {#if required}
          <span class="text-primary-400"> *</span>
        {/if}
      </div>
    {/if}
    {#if description}
      <p class="text-xs text-surface-500" id={descriptionId}>{description}</p>
    {/if}
  </div>

  <div class="flex min-w-0 flex-1 flex-col gap-2">
    {#if control}
      {@render control()}
    {/if}
    {#if actions}
      <div class="flex flex-wrap gap-2">
        {@render actions()}
      </div>
    {/if}
    {#if error}
      <p class="text-xs text-rose-400" id={errorId}>{error}</p>
    {:else if hint}
      <p class="text-xs text-surface-500" id={hintId}>{hint}</p>
    {/if}
    {#if footer}
      {@render footer()}
    {/if}
  </div>
</label>
