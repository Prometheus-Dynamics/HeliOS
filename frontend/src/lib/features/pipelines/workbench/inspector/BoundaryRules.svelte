<script lang="ts">
  const {
    direction,
    name,
    variants = [],
    settable = false,
    draftValue = '',
    error = null,
    policy = 'NewestWins',
    capacity = 3,
    channelPolicyOptions = [],
    onPolicyChange,
    onCapacityChange,
    onDraftChange,
    onApply,
    onClear
  }: {
    direction: 'input' | 'output';
    name: string;
    variants?: string[];
    settable?: boolean;
    draftValue?: string;
    error?: string | null;
    policy?: string;
    capacity?: number;
    channelPolicyOptions?: Array<{ value: string; label: string }>;
    onPolicyChange?: (value: string) => void;
    onCapacityChange?: (capacity: number) => void;
    onDraftChange?: (value: string) => void;
    onApply?: () => void;
    onClear?: () => void;
  } = $props();
</script>

{#if direction === 'input'}
  <div class="grid gap-2 md:grid-cols-2 text-micro text-surface-400">
    <label class="flex flex-col gap-1">
      <span>Policy</span>
      <select
        class="input h-8 text-xs"
        value={policy}
        onchange={(event) => onPolicyChange?.((event.currentTarget as HTMLSelectElement).value)}
      >
        {#each channelPolicyOptions as option}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </label>
    <label class="flex flex-col gap-1">
      <span>Capacity</span>
      <input
        class="input h-8 text-xs"
        type="number"
        min="1"
        value={capacity}
        oninput={(event) => {
          const next = Number.parseInt((event.currentTarget as HTMLInputElement).value, 10);
          if (Number.isFinite(next) && next > 0) {
            onCapacityChange?.(next);
          }
        }}
      />
    </label>
  </div>
  <div class="space-y-2 text-micro text-surface-500 mt-3">
    {#if settable}
      <label class="flex flex-col gap-1">
        <span>Value</span>
        {#if variants.length > 0}
          <select
            class="input h-8 text-xs"
            value={draftValue ?? variants[0] ?? ''}
            onchange={(event) => onDraftChange?.((event.currentTarget as HTMLSelectElement).value)}
          >
            {#each variants as option (option)}
              <option value={option}>{option}</option>
            {/each}
          </select>
        {:else}
          <input
            class="input h-8 text-xs"
            placeholder="Enter value"
            value={draftValue ?? ''}
            oninput={(event) => onDraftChange?.((event.currentTarget as HTMLInputElement).value)}
          />
        {/if}
      </label>
      {#if error}
        <p class="text-error-300">{error}</p>
      {/if}
      <div class="flex flex-wrap gap-2">
        <button
          class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
          type="button"
          onclick={() => onApply?.()}
        >
          Apply
        </button>
        <button
          class="btn btn-3xs preset-outline uppercase tracking-[0.3em]"
          type="button"
          onclick={() => onClear?.()}
        >
          Clear
        </button>
      </div>
    {:else}
      <p class="text-micro text-surface-500">This port is not settable.</p>
    {/if}
  </div>
{:else}
  <label class="flex items-center justify-between gap-3 text-surface-400">
    <span>{name}</span>
    <input
      class="input h-8 w-24 text-xs"
      type="number"
      min="1"
      value={capacity}
      oninput={(event) => {
        const next = Number.parseInt((event.currentTarget as HTMLInputElement).value, 10);
        if (Number.isFinite(next) && next > 0) {
          onCapacityChange?.(next);
        }
      }}
    />
  </label>
{/if}
